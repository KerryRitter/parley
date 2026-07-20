//! Panel fusion: ask several agents the same prompt in parallel, then have a
//! judge agent synthesize a single answer from their replies.
//!
//! Exposed two ways over the same engine:
//!   * `par fuse` — the CLI command (`run_cli`), for fusing from the terminal.
//!   * the MCP `fuse` tool (see `mcp.rs`), so an agent can convene its own panel.
//!
//! This is the multi-model deliberation idea behind OpenRouter's Fusion and
//! Sakana's AB-MCTS, built on `par`'s existing headless `ask::run`: every
//! panelist runs concurrently with its own auth and context, and a judge —
//! Claude by default — turns N replies into one decision. A diverse panel
//! covers each other's blind spots; consensus is high-confidence, disagreement
//! is a flag, and the union surfaces what any single model missed.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;

use crate::ask::{self, AskRequest, ContextRef};
use crate::cli::FuseOptions;
use crate::harness::normalize_harness;
use crate::route;
use crate::session;

/// Default panel when none is supplied: three different model families.
pub(crate) const DEFAULT_PANEL: &[&str] = &["claude", "codex", "gemini"];
/// Default judge — the model fusion synthesizes from.
pub(crate) const DEFAULT_JUDGE: &str = "claude";

/// One panelist's outcome: a display label and either its reply or a skip reason.
pub(crate) struct PanelReply {
    pub label: String,
    pub reply: Result<String, String>,
}

/// `par fuse` entry point: run the panel, then the judge, printing as it goes.
pub(crate) fn run_cli(options: FuseOptions) -> Result<(), String> {
    let prompt = options
        .prompt
        .ok_or("fuse requires a prompt: par fuse -p \"<task>\" --panel cl,co,g")?;

    let cwd = match options.cwd {
        Some(path) => PathBuf::from(path),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };
    let max_context = options
        .max_context_chars
        .unwrap_or(session::DEFAULT_CONTEXT_CHARS);

    // `--panel auto` (the sole entry) asks the router to pick a diverse panel.
    // `auto` listed alongside others (`--panel auto,solve`) is instead a
    // metaharness panelist, handled by the harness factory like any other.
    let panel_codes = if options.panel.len() == 1 && options.panel[0] == "auto" {
        route::pick_panel(&prompt, 3, route::resolve_bias(None), true)
    } else {
        options.panel.clone()
    };
    let panel = resolve_panel(&panel_codes)?;
    let judge = normalize_harness(&options.judge.unwrap_or_else(|| DEFAULT_JUDGE.to_string()));
    let context = options.context_from.as_deref().map(parse_context_spec);

    if options.dry_run {
        return dry_run(
            &prompt,
            &panel,
            &judge,
            &options.judge_model,
            &context,
            &cwd,
            max_context,
        );
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "⚖  fusing {} agents → judge: {}\n", panel.len(), judge).ok();
    out.flush().ok();

    let (answers, skipped) = split_replies(run_panel(
        &prompt,
        &panel,
        context,
        &cwd,
        max_context,
        options.yolo,
    ));
    for (label, text) in &answers {
        writeln!(out, "━━ {label} ━━\n{text}\n").ok();
    }
    for note in &skipped {
        writeln!(out, "━━ {note} (skipped) ━━\n").ok();
    }
    out.flush().ok();

    if answers.len() < 2 {
        return Err(insufficient_panel_message(answers.len(), &skipped));
    }

    writeln!(out, "━━ ⚖ fused answer · judge {judge} ━━").ok();
    out.flush().ok();
    let fused = run_judge(
        &prompt,
        &answers,
        &judge,
        options.judge_model,
        &cwd,
        max_context,
    )?;
    println!("{fused}");
    Ok(())
}

/// Resolve the panel list: explicit codes, else the default trio. Errors if < 2.
pub(crate) fn resolve_panel(panel: &[String]) -> Result<Vec<String>, String> {
    let panel: Vec<String> = if panel.is_empty() {
        DEFAULT_PANEL.iter().map(|s| s.to_string()).collect()
    } else {
        panel.to_vec()
    };
    if panel.len() < 2 {
        return Err("fuse needs at least 2 panelists (e.g. --panel cl,co,g)".to_string());
    }
    Ok(panel)
}

