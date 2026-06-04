//! Gemini sessions live under `~/.gemini/tmp/<projectHash>/chats/session-*.json`.
//! The `projectHash` is not a plain hash of the cwd we can reproduce, and Gemini
//! resumes by index (not id) scoped to the current project, so listing is skipped
//! and resume delegates to the native CLI (`--resume <index|latest>`).

use std::path::Path;

use super::{SessionRef, SessionStore};
use crate::harness::Invocation;

pub(crate) struct GeminiSessions;

impl SessionStore for GeminiSessions {
    fn harness(&self) -> &'static str {
        "gemini"
    }

    fn list(&self, _cwd: &Path) -> Result<Vec<SessionRef>, String> {
        // Hash-scoped, index-addressed store; cannot reliably enumerate by cwd.
        Ok(Vec::new())
    }

    fn resume_invocation(&self, id: &str, _cwd: &Path, yolo: bool) -> Result<Invocation, String> {
        // `id` is a session index or "latest"; default to the most recent.
        let target = if id.is_empty() { "latest" } else { id };
        let mut args = vec!["--resume".to_string(), target.to_string()];
        if yolo {
            args.push("--yolo".to_string());
        }
        Ok(Invocation::new("gemini", args))
    }
}
