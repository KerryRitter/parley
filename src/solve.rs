//! `par solve` — route a prompt to one agent, then **auto-escalate to a panel**
//! if that agent gets stuck. This is the Parley embodiment of the router's
//! loop-escalation idea, and a strictly more powerful version of it: the router
//! can only swap to a stronger *model*; `par` swaps *strategy* — a single agent
//! that fails escalates to a `fuse` panel (several agents + a judge), seeded
//! with the failed attempt so the panel knows what already didn't work.
//!
//! Detection uses [`crate::signals`]: a degenerate (near-empty) reply, an
//! error/failure marker in the output, a non-zero exit, a watchdog timeout, or —
//! for code-shaped tasks — the agent leaving the working tree untouched (the
//! "no edit = no progress" oracle, measured from `git`).
//!
//! `--shadow` runs the detection but takes no action (prints "would have
//! escalated"), mirroring the router's shadow-mode-first rollout discipline:
//! measure precision on real runs before arming the auto-escalation.

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use crate::ask::{self, AskRequest};
use crate::cli::SolveOptions;
use crate::fuse;
use crate::harness::normalize_harness;
use crate::route::{self, TaskClass};
use crate::session;
use crate::signals::{self, Failure};
use crate::telemetry::Event;

pub(crate) fn run_cli(options: SolveOptions) -> Result<(), String> {
    let prompt = options
        .prompt
        .clone()
        .ok_or("solve requires a prompt: par solve \"<task>\"")?;

    let cwd = match &options.cwd {
        Some(path) => PathBuf::from(path),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };
    let max_context = session::DEFAULT_CONTEXT_CHARS;
    let bias = route::resolve_bias(options.bias);

    // 1. Pick the first agent (explicit -h, else auto-route).
    let selector = options
        .harness
        .clone()
        .unwrap_or_else(|| "auto".to_string());
    let (harness, route_reason) = route::resolve_harness(&selector, &prompt, options.bias);
    let class = route::classify(&prompt);

    if options.dry_run {
        return dry_run(&prompt, &harness, &options, &cwd, max_context, bias, class);
    }

    if let Some(reason) = &route_reason {
        eprintln!("⚖  solve: routing to {harness} — {reason}");
    } else {
        eprintln!("⚖  solve: running {harness}");
    }

    // 2. Run the first agent, watching the working tree for progress.
    let before = signals::git_dirty(&cwd);
    let started = Instant::now();
    let req = AskRequest {
        harness: harness.clone(),
        prompt: prompt.clone(),
        model: None,
        provider: None,
        cwd: cwd.clone(),
        yolo: options.yolo,
        context: None,
        max_context_chars: max_context,
    };
    let out = ask::run(&req)?;
    let elapsed = started.elapsed().as_millis();
    let after = signals::git_dirty(&cwd);

    let reply = out.stdout.trim().to_string();
    let expect_changes = options
        .expect_changes
        .unwrap_or_else(|| is_code_task(class));
    let failure = classify_failure(&out, &reply, expect_changes, &before, &after);

    // 3. No failure → done. Print the reply, log success.
    let Some(failure) = failure else {
        println!("{reply}");
        Event {
            cmd: "solve",
            harness: Some(harness.clone()),
            task_class: Some(class.name().to_string()),
            prompt: Some(prompt.clone()),
            ok: true,
            duration_ms: elapsed,
            out_len: reply.len(),
            reason: route_reason,
            ..Event::default()
        }
        .record();
        return Ok(());
    };

    // 4a. Shadow mode → report what would happen, change nothing.
    if options.shadow {
        if !reply.is_empty() {
            println!("{reply}");
        }
        eprintln!(
            "⚖  solve [shadow]: would escalate to a panel — {} (set without --shadow to act)",
            failure.reason()
        );
        record_failure(&harness, class, &prompt, elapsed, &reply, failure, false);
        return Ok(());
    }

    // 4b. Escalate: convene a panel seeded with the failed attempt.
    eprintln!(
        "⚖  solve: {harness} fell short — {}. Escalating to a panel.",
        failure.reason()
    );
    let panel = escalation_panel(&options.panel, &prompt, &harness, bias);
    let judge = normalize_harness(
        &options
            .judge
            .clone()
            .unwrap_or_else(|| fuse::DEFAULT_JUDGE.to_string()),
    );
    let panel_prompt = seed_prompt(&prompt, &harness, failure, &reply);

    eprintln!("⚖  panel: {} → judge: {judge}\n", panel.join(", "));
    let (answers, skipped) = fuse::split_replies(fuse::run_panel(
        &panel_prompt,
        &panel,
        None,
        &cwd,
        max_context,
        options.yolo,
    ));
    if answers.len() < 2 {
        // Escalation itself couldn't muster a panel — fall back to the original
        // reply rather than erroring, so the user still gets something.
        if !reply.is_empty() {
            println!("{reply}");
        }
        record_failure(&harness, class, &prompt, elapsed, &reply, failure, false);
        return Err(fuse::insufficient_panel_message(answers.len(), &skipped));
    }
    for note in &skipped {
        eprintln!("━━ {note} (skipped) ━━");
    }
    let fused = fuse::run_judge(
        &panel_prompt,
        &answers,
        &judge,
        options.judge_model.clone(),
        &cwd,
        max_context,
    )?;
    println!("{fused}");

    record_failure(&harness, class, &prompt, elapsed, &reply, failure, true);
    Ok(())
}