/// Split panel outcomes into successful `(label, reply)` answers and skip notes.
pub(crate) fn split_replies(replies: Vec<PanelReply>) -> (Vec<(String, String)>, Vec<String>) {
    let mut answers = Vec::new();
    let mut skipped = Vec::new();
    for r in replies {
        match r.reply {
            Ok(text) => answers.push((r.label, text)),
            Err(e) => skipped.push(format!("{} ({e})", r.label)),
        }
    }
    (answers, skipped)
}

pub(crate) fn insufficient_panel_message(got: usize, skipped: &[String]) -> String {
    let detail = if skipped.is_empty() {
        "none".to_string()
    } else {
        skipped.join(", ")
    };
    format!(
        "fusion needs at least 2 panelists to succeed; got {got}. Skipped: {detail}. Check the panel agents are installed."
    )
}

/// Run a panel concurrently: every agent answers `prompt` in its own thread,
/// optionally seeded with a shared session transcript. Input order is preserved.
pub(crate) fn run_panel(
    prompt: &str,
    panel: &[String],
    context: Option<ContextRef>,
    cwd: &Path,
    max_context: usize,
    yolo: bool,
) -> Vec<PanelReply> {
    let requests = build_requests(prompt, panel, &context, cwd, max_context, yolo);

    thread::scope(|scope| {
        let handles: Vec<_> = requests
            .iter()
            .map(|(label, req)| scope.spawn(move || (label.clone(), run_one(req))))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                let (label, reply) = h.join().unwrap_or_else(|_| {
                    ("?".to_string(), Err("panelist thread panicked".to_string()))
                });
                PanelReply { label, reply }
            })
            .collect()
    })
}

/// Run the judge agent over the panel's answers; returns its synthesized text.
pub(crate) fn run_judge(
    prompt: &str,
    answers: &[(String, String)],
    judge: &str,
    judge_model: Option<String>,
    cwd: &Path,
    max_context: usize,
) -> Result<String, String> {
    let req = AskRequest {
        harness: normalize_harness(judge),
        prompt: build_judge_prompt(prompt, answers),
        model: judge_model,
        provider: None,
        cwd: cwd.to_path_buf(),
        yolo: true,
        context: None,
        max_context_chars: max_context,
    };
    run_one(&req)
}

/// Run one agent headless, flattening a failure (non-zero exit, timeout, or a
/// clean exit with no output) into an error string so the panel can skip it.
fn run_one(req: &AskRequest) -> Result<String, String> {
    ask::run(req)?.reply()
}

/// Build one `AskRequest` per panelist, deduplicating labels when the same agent
/// appears twice (`claude (1)`, `claude (2)`). Self-pairing is a valid technique.
fn build_requests(
    prompt: &str,
    panel: &[String],
    context: &Option<ContextRef>,
    cwd: &Path,
    max_context: usize,
    yolo: bool,
) -> Vec<(String, AskRequest)> {
    let canon: Vec<String> = panel.iter().map(|h| normalize_harness(h)).collect();
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    canon
        .iter()
        .map(|name| {
            let total = canon.iter().filter(|n| *n == name).count();
            let label = if total > 1 {
                let n = seen.entry(name).or_insert(0);
                *n += 1;
                format!("{name} ({n})")
            } else {
                name.clone()
            };
            let req = AskRequest {
                harness: name.clone(),
                prompt: prompt.to_string(),
                model: None,
                provider: None,
                cwd: cwd.to_path_buf(),
                yolo,
                context: context.clone(),
                max_context_chars: max_context,
            };
            (label, req)
        })
        .collect()
}

/// The judge prompt — turns the panel's replies into one answer.
pub(crate) fn build_judge_prompt(prompt: &str, answers: &[(String, String)]) -> String {
    let mut p = String::new();
    p.push_str(&format!(
        "You are the judge of a multi-model panel. {} agents independently answered the QUESTION below.\n\n",
        answers.len()
    ));
    p.push_str("Produce, in this order:\n");
    p.push_str("1. CONSENSUS — claims most or all agents agree on (treat as high-confidence).\n");
    p.push_str("2. CONTRADICTIONS — where they disagree, and which side is right and why.\n");
    p.push_str("3. GAPS — important points only one agent raised.\n");
    p.push_str("4. BLIND SPOTS — anything important that NO agent addressed.\n");
    p.push_str("5. FINAL ANSWER — the single best answer. Do not average; pick the strongest reasoning.\n\n");
    p.push_str("=== QUESTION ===\n");
    p.push_str(prompt);
    p.push('\n');
    for (label, text) in answers {
        p.push_str(&format!("\n=== Reply from {label} ===\n{text}\n"));
    }
    p
}

