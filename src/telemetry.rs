//! Lightweight, local, append-only telemetry — the zero-dependency distillation
//! of workweave/router's request-telemetry + feedback loop.
//!
//! The router logs one wide row per turn to Postgres (decision, every
//! candidate's score, the outcome) so a later offline pass can learn a better
//! policy. `par` has no server and no DB, so it does the same thing the only way
//! a CLI should: one JSON object per line appended to a file under
//! `~/.config/par/`, written fire-and-forget after the run so a failed write can
//! never break the command.
//!
//! Privacy: the prompt text is **never** stored by default — only its length and
//! a non-reversible fingerprint. Set `PARLEY_TELEMETRY_PROMPTS=1` to opt into
//! storing raw prompts locally. Disable telemetry entirely with
//! `PARLEY_TELEMETRY=off`.
//!
//! What it powers: `par stats` (a per-`(task, agent)` scoreboard) and the
//! routing table's future ability to learn from real outcomes — `par fuse`'s
//! judge already picks a winner per prompt, which is a free quality label.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::Json;
use crate::signals;

/// One telemetry event. Build it, then call [`Event::record`] — recording is
/// best-effort and silent.
#[derive(Default)]
pub(crate) struct Event {
    pub cmd: &'static str,
    pub harness: Option<String>,
    pub panel: Vec<String>,
    pub judge: Option<String>,
    pub task_class: Option<String>,
    /// Source prompt — used only for length + fingerprint unless prompt capture
    /// is explicitly enabled.
    pub prompt: Option<String>,
    pub ok: bool,
    pub duration_ms: u128,
    pub out_len: usize,
    pub degenerate: bool,
    pub errored: bool,
    pub timed_out: bool,
    pub looped: bool,
    pub escalated: bool,
    pub reason: Option<String>,
}

impl Event {
    /// Append this event as one JSONL row. Generates a `run_id`, records it as
    /// the "last run" so `par rate` can attach feedback, and returns it. All
    /// I/O errors are swallowed — telemetry must never fail a command.
    pub(crate) fn record(&self) -> Option<String> {
        if !enabled() {
            return None;
        }
        let run_id = new_run_id();
        let json = self.to_json(&run_id);
        let _ = append_line(&json.to_compact_string());
        let _ = write_last_run(&run_id, self.cmd);
        Some(run_id)
    }

    fn to_json(&self, run_id: &str) -> Json {
        let mut map: BTreeMap<String, Json> = BTreeMap::new();
        map.insert("ts".into(), Json::Number(unix_secs() as f64));
        map.insert("run_id".into(), Json::Str(run_id.to_string()));
        map.insert("cmd".into(), Json::Str(self.cmd.to_string()));
        if let Some(h) = &self.harness {
            map.insert("harness".into(), Json::Str(h.clone()));
        }
        if !self.panel.is_empty() {
            map.insert(
                "panel".into(),
                Json::Array(self.panel.iter().map(|p| Json::Str(p.clone())).collect()),
            );
        }
        if let Some(j) = &self.judge {
            map.insert("judge".into(), Json::Str(j.clone()));
        }
        if let Some(tc) = &self.task_class {
            map.insert("task_class".into(), Json::Str(tc.clone()));
        }
        if let Some(prompt) = &self.prompt {
            map.insert(
                "prompt_len".into(),
                Json::Number(prompt.chars().count() as f64),
            );
            map.insert(
                "prompt_hash".into(),
                Json::Str(signals::fingerprint(prompt)),
            );
            if include_prompts() {
                map.insert("prompt".into(), Json::Str(prompt.clone()));
            }
        }
        map.insert("ok".into(), Json::Bool(self.ok));
        map.insert("duration_ms".into(), Json::Number(self.duration_ms as f64));
        map.insert("out_len".into(), Json::Number(self.out_len as f64));
        // Only emit the failure flags that fired, to keep rows small.
        for (key, set) in [
            ("degenerate", self.degenerate),
            ("errored", self.errored),
            ("timed_out", self.timed_out),
            ("looped", self.looped),
            ("escalated", self.escalated),
        ] {
            if set {
                map.insert(key.into(), Json::Bool(true));
            }
        }
        if let Some(r) = &self.reason {
            map.insert("reason".into(), Json::Str(r.clone()));
        }
        Json::Object(map)
    }
}

