//! Outcome signals — cheap, pure heuristics for "did this agent actually make
//! progress, or is it stuck?". These are the portable distillation of the
//! workweave/router loop- and spiral-detection work, adapted to what `par` can
//! observe: a captured reply (text + exit status) and the working tree.
//!
//! The router watches a re-sent transcript turn by turn; `par` drives an agent
//! CLI once per invocation, so its unit of observation is coarser — a whole
//! headless run. The signals below therefore key off the *captured reply* and
//! *filesystem progress* rather than per-tool-call telemetry, and the loop
//! detectors operate over a sequence of replies (e.g. `par converse` turns).
//!
//! Everything here is a pure function except [`git_dirty`], which shells out to
//! `git` best-effort; all of it is unit-tested.

use std::path::Path;
use std::process::{Command, Stdio};

/// Replies at or below this length (after trimming) are treated as degenerate —
/// an agent that returned essentially nothing. Mirrors the router's "<10 output
/// tokens, no tool call" degenerate-response rule, in characters.
pub(crate) const DEGENERATE_MAX_CHARS: usize = 24;

/// High-precision markers that a tool/command/test *failed*, even when the
/// agent's wrapper exited 0. Scanned over a bounded prefix of the reply.
const ERROR_MARKERS: &[&str] = &[
    "Traceback (most recent call last)",
    "panic:",
    "fatal:",
    "FAILED",
    "Error:",
    "error:",
    "Exception:",
    "command not found",
    "No such file or directory",
    "cannot find",
    "compilation failed",
    "test failed",
    "tests failed",
];

/// Only the first N bytes of a reply are scanned for error markers — the router
/// scans the first 2KB of each tool result for the same reason (speed + the
/// signal is almost always near the top).
const ERROR_SCAN_BYTES: usize = 2048;

/// Why a reply was judged a failure, for telemetry and escalation reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Non-zero exit / no usable output.
    Errored,
    /// The watchdog killed the agent for exceeding a time budget.
    TimedOut,
    /// Exited cleanly but produced essentially nothing.
    Degenerate,
    /// Output carries an error/failure marker (red tests, traceback, ...).
    ErrorMarker,
    /// Claimed to work but left the working tree untouched.
    NoProgress,
}

impl Failure {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Failure::Errored => "the agent exited with a failure status",
            Failure::TimedOut => "the agent timed out (exceeded its time budget)",
            Failure::Degenerate => "the agent returned essentially nothing",
            Failure::ErrorMarker => "the agent's output contains an error/failure marker",
            Failure::NoProgress => "the agent changed no files despite the task",
        }
    }
}

/// An empty or near-empty reply.
pub(crate) fn is_degenerate(text: &str) -> bool {
    text.trim().chars().count() <= DEGENERATE_MAX_CHARS
}

/// Does the reply carry a failure marker in its leading bytes?
pub(crate) fn has_error_marker(text: &str) -> bool {
    let head = head_bytes(text, ERROR_SCAN_BYTES);
    ERROR_MARKERS.iter().any(|m| head.contains(m))
}

fn head_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    // Snap to a char boundary so slicing never panics on multibyte input.
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// FNV-1a 64-bit hash — a tiny, dependency-free, deterministic digest used for
/// reply/signature fingerprints. Not cryptographic; we only need collision
/// resistance good enough to tell "same text" from "different text".
pub(crate) fn fnv1a_64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A short hex fingerprint of arbitrary text (for telemetry: log the hash, not
/// the prompt).
pub(crate) fn fingerprint(s: &str) -> String {
    format!("{:016x}", fnv1a_64(s.trim()))
}

/// The most times any single signature repeats in a window (the tight-loop
/// bar, and the basis for [`replies_looping`]).
fn max_repeat(signatures: &[String]) -> usize {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut max = 0;
    for s in signatures {
        let c = counts.entry(s.as_str()).or_insert(0);
        *c += 1;
        max = max.max(*c);
    }
    max
}

/// Across a sequence of full replies (e.g. `par converse` turns), are the last
/// `window` replies collapsing onto the same answer? Used to stop a two-agent
/// loop that has stopped progressing.
pub(crate) fn replies_looping(replies: &[String], window: usize) -> bool {
    if replies.len() < window || window < 2 {
        return false;
    }
    let sigs: Vec<String> = tail(replies, window)
        .iter()
        .map(|r| fingerprint(r))
        .collect();
    // Every reply in the window identical to at least one other => stalled.
    max_repeat(&sigs) >= window
}

fn tail<T>(items: &[T], n: usize) -> &[T] {
    let start = items.len().saturating_sub(n);
    &items[start..]
}

/// List of files git considers dirty in `cwd` (porcelain paths), best-effort.
/// `None` when `cwd` is not a git work tree or git is unavailable — callers
/// treat that as "can't measure progress", never as "no progress".
pub(crate) fn git_dirty(cwd: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .map(|l| l.get(3..).unwrap_or(l).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// Did the working tree change between two `git_dirty` snapshots? `None` (can't
/// measure) is reported as "changed" so we never falsely flag no-progress.
pub(crate) fn workspace_changed(before: &Option<Vec<String>>, after: &Option<Vec<String>>) -> bool {
    match (before, after) {
        (Some(b), Some(a)) => b != a,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_catches_empty_and_tiny() {
        assert!(is_degenerate(""));
        assert!(is_degenerate("   \n  "));
        assert!(is_degenerate("ok"));
        assert!(!is_degenerate(
            "Here is a complete and substantive answer to your question."
        ));
    }

    #[test]
    fn error_markers_are_detected_near_the_top() {
        assert!(has_error_marker(
            "Traceback (most recent call last):\n  ..."
        ));
        assert!(has_error_marker("running tests...\n3 passed, 1 FAILED"));
        assert!(has_error_marker("error: cannot borrow `x` as mutable"));
        assert!(!has_error_marker(
            "All good. The design uses a token bucket."
        ));
    }

    #[test]
    fn error_marker_scan_is_bounded_and_utf8_safe() {
        // Marker pushed past the scan window should not be found.
        let mut s = "x".repeat(ERROR_SCAN_BYTES);
        s.push_str(" FAILED");
        assert!(!has_error_marker(&s));
        // Multibyte input must not panic when sliced at the boundary.
        let emoji = "🚀".repeat(ERROR_SCAN_BYTES);
        let _ = has_error_marker(&emoji);
    }

    #[test]
    fn fnv_is_deterministic_and_distinguishing() {
        assert_eq!(fnv1a_64("abc"), fnv1a_64("abc"));
        assert_ne!(fnv1a_64("abc"), fnv1a_64("abd"));
        assert_eq!(fingerprint(" hi "), fingerprint("hi"));
    }

    #[test]
    fn replies_looping_detects_collapsed_dialogue() {
        let looped: Vec<String> = vec![
            "let's agree".to_string(),
            "I agree".to_string(),
            "I agree".to_string(),
            "I agree".to_string(),
        ];
        assert!(replies_looping(&looped, 3));
        let progressing: Vec<String> = vec![
            "point one".to_string(),
            "point two".to_string(),
            "point three".to_string(),
        ];
        assert!(!replies_looping(&progressing, 3));
    }

    #[test]
    fn workspace_change_treats_unmeasurable_as_changed() {
        assert!(workspace_changed(&None, &Some(vec![])));
        assert!(!workspace_changed(
            &Some(vec!["a".to_string()]),
            &Some(vec!["a".to_string()])
        ));
        assert!(workspace_changed(
            &Some(vec![]),
            &Some(vec!["new.rs".to_string()])
        ));
    }
}