/// Print every routed invocation (each panelist + the judge with placeholders).
fn dry_run(
    prompt: &str,
    panel: &[String],
    judge: &str,
    judge_model: &Option<String>,
    context: &Option<ContextRef>,
    cwd: &Path,
    max_context: usize,
) -> Result<(), String> {
    let requests = build_requests(prompt, panel, context, cwd, max_context, true);
    for (label, req) in &requests {
        println!(
            "# panelist {label} — dry run\n{}",
            ask::build(req)?.to_json()
        );
    }
    let placeholder: Vec<(String, String)> = requests
        .iter()
        .map(|(label, _)| (label.clone(), format!("<reply from {label}>")))
        .collect();
    let judge_req = AskRequest {
        harness: normalize_harness(judge),
        prompt: build_judge_prompt(prompt, &placeholder),
        model: judge_model.clone(),
        provider: None,
        cwd: cwd.to_path_buf(),
        yolo: true,
        context: None,
        max_context_chars: max_context,
    };
    println!(
        "# judge {judge} — dry run\n{}",
        ask::build(&judge_req)?.to_json()
    );
    Ok(())
}

/// Parse a `harness[:session]` context spec. Missing session means "latest".
fn parse_context_spec(spec: &str) -> ContextRef {
    match spec.split_once(':') {
        Some((harness, session)) => ContextRef {
            harness: harness.to_string(),
            session: session.to_string(),
        },
        None => ContextRef {
            harness: spec.to_string(),
            session: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/tmp")
    }

    #[test]
    fn resolve_panel_defaults_and_rejects_singletons() {
        assert_eq!(resolve_panel(&[]).unwrap(), DEFAULT_PANEL);
        assert!(resolve_panel(&["cl".to_string()]).is_err());
        assert_eq!(
            resolve_panel(&["cl".to_string(), "co".to_string()])
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn duplicate_panelists_get_numbered_labels() {
        let panel = vec!["cl".to_string(), "cl".to_string(), "co".to_string()];
        let reqs = build_requests("hi", &panel, &None, &cwd(), 12_000, true);
        assert_eq!(reqs[0].0, "claude (1)");
        assert_eq!(reqs[1].0, "claude (2)");
        assert_eq!(reqs[2].0, "codex");
        assert_eq!(reqs[0].1.harness, "claude");
        assert_eq!(reqs[0].1.prompt, "hi");
    }

    #[test]
    fn requests_carry_context_and_budget() {
        let ctx = Some(ContextRef {
            harness: "claude".into(),
            session: String::new(),
        });
        let panel = vec!["g".to_string(), "co".to_string()];
        let reqs = build_requests("design X", &panel, &ctx, &cwd(), 8_000, true);
        assert_eq!(reqs[0].1.harness, "gemini");
        assert_eq!(reqs[0].1.max_context_chars, 8_000);
        assert!(reqs[0].1.context.is_some());
    }

    #[test]
    fn split_replies_separates_success_and_skips() {
        let replies = vec![
            PanelReply {
                label: "claude".into(),
                reply: Ok("a".into()),
            },
            PanelReply {
                label: "codex".into(),
                reply: Err("not installed".into()),
            },
        ];
        let (answers, skipped) = split_replies(replies);
        assert_eq!(answers, vec![("claude".to_string(), "a".to_string())]);
        assert_eq!(skipped, vec!["codex (not installed)".to_string()]);
    }

    #[test]
    fn judge_prompt_has_structure_and_all_replies() {
        let answers = vec![
            ("claude".to_string(), "use a token bucket".to_string()),
            ("codex".to_string(), "use a leaky bucket".to_string()),
        ];
        let p = build_judge_prompt("rate limiter?", &answers);
        assert!(p.contains("CONSENSUS"));
        assert!(p.contains("CONTRADICTIONS"));
        assert!(p.contains("FINAL ANSWER"));
        assert!(p.contains("token bucket"));
        assert!(p.contains("leaky bucket"));
        assert!(p.contains("Reply from claude"));
    }

    #[test]
    fn context_spec_parses_harness_and_session() {
        let c = parse_context_spec("co:abc-123");
        assert_eq!(c.harness, "co");
        assert_eq!(c.session, "abc-123");
        assert_eq!(parse_context_spec("claude").session, "");
    }
}