/// Attach a thumbs-up/down (and optional note) to the most recent run.
pub(crate) fn run_rate(sign: bool, note: Option<String>) -> Result<(), String> {
    let (run_id, cmd) = read_last_run()
        .ok_or("no recent par run to rate (run something first, then `par rate +`)")?;
    let mut map: BTreeMap<String, Json> = BTreeMap::new();
    map.insert("ts".into(), Json::Number(unix_secs() as f64));
    map.insert("cmd".into(), Json::Str("rate".into()));
    map.insert("rates_run".into(), Json::Str(run_id.clone()));
    map.insert("rates_cmd".into(), Json::Str(cmd));
    map.insert(
        "rating".into(),
        Json::Str(if sign { "+".into() } else { "-".into() }),
    );
    if let Some(note) = note {
        map.insert("note".into(), Json::Str(note));
    }
    append_line(&Json::Object(map).to_compact_string())?;
    println!(
        "Recorded {} feedback for run {run_id}.",
        if sign { "👍" } else { "👎" }
    );
    Ok(())
}

/// `par stats` — read the JSONL log and print a scoreboard.
pub(crate) fn run_stats(json_out: bool) -> Result<(), String> {
    let path = telemetry_path()?;
    let text = fs::read_to_string(&path).unwrap_or_default();
    let rows: Vec<Json> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| Json::parse(l).ok())
        .collect();

    if rows.is_empty() {
        println!("No telemetry yet at {}.", path.display());
        println!("(Runs are logged automatically; disable with PARLEY_TELEMETRY=off.)");
        return Ok(());
    }

    // Aggregate run rows (skip feedback rows) by (task_class, harness).
    let mut board: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut totals = Tally::default();
    let mut ratings_up = 0usize;
    let mut ratings_down = 0usize;

    for row in &rows {
        let cmd = row.get("cmd").and_then(Json::as_str).unwrap_or("");
        if cmd == "rate" {
            match row.get("rating").and_then(Json::as_str) {
                Some("+") => ratings_up += 1,
                Some("-") => ratings_down += 1,
                _ => {}
            }
            continue;
        }
        let ok = row.get("ok").and_then(Json::as_bool).unwrap_or(false);
        let ms = row
            .get("duration_ms")
            .and_then(Json::as_number)
            .unwrap_or(0.0);
        let task = row
            .get("task_class")
            .and_then(Json::as_str)
            .unwrap_or("-")
            .to_string();
        let agent = row
            .get("harness")
            .and_then(Json::as_str)
            .map(str::to_string)
            .or_else(|| {
                row.get("panel")
                    .and_then(Json::as_array)
                    .map(|_| "panel".to_string())
            })
            .unwrap_or_else(|| cmd.to_string());
        let tally = board.entry((task, agent)).or_default();
        tally.add(ok, ms);
        totals.add(ok, ms);
    }

    if json_out {
        let mut entries: Vec<Json> = Vec::new();
        for ((task, agent), t) in &board {
            let mut m: BTreeMap<String, Json> = BTreeMap::new();
            m.insert("task_class".into(), Json::Str(task.clone()));
            m.insert("agent".into(), Json::Str(agent.clone()));
            m.insert("runs".into(), Json::Number(t.runs as f64));
            m.insert("ok".into(), Json::Number(t.ok as f64));
            m.insert("success_rate".into(), Json::Number(t.rate()));
            m.insert("avg_ms".into(), Json::Number(t.avg_ms()));
            entries.push(Json::Object(m));
        }
        println!("{}", Json::Array(entries).to_pretty_string());
        return Ok(());
    }

    println!(
        "Parley stats — {} runs, {:.0}% ok, {} 👍 / {} 👎",
        totals.runs,
        totals.rate() * 100.0,
        ratings_up,
        ratings_down
    );
    println!("  source: {}\n", path.display());
    println!(
        "  {:<14} {:<10} {:>5} {:>6} {:>9}",
        "TASK", "AGENT", "RUNS", "OK%", "AVG"
    );
    let mut ranked: Vec<_> = board.into_iter().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1.runs));
    for ((task, agent), t) in ranked {
        println!(
            "  {:<14} {:<10} {:>5} {:>5.0}% {:>8}",
            truncate(&task, 14),
            truncate(&agent, 10),
            t.runs,
            t.rate() * 100.0,
            human_ms(t.avg_ms()),
        );
    }
    Ok(())
}