/// Decide whether the first agent's run counts as a failure worth escalating.
fn classify_failure(
    out: &crate::process::Captured,
    reply: &str,
    expect_changes: bool,
    before: &Option<Vec<String>>,
    after: &Option<Vec<String>>,
) -> Option<Failure> {
    if out.timed_out {
        return Some(Failure::TimedOut);
    }
    if !out.success {
        return Some(Failure::Errored);
    }
    if signals::is_degenerate(reply) {
        return Some(Failure::Degenerate);
    }
    if signals::has_error_marker(reply) {
        return Some(Failure::ErrorMarker);
    }
    // The "no edit = no progress" oracle: only for tasks that should change
    // files, and only when git could actually measure it.
    if expect_changes && !signals::workspace_changed(before, after) {
        return Some(Failure::NoProgress);
    }
    None
}

/// Build the escalation panel: an explicit `--panel`, else an auto-selected
/// diverse panel that is guaranteed to include an agent other than the one that
/// just failed.
fn escalation_panel(explicit: &[String], prompt: &str, failed: &str, bias: f64) -> Vec<String> {
    if !explicit.is_empty() {
        return fuse::resolve_panel(explicit).unwrap_or_else(|_| default_panel());
    }
    let mut panel = route::pick_panel(prompt, 3, bias, true);
    // Make sure the failed agent isn't the whole panel; ensure ≥2 distinct.
    panel.retain(|p| normalize_harness(p) != normalize_harness(failed));
    if panel.len() < 2 {
        return default_panel();
    }
    panel
}

fn default_panel() -> Vec<String> {
    fuse::DEFAULT_PANEL.iter().map(|s| s.to_string()).collect()
}

/// Prepend the failed attempt to the panel prompt so the panel doesn't repeat
/// the same dead end.
fn seed_prompt(prompt: &str, failed: &str, failure: Failure, reply: &str) -> String {
    let shown = if reply.trim().is_empty() {
        "(it produced no usable output)".to_string()
    } else {
        format!("Its output was:\n\n{reply}")
    };
    format!(
        "{prompt}\n\n---\nNOTE: a previous attempt by `{failed}` was insufficient ({}). {shown}\n\nProduce a correct, complete answer; do not repeat the same mistake.",
        failure.reason()
    )
}

fn is_code_task(class: TaskClass) -> bool {
    matches!(
        class,
        TaskClass::Code | TaskClass::Debug | TaskClass::Refactor | TaskClass::Test
    )
}

fn record_failure(
    harness: &str,
    class: TaskClass,
    prompt: &str,
    elapsed: u128,
    reply: &str,
    failure: Failure,
    escalated: bool,
) {
    Event {
        cmd: "solve",
        harness: Some(harness.to_string()),
        task_class: Some(class.name().to_string()),
        prompt: Some(prompt.to_string()),
        ok: escalated,
        duration_ms: elapsed,
        out_len: reply.len(),
        degenerate: failure == Failure::Degenerate,
        errored: matches!(failure, Failure::Errored | Failure::ErrorMarker),
        timed_out: failure == Failure::TimedOut,
        looped: failure == Failure::NoProgress,
        escalated,
        reason: Some(failure.reason().to_string()),
        ..Event::default()
    }
    .record();
}

fn dry_run(
    prompt: &str,
    harness: &str,
    options: &SolveOptions,
    cwd: &std::path::Path,
    max_context: usize,
    bias: f64,
    class: TaskClass,
) -> Result<(), String> {
    let req = AskRequest {
        harness: harness.to_string(),
        prompt: prompt.to_string(),
        model: None,
        provider: None,
        cwd: cwd.to_path_buf(),
        yolo: options.yolo,
        context: None,
        max_context_chars: max_context,
    };
    println!("# task class: {}", class.name());
    println!("# first agent: {harness} (dry run)");
    println!("{}", ask::build(&req)?.to_json());
    let panel = escalation_panel(&options.panel, prompt, harness, bias);
    println!(
        "# on failure, would escalate to panel: {}",
        panel.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_tasks_expect_file_changes() {
        assert!(is_code_task(TaskClass::Code));
        assert!(is_code_task(TaskClass::Debug));
        assert!(!is_code_task(TaskClass::Explain));
        assert!(!is_code_task(TaskClass::Architecture));
    }

    #[test]
    fn escalation_panel_excludes_the_failed_agent() {
        let panel = escalation_panel(&[], "design an architecture", "claude", 0.7);
        assert!(panel.len() >= 2);
        assert!(!panel.iter().any(|p| normalize_harness(p) == "claude"));
    }

    #[test]
    fn explicit_panel_is_respected() {
        let panel = escalation_panel(
            &["co".to_string(), "g".to_string()],
            "anything",
            "claude",
            0.7,
        );
        // resolve_panel keeps codes verbatim; run_panel normalizes them later.
        assert_eq!(panel, vec!["co".to_string(), "g".to_string()]);
    }

    #[test]
    fn seed_prompt_embeds_the_failed_attempt() {
        let p = seed_prompt(
            "fix the bug",
            "codex",
            Failure::ErrorMarker,
            "it threw an error",
        );
        assert!(p.contains("fix the bug"));
        assert!(p.contains("codex"));
        assert!(p.contains("it threw an error"));
        let empty = seed_prompt("fix it", "qwen", Failure::Degenerate, "   ");
        assert!(empty.contains("no usable output"));
    }
}
