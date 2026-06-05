//! Cross-agent session discovery and resume.
//!
//! Each agent CLI writes its own session transcripts to disk in its own format
//! and location, and each has its own resume command. This module reads those
//! on-disk transcripts, scopes them to a working directory (the way every
//! harness's own `--resume` does), and maps a chosen session back to the
//! harness's native resume invocation.
//!
//! Two reliability tiers:
//! - **Native parsers** (claude, codex, opencode): exact cwd match with rich
//!   listing (id, title, mtime).
//! - **Delegate adapters** (cursor, gemini): the cwd→store mapping is a hash we
//!   do not reproduce, but the binaries self-scope to cwd via their own resume
//!   commands, so listing is best-effort and resume shells out to the native CLI.

mod claude;
mod codex;
mod cursor;
mod gemini;
mod opencode;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::cli::ResumeOptions;
use crate::harness::{normalize_harness, Invocation};
use crate::json::Json;
use crate::process::run_invocation;

/// A resumable session discovered on disk, normalized across harnesses.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SessionRef {
    pub harness: String,
    /// Value handed to the harness's native resume command.
    pub id: String,
    pub cwd: String,
    /// Epoch milliseconds of last activity (file mtime or a stored timestamp);
    /// used for newest-first ordering.
    pub updated_ms: Option<i64>,
    /// Summary, first user prompt, or native title — whatever the store offers.
    pub title: String,
    pub message_count: Option<usize>,
    /// True when listing is best-effort because the store is hash-scoped
    /// (cursor, gemini). Resume still works via the native CLI.
    pub delegated: bool,
}

/// One message in a session transcript, normalized across agents.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Turn {
    pub role: String,
    pub text: String,
}

pub(crate) trait SessionStore {
    fn harness(&self) -> &'static str;
    /// Sessions scoped to `cwd`, newest first. Returns an empty vec (not an
    /// error) when the store is absent.
    fn list(&self, cwd: &Path) -> Result<Vec<SessionRef>, String>;
    /// Build the native resume command for `id` in `cwd`. `id` may be an
    /// empty string for delegate stores that only resume the latest session.
    fn resume_invocation(&self, id: &str, cwd: &Path, yolo: bool) -> Result<Invocation, String>;
    /// Extract the conversation transcript (user/assistant turns) for a
    /// session, oldest first. Default: unsupported (delegate/hash-scoped stores).
    fn transcript(&self, _id: &str, _cwd: &Path) -> Result<Vec<Turn>, String> {
        Err(format!(
            "transcript export is not supported for {} sessions",
            self.harness()
        ))
    }
}

fn registry() -> Vec<Box<dyn SessionStore>> {
    vec![
        Box::new(claude::ClaudeSessions),
        Box::new(codex::CodexSessions),
        Box::new(opencode::OpencodeSessions),
        Box::new(cursor::CursorSessions),
        Box::new(gemini::GeminiSessions),
    ]
}

fn store_for(harness: &str) -> Option<Box<dyn SessionStore>> {
    let normalized = normalize_harness(harness);
    registry().into_iter().find(|s| s.harness() == normalized)
}

