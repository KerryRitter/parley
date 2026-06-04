//! Claude Code sessions: `~/.claude/projects/<slug>/<session-id>.jsonl`,
//! where `<slug>` is the cwd with every non-alphanumeric char mapped to `-`.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{canonical, file_mtime_ms, home_dir, SessionRef, SessionStore};
use crate::harness::Invocation;
use crate::json::Json;

pub(crate) struct ClaudeSessions;

impl SessionStore for ClaudeSessions {
    fn harness(&self) -> &'static str {
        "claude"
    }

    fn list(&self, cwd: &Path) -> Result<Vec<SessionRef>, String> {
        let home = match home_dir() {
            Some(h) => h,
            None => return Ok(Vec::new()),
        };
        let dir = home
            .join(".claude")
            .join("projects")
            .join(slug_for_cwd(cwd));
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(stem) => stem.to_string(),
                None => continue,
            };
            let (title, count) = summarize(&path);
            sessions.push(SessionRef {
                harness: "claude".to_string(),
                id,
                cwd: cwd.display().to_string(),
                updated_ms: file_mtime_ms(&path),
                title,
                message_count: count,
                delegated: false,
            });
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        Ok(sessions)
    }

    fn resume_invocation(&self, id: &str, _cwd: &Path, yolo: bool) -> Result<Invocation, String> {
        if id.is_empty() {
            return Err("claude resume requires a session id".to_string());
        }
        let mut args = vec!["--resume".to_string(), id.to_string()];
        if yolo {
            args.push("--dangerously-skip-permissions".to_string());
        }
        Ok(Invocation::new("claude", args))
    }
}

/// Map a cwd to Claude's project directory name: every char outside
/// `[A-Za-z0-9]` becomes `-` (so `/home/x/my.app` → `-home-x-my-app`).
pub(crate) fn slug_for_cwd(cwd: &Path) -> String {
    canonical(cwd)
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Best-effort title (a `summary` line if present, else the first real user
/// prompt) plus a rough message count. Stops scanning once a summary is found.
fn summarize(path: &Path) -> (String, Option<usize>) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return ("(unreadable)".to_string(), None),
    };
    let reader = BufReader::new(file);

    let mut summary: Option<String> = None;
    let mut first_user: Option<String> = None;
    let mut messages = 0usize;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let json = match Json::parse(line) {
            Ok(j) => j,
            Err(_) => continue,
        };
        let kind = json.get("type").and_then(Json::as_str).unwrap_or("");
        match kind {
            "summary" => {
                if let Some(s) = json.get("summary").and_then(Json::as_str) {
                    summary = Some(s.to_string());
                    break;
                }
            }
            "user" => {
                messages += 1;
                if first_user.is_none() {
                    if let Some(text) = user_text(&json) {
                        if !text.trim().is_empty() && !text.trim_start().starts_with('<') {
                            first_user = Some(text);
                        }
                    }
                }
            }
            "assistant" => messages += 1,
            _ => {}
        }
    }

    let title = summary
        .or(first_user)
        .unwrap_or_else(|| "(no summary)".to_string());
    let count = if messages > 0 { Some(messages) } else { None };
    (title, count)
}

/// Extract the text of a Claude `user` line. `content` is either a string or an
/// array of `{type:"text", text:"..."}` blocks.
fn user_text(json: &Json) -> Option<String> {
    let content = json.get("message")?.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let arr = content.as_array()?;
    for block in arr {
        if block.get("type").and_then(Json::as_str) == Some("text") {
            if let Some(text) = block.get("text").and_then(Json::as_str) {
                return Some(text.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn slug_maps_non_alnum_to_dash() {
        let slug = slug_for_cwd(&PathBuf::from("/home/kerry/Work/my.app_v2"));
        // Canonicalize falls back to the raw path when it doesn't exist.
        assert_eq!(slug, "-home-kerry-Work-my-app-v2");
    }
}
