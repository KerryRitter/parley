//! Cursor chats live under `~/.cursor/chats/<workspace-hash>/<chat-uuid>/`. The
//! cwd→hash mapping is not a plain md5/sha256 of the path, so we don't reproduce
//! it here. `cursor-agent` self-scopes to the current directory, so listing is
//! skipped and resume delegates to the native CLI.

use std::path::Path;

use super::{SessionRef, SessionStore};
use crate::harness::Invocation;

pub(crate) struct CursorSessions;

impl SessionStore for CursorSessions {
    fn harness(&self) -> &'static str {
        "cursor"
    }

    fn list(&self, _cwd: &Path) -> Result<Vec<SessionRef>, String> {
        // Hash-scoped store; cannot reliably map cwd → chats dir.
        Ok(Vec::new())
    }

    fn resume_invocation(&self, id: &str, _cwd: &Path, yolo: bool) -> Result<Invocation, String> {
        let mut args = if id.is_empty() {
            // `cursor-agent resume` resumes the latest chat for the cwd.
            vec!["resume".to_string()]
        } else {
            vec!["--resume".to_string(), id.to_string()]
        };
        if yolo {
            args.push("--force".to_string());
        }
        Ok(Invocation::new("cursor-agent", args))
    }
}
