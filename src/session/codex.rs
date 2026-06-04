//! Codex sessions: `~/.codex/sessions/Y/M/D/rollout-*.jsonl`. The first line is
//! a `session_meta` record carrying the launch `cwd` and session `id`.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::{canonical, file_mtime_ms, home_dir, SessionRef, SessionStore};
use crate::harness::Invocation;
use crate::json::Json;

pub(crate) struct CodexSessions;

impl SessionStore for CodexSessions {
    fn harness(&self) -> &'static str {
        "codex"
    }

    fn list(&self, cwd: &Path) -> Result<Vec<SessionRef>, String> {
        let home = match home_dir() {
            Some(h) => h,
            None => return Ok(Vec::new()),
        };
        let root = home.join(".codex").join("sessions");
        if !root.is_dir() {
            return Ok(Vec::new());
        }

        let target = canonical(cwd).to_string_lossy().to_string();
        let raw_target = cwd.to_string_lossy().to_string();

        let mut rollouts = Vec::new();
        collect_jsonl(&root, &mut rollouts);

        let mut sessions = Vec::new();
        for path in rollouts {
            if let Some(session) = read_session(&path, &target, &raw_target) {
                sessions.push(session);
            }
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        Ok(sessions)
    }

    fn resume_invocation(&self, id: &str, _cwd: &Path, _yolo: bool) -> Result<Invocation, String> {
        // `codex resume` opens an interactive session and (unlike `codex exec`)
        // does not accept `--yolo`; the user approves within the TUI.
        if id.is_empty() {
            return Ok(Invocation::new(
                "codex",
                vec!["resume".to_string(), "--last".to_string()],
            ));
        }
        Ok(Invocation::new(
            "codex",
            vec!["resume".to_string(), id.to_string()],
        ))
    }
}

/// Recursively collect `rollout-*.jsonl` files under `dir`.
fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

/// Parse the leading `session_meta` line; return a SessionRef only when the
/// session's cwd matches the target.
fn read_session(path: &Path, target: &str, raw_target: &str) -> Option<SessionRef> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    let meta = Json::parse(first.trim()).ok()?;
    if meta.get("type").and_then(Json::as_str) != Some("session_meta") {
        return None;
    }
    let payload = meta.get("payload")?;
    let session_cwd = payload.get("cwd").and_then(Json::as_str)?;
    if session_cwd != target && session_cwd != raw_target {
        return None;
    }
    let id = payload.get("id").and_then(Json::as_str)?.to_string();

    let title = first_user_prompt(&mut reader).unwrap_or_else(|| "(codex session)".to_string());

    Some(SessionRef {
        harness: "codex".to_string(),
        id,
        cwd: session_cwd.to_string(),
        updated_ms: file_mtime_ms(path),
        title,
        message_count: None,
        delegated: false,
    })
}

/// Scan the next chunk of lines for the first genuine user prompt, skipping the
/// injected permission/instruction blocks (which start with `<`).
fn first_user_prompt(reader: &mut BufReader<File>) -> Option<String> {
    for line in reader.lines().map_while(Result::ok).take(80) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let json = match Json::parse(line) {
            Ok(j) => j,
            Err(_) => continue,
        };
        let payload = match json.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let ptype = payload.get("type").and_then(Json::as_str).unwrap_or("");
        // Event-stream user message.
        if ptype == "user_message" {
            if let Some(msg) = payload.get("message").and_then(Json::as_str) {
                if !msg.trim_start().starts_with('<') {
                    return Some(msg.to_string());
                }
            }
        }
        // Response-item user message with input_text content.
        if ptype == "message" && payload.get("role").and_then(Json::as_str) == Some("user") {
            if let Some(text) = input_text(payload) {
                if !text.trim_start().starts_with('<') {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn input_text(payload: &Json) -> Option<String> {
    let arr = payload.get("content")?.as_array()?;
    for block in arr {
        if block.get("type").and_then(Json::as_str) == Some("input_text") {
            if let Some(text) = block.get("text").and_then(Json::as_str) {
                return Some(text.to_string());
            }
        }
    }
    None
}