#[derive(Default)]
struct Tally {
    runs: usize,
    ok: usize,
    total_ms: f64,
}

impl Tally {
    fn add(&mut self, ok: bool, ms: f64) {
        self.runs += 1;
        if ok {
            self.ok += 1;
        }
        self.total_ms += ms;
    }
    fn rate(&self) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            self.ok as f64 / self.runs as f64
        }
    }
    fn avg_ms(&self) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            self.total_ms / self.runs as f64
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn human_ms(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.1}s", ms / 1000.0)
    } else {
        format!("{ms:.0}ms")
    }
}

// ---- enablement + paths ----------------------------------------------------

pub(crate) fn enabled() -> bool {
    match env::var("PARLEY_TELEMETRY") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        ),
        Err(_) => true,
    }
}

fn include_prompts() -> bool {
    matches!(
        env::var("PARLEY_TELEMETRY_PROMPTS")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

fn base_dir() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("PAR_TELEMETRY_FILE") {
        // An explicit file path; its parent is the base.
        let p = PathBuf::from(path);
        return Ok(p.parent().map(PathBuf::from).unwrap_or(p));
    }
    if let Ok(home) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(home).join("par"));
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set; cannot find config dir")?;
    Ok(PathBuf::from(home).join(".config").join("par"))
}

fn telemetry_path() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("PAR_TELEMETRY_FILE") {
        return Ok(PathBuf::from(path));
    }
    Ok(base_dir()?.join("telemetry.jsonl"))
}

fn last_run_path() -> Result<PathBuf, String> {
    Ok(base_dir()?.join("last_run"))
}

fn append_line(line: &str) -> Result<(), String> {
    let path = telemetry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("write {}: {e}", path.display()))
}

fn write_last_run(run_id: &str, cmd: &str) -> Result<(), String> {
    let path = last_run_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(&path, format!("{run_id}\t{cmd}\n"))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn read_last_run() -> Option<(String, String)> {
    let text = fs::read_to_string(last_run_path().ok()?).ok()?;
    let line = text.lines().next()?;
    let (run_id, cmd) = line.split_once('\t')?;
    Some((run_id.to_string(), cmd.to_string()))
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_run_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{:x}-{:x}", millis, std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_json_omits_prompt_text_by_default() {
        let ev = Event {
            cmd: "run",
            harness: Some("claude".into()),
            task_class: Some("code".into()),
            prompt: Some("write a parser".into()),
            ok: true,
            duration_ms: 1200,
            out_len: 500,
            ..Event::default()
        };
        let json = ev.to_json("test-run");
        assert_eq!(json.get("harness").and_then(Json::as_str), Some("claude"));
        assert_eq!(json.get("task_class").and_then(Json::as_str), Some("code"));
        assert!(json.get("prompt_hash").is_some());
        assert!(json.get("prompt_len").is_some());
        // Raw prompt must not be present unless explicitly opted in.
        assert!(json.get("prompt").is_none());
        assert_eq!(json.get("ok").and_then(Json::as_bool), Some(true));
    }

    #[test]
    fn failure_flags_only_emitted_when_set() {
        let ev = Event {
            cmd: "solve",
            degenerate: true,
            escalated: true,
            ..Event::default()
        };
        let json = ev.to_json("r");
        assert_eq!(json.get("degenerate").and_then(Json::as_bool), Some(true));
        assert_eq!(json.get("escalated").and_then(Json::as_bool), Some(true));
        assert!(json.get("errored").is_none());
        assert!(json.get("looped").is_none());
    }

    #[test]
    fn enabled_respects_env() {
        // Default (unset) is on; this test only checks the parse of explicit
        // values via a temporary override.
        std::env::set_var("PARLEY_TELEMETRY", "off");
        assert!(!enabled());
        std::env::set_var("PARLEY_TELEMETRY", "1");
        assert!(enabled());
        std::env::remove_var("PARLEY_TELEMETRY");
    }
}
