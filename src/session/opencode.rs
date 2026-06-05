//! OpenCode sessions: `~/.local/share/opencode/storage/session/<projectID>/<id>.json`.
//! Each file records its `directory` (exact cwd), `title`, `id`, and `time.updated`.

use std::fs::{self};
use std::path::Path;

use super::{canonical, home_dir, SessionRef, SessionStore, Turn};
use crate::harness::Invocation;
use crate::json::Json;

pub(crate) struct OpencodeSessions;

impl SessionStore for OpencodeSessions {
    fn harness(&self) -> &'static str {
        "opencode"
    }

    fn list(&self, cwd: &Path) -> Result<Vec<SessionRef>, String> {
        let home = match home_dir() {
            Some(h) => h,
            None => return Ok(Vec::new()),
        };
        let root = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("storage")
            .join("session");
        if !root.is_dir() {
            return Ok(Vec::new());
        }

        let target = canonical(cwd).to_string_lossy().to_string();
        let raw_target = cwd.to_string_lossy().to_string();

        let mut sessions = Vec::new();
        // storage/session/<projectID>/<sessionID>.json
        for project in fs::read_dir(&root).into_iter().flatten().flatten() {
            let pdir = project.path();
            if !pdir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&pdir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let raw = match fs::read_to_string(&path) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let json = match Json::parse(&raw) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                let directory = match json.get("directory").and_then(Json::as_str) {
                    Some(d) => d,
                    None => continue,
                };
                if directory != target && directory != raw_target {
                    continue;
                }
                let id = match json.get("id").and_then(Json::as_str) {
                    Some(i) => i.to_string(),
                    None => continue,
                };
                let title = json
                    .get("title")
                    .and_then(Json::as_str)
                    .unwrap_or("(untitled)")
                    .to_string();
                let updated_ms = json
                    .get("time")
                    .and_then(|t| t.get("updated"))
                    .and_then(Json::as_number)
                    .map(|n| n as i64);
                sessions.push(SessionRef {
                    harness: "opencode".to_string(),
                    id,
                    cwd: directory.to_string(),
                    updated_ms,
                    title,
                    message_count: None,
                    delegated: false,
                });
            }
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        Ok(sessions)
    }

    fn resume_invocation(&self, id: &str, _cwd: &Path, _yolo: bool) -> Result<Invocation, String> {
        if id.is_empty() {
            return Ok(Invocation::new("opencode", vec!["--continue".to_string()]));
        }
        Ok(Invocation::new(
            "opencode",
            vec!["--session".to_string(), id.to_string()],
        ))
    }

    fn transcript(&self, id: &str, _cwd: &Path) -> Result<Vec<Turn>, String> {
        let home = home_dir().ok_or("cannot resolve HOME")?;
        let storage = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("storage");
        let msg_dir = storage.join("message").join(id);
        if !msg_dir.is_dir() {
            return Err(format!("opencode session {id} not found"));
        }

        // Collect (created, message_id, role), then order by creation time.
        let mut messages = Vec::new();
        for entry in fs::read_dir(&msg_dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json = match fs::read_to_string(&path)
                .ok()
                .and_then(|r| Json::parse(&r).ok())
            {
                Some(j) => j,
                None => continue,
            };
            let msg_id = match json.get("id").and_then(Json::as_str) {
                Some(i) => i.to_string(),
                None => continue,
            };
            let role = json
                .get("role")
                .and_then(Json::as_str)
                .unwrap_or("unknown")
                .to_string();
            let created = json
                .get("time")
                .and_then(|t| t.get("created"))
                .and_then(Json::as_number)
                .unwrap_or(0.0);
            messages.push((created as i64, msg_id, role));
        }
        messages.sort_by_key(|(created, _, _)| *created);

        let mut turns = Vec::new();
        for (_, msg_id, role) in messages {
            if let Some(text) = part_text(&storage.join("part").join(&msg_id)) {
                let text = text.trim();
                if !text.is_empty() {
                    turns.push(Turn {
                        role,
                        text: text.to_string(),
                    });
                }
            }
        }
        Ok(turns)
    }
}

/// Concatenate the `text` parts of one opencode message (`part/<messageID>/*.json`).
fn part_text(part_dir: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for entry in fs::read_dir(part_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let json = match fs::read_to_string(&path)
            .ok()
            .and_then(|r| Json::parse(&r).ok())
        {
            Some(j) => j,
            None => continue,
        };
        if json.get("type").and_then(Json::as_str) == Some("text") {
            if let Some(text) = json.get("text").and_then(Json::as_str) {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