/// Merge sessions across stores (optionally filtered to one harness),
/// newest-first. Per-store errors are swallowed so one broken store does not
/// hide the rest.
fn list_all(cwd: &Path, harness_filter: Option<&str>) -> Vec<SessionRef> {
    let filter = harness_filter.map(normalize_harness);
    let mut sessions = Vec::new();
    for store in registry() {
        if let Some(f) = &filter {
            if store.harness() != f {
                continue;
            }
        }
        if let Ok(mut found) = store.list(cwd) {
            sessions.append(&mut found);
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
    sessions
}

pub(crate) fn run_resume(options: ResumeOptions) -> Result<(), String> {
    let cwd = match &options.cwd {
        Some(path) => PathBuf::from(path),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };

    let sessions = list_all(&cwd, options.harness.as_deref());

    if options.list {
        if options.json {
            print!("{}", sessions_to_json(&sessions, &cwd).to_pretty_string());
        } else {
            print!("{}", format_listing(&sessions, &cwd));
        }
        return Ok(());
    }

    // Resolve which session (or raw id) to resume.
    let invocation = if let Some(selector) = options.selector.as_deref() {
        resolve_selector(selector, &sessions, &options, &cwd)?
    } else if options.latest {
        match sessions.first() {
            Some(session) => invocation_for(session, &cwd, options.yolo)?,
            None => delegate_latest_or_err(&options, &cwd)?,
        }
    } else if sessions.is_empty() {
        // No scoped listing (e.g. a hash-scoped delegate store). Fall back to
        // the native CLI's own "resume latest" when one harness is targeted.
        delegate_latest_or_err(&options, &cwd)?
    } else {
        let session = pick_interactive(&sessions, &cwd)?;
        invocation_for(&session, &cwd, options.yolo)?
    };

    if options.print {
        println!("{}", render_command(&invocation));
        return Ok(());
    }

    run_invocation(invocation, cwd.to_str(), true)
}

/// A selector is either a 1-based index into the listing or a raw session id
/// (which then requires `--harness` to know who owns it).
fn resolve_selector(
    selector: &str,
    sessions: &[SessionRef],
    options: &ResumeOptions,
    cwd: &Path,
) -> Result<Invocation, String> {
    if let Ok(index) = selector.parse::<usize>() {
        if index >= 1 && index <= sessions.len() {
            return invocation_for(&sessions[index - 1], cwd, options.yolo);
        }
    }
    // Treat as a raw session id.
    match &options.harness {
        Some(harness) => {
            let store = store_for(harness)
                .ok_or_else(|| format!("no session store for harness \"{harness}\""))?;
            store.resume_invocation(selector, cwd, options.yolo)
        }
        None => Err(format!(
            "\"{selector}\" is not a list index; pass --harness <name> to resume it as a session id"
        )),
    }
}

/// When no scoped sessions were found, ask the targeted harness to resume its
/// latest session itself (delegate stores self-scope to cwd). Native stores
/// reject an empty id, surfacing a clear "nothing to resume" error.
fn delegate_latest_or_err(options: &ResumeOptions, cwd: &Path) -> Result<Invocation, String> {
    match &options.harness {
        Some(harness) => {
            let store = store_for(harness)
                .ok_or_else(|| format!("no session store for harness \"{harness}\""))?;
            store.resume_invocation("", cwd, options.yolo)
        }
        None => Err(no_sessions_message(options, cwd)),
    }
}

fn invocation_for(session: &SessionRef, cwd: &Path, yolo: bool) -> Result<Invocation, String> {
    let store = store_for(&session.harness)
        .ok_or_else(|| format!("no session store for harness \"{}\"", session.harness))?;
    store.resume_invocation(&session.id, cwd, yolo)
}

fn pick_interactive(sessions: &[SessionRef], cwd: &Path) -> Result<SessionRef, String> {
    if sessions.is_empty() {
        return Err(format!("no resumable sessions found for {}", cwd.display()));
    }
    print!("{}", format_listing(sessions, cwd));
    print!(
        "\nResume which session? [1-{}] (q to cancel): ",
        sessions.len()
    );
    io::stdout().flush().ok();

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("failed to read selection: {e}"))?;
    let choice = line.trim();
    if choice.is_empty() || choice.eq_ignore_ascii_case("q") {
        return Err("cancelled".to_string());
    }
    let index: usize = choice
        .parse()
        .map_err(|_| format!("not a number: {choice}"))?;
    if index < 1 || index > sessions.len() {
        return Err(format!("out of range: {index}"));
    }
    Ok(sessions[index - 1].clone())
}

fn no_sessions_message(options: &ResumeOptions, cwd: &Path) -> String {
    match &options.harness {
        Some(h) => format!("no {h} sessions found for {}", cwd.display()),
        None => format!("no resumable sessions found for {}", cwd.display()),
    }
}

// ---- Rendering -------------------------------------------------------------

fn format_listing(sessions: &[SessionRef], cwd: &Path) -> String {
    if sessions.is_empty() {
        return format!("No resumable sessions for {}\n", cwd.display());
    }
    let mut out = format!("Resumable sessions for {}:\n", cwd.display());
    for (i, s) in sessions.iter().enumerate() {
        let title = truncate(&s.title, 60);
        let flag = if s.delegated { " ~" } else { "" };
        out.push_str(&format!(
            "  [{:>2}] {:<9}{}  {}\n",
            i + 1,
            s.harness,
            flag,
            title
        ));
    }
    out
}

/// Truncate to `max` chars on a char boundary, collapsing whitespace/newlines.
fn truncate(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut s: String = flat.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// Render an invocation as a copy-pasteable shell command.
pub(crate) fn render_command(inv: &Invocation) -> String {
    let mut parts = vec![inv.command.clone()];
    for arg in &inv.args {
        parts.push(shell_quote(arg));
    }
    parts.join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || "\"'\\$`*?[]{}();&|<>".contains(c))
    {
        format!("'{}'", arg.replace('\'', "'\\''"))
    } else {
        arg.to_string()
    }
}

// ---- JSON helpers (shared with the MCP server) -----------------------------

/// Build a JSON object for one session, including its resume command string.
pub(crate) fn session_to_json(session: &SessionRef, cwd: &Path) -> Json {
    use std::collections::BTreeMap;
    let mut map = BTreeMap::new();
    map.insert("harness".to_string(), Json::Str(session.harness.clone()));
    map.insert("id".to_string(), Json::Str(session.id.clone()));
    map.insert("cwd".to_string(), Json::Str(session.cwd.clone()));
    map.insert("title".to_string(), Json::Str(session.title.clone()));
    map.insert("delegated".to_string(), Json::Bool(session.delegated));
    if let Some(ms) = session.updated_ms {
        map.insert("updated_ms".to_string(), Json::Number(ms as f64));
    }
    if let Some(count) = session.message_count {
        map.insert("message_count".to_string(), Json::Number(count as f64));
    }
    if let Ok(inv) = invocation_for(session, cwd, false) {
        map.insert(
            "resume_command".to_string(),
            Json::Str(render_command(&inv)),
        );
    }
    Json::Object(map)
}

fn sessions_to_json(sessions: &[SessionRef], cwd: &Path) -> Json {
    Json::Array(sessions.iter().map(|s| session_to_json(s, cwd)).collect())
}

// ---- Entry points used by the MCP server -----------------------------------

pub(crate) fn list_sessions_json(cwd: &Path, harness_filter: Option<&str>) -> Json {
    let sessions = list_all(cwd, harness_filter);
    sessions_to_json(&sessions, cwd)
}

pub(crate) fn last_session_json(cwd: &Path, harness_filter: Option<&str>) -> Option<Json> {
    let sessions = list_all(cwd, harness_filter);
    sessions.first().map(|s| session_to_json(s, cwd))
}

pub(crate) fn resume_command_string(
    harness: &str,
    id: &str,
    cwd: &Path,
    yolo: bool,
) -> Result<String, String> {
    let store =
        store_for(harness).ok_or_else(|| format!("no session store for harness \"{harness}\""))?;
    let inv = store.resume_invocation(id, cwd, yolo)?;
    Ok(render_command(&inv))
}

/// Default ceiling on how much transcript text to inject as context.
pub(crate) const DEFAULT_CONTEXT_CHARS: usize = 12_000;

/// Build a context preamble from a prior session's transcript, for handing one
/// agent's conversation to another. `selector` is a session id, or empty /
/// "latest" for the newest session in `cwd`. Keeps the most recent turns within
/// `max_chars`.
pub(crate) fn transcript_context(
    harness: &str,
    selector: &str,
    cwd: &Path,
    max_chars: usize,
) -> Result<String, String> {
    let store =
        store_for(harness).ok_or_else(|| format!("no session store for harness \"{harness}\""))?;

    let id = if selector.is_empty() || selector.eq_ignore_ascii_case("latest") {
        store
            .list(cwd)?
            .into_iter()
            .next()
            .map(|s| s.id)
            .ok_or_else(|| format!("no {harness} sessions found for {}", cwd.display()))?
    } else {
        selector.to_string()
    };

    let turns = store.transcript(&id, cwd)?;
    Ok(format_context(harness, &id, &turns, max_chars))
}

fn format_context(harness: &str, id: &str, turns: &[Turn], max_chars: usize) -> String {
    let mut body = String::new();
    for turn in turns {
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        body.push_str(&format!("[{}]\n{}\n\n", turn.role, text));
    }

    // Keep the tail (most recent turns) when over budget.
    let mut truncated = false;
    if body.chars().count() > max_chars {
        let start = body.chars().count() - max_chars;
        body = body.chars().skip(start).collect();
        truncated = true;
    }

    let header = format!("=== Context from {harness} session {id} ===");
    let note = if truncated {
        "\n[...earlier turns omitted to fit the context budget...]\n\n"
    } else {
        "\n"
    };
    format!("{header}{note}{}", body.trim_end())
}

// ---- Shared filesystem helpers (used by adapter submodules) ----------------

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

/// File mtime as epoch milliseconds.
pub(crate) fn file_mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

/// Canonicalize a path for cwd comparison, falling back to the raw path when
/// the target does not exist.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_collapses_and_caps() {
        assert_eq!(truncate("hello   world", 40), "hello world");
        let long = "a".repeat(100);
        let t = truncate(&long, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn shell_quote_wraps_spaces() {
        assert_eq!(shell_quote("plain"), "plain");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn render_command_quotes_args() {
        let inv = Invocation::new("claude", vec!["--resume".into(), "abc 123".into()]);
        assert_eq!(render_command(&inv), "claude --resume 'abc 123'");
    }

    #[test]
    fn list_all_sorts_newest_first() {
        let mut a = SessionRef {
            harness: "claude".into(),
            id: "a".into(),
            cwd: "/x".into(),
            updated_ms: Some(100),
            title: "older".into(),
            message_count: None,
            delegated: false,
        };
        let b = SessionRef {
            updated_ms: Some(200),
            title: "newer".into(),
            ..a.clone()
        };
        let mut v = [a.clone(), b.clone()];
        v.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        assert_eq!(v[0].title, "newer");
        a.updated_ms = None;
        // None sorts last under newest-first.
        let mut v2 = [a, b];
        v2.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        assert_eq!(v2[0].title, "newer");
    }
}
