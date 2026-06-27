//! Parley desktop — a fused multi-agent chat app over the local coding-agent
//! CLIs. The backend is a thin orchestrator: it reuses the `par` binary as its
//! brain (all harness/route/fuse logic lives there, single-source) and owns the
//! one thing a GUI needs that the CLI doesn't — live, concurrent streaming.
//!
//! How it drives agents without re-implementing any adapter: for any target it
//! asks `par … --dry-run` for the exact `{command, args, env}` to run (the same
//! argv the CLI would run), then spawns that itself with piped stdout/stderr and
//! streams the output to the UI as Tauri events. Fusion is orchestrated here so
//! each panelist streams into its own pane in parallel, then a judge synthesizes
//! — the `par fuse` engine, but live instead of all-at-once.
//!
//! Because it shells out to the real agent CLIs, it inherits their auth,
//! subscriptions, and prompt caching for free — Parley's whole thesis.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Cap on how much prior-conversation context is replayed into each turn, so a
/// long chat stays responsive. Mirrors `par`'s default context budget.
const MAX_CONTEXT_CHARS: usize = 12_000;

/// The default fuse panel when the user picks "fuse" without naming agents.
const DEFAULT_PANEL: &[&str] = &["claude", "codex", "gemini"];
const DEFAULT_JUDGE: &str = "claude";

// ---- engine: locate + drive `par` ------------------------------------------

/// Absolute name/path of the `par` binary. Override with `PARLEY_BIN`; otherwise
/// rely on it being on PATH (the standard install puts it in ~/.local/bin).
fn par_bin() -> String {
    std::env::var("PARLEY_BIN").unwrap_or_else(|_| "par".to_string())
}

/// A resolved command to run: exactly what `par --dry-run` emits.
#[derive(Debug, Clone, Deserialize)]
struct Invocation {
    command: String,
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Ask `par` for the exact argv it would run for a single agent, without running
/// it. This is the integration seam: all harness/model/yolo logic stays in `par`.
async fn resolve_invocation(
    target: &str,
    prompt: &str,
    model: &Option<String>,
    yolo: bool,
) -> Result<Invocation, String> {
    let mut args = vec![
        "-h".to_string(),
        target.to_string(),
        "-p".to_string(),
        prompt.to_string(),
        "--dry-run".to_string(),
    ];
    if let Some(model) = model {
        if !model.trim().is_empty() {
            args.push("-m".to_string());
            args.push(model.clone());
        }
    }
    if !yolo {
        args.push("--no-yolo".to_string());
    }

    let output = Command::new(par_bin())
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("could not run `par` (is it on PATH?): {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "par --dry-run failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("could not parse par --dry-run output: {e}"))
}

// ---- events streamed to the UI ---------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent {
    chat_id: String,
    msg_id: String,
    /// Which sub-stream this belongs to: "main", an agent name, or "fused".
    pane: String,
    /// "start" | "chunk" | "status" | "done" | "error".
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<i32>,
}

fn emit(app: &AppHandle, ev: ChatEvent) {
    // Best-effort: a dropped event must never crash a stream.
    let _ = app.emit("chat-event", ev);
}

// ---- request from the UI ---------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Turn {
    role: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendReq {
    chat_id: String,
    msg_id: String,
    /// "auto" | "fuse" | "solve" | a real agent code/name.
    target: String,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    /// Prior turns, replayed as context (newest-last).
    #[serde(default)]
    history: Vec<Turn>,
    /// Working directory the agents operate in. Defaults to $HOME.
    #[serde(default)]
    cwd: Option<String>,
    /// Comma/array panel for fuse; empty = default panel.
    #[serde(default)]
    panel: Vec<String>,
    #[serde(default)]
    judge: Option<String>,
    #[serde(default = "default_true")]
    yolo: bool,
}

fn default_true() -> bool {
    true
}

fn resolve_cwd(cwd: &Option<String>) -> PathBuf {
    cwd.as_ref()
        .filter(|c| !c.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Fold prior turns into a single context preamble + the new user message, so a
/// stateless agent call still sees the conversation. Trimmed to the most recent
/// `MAX_CONTEXT_CHARS` characters.
fn build_prompt(history: &[Turn], prompt: &str) -> String {
    if history.is_empty() {
        return prompt.to_string();
    }
    let mut convo = String::new();
    for t in history {
        convo.push_str(&format!("[{}]\n{}\n\n", t.role, t.text.trim()));
    }
    let convo = if convo.chars().count() > MAX_CONTEXT_CHARS {
        let start = convo.chars().count() - MAX_CONTEXT_CHARS;
        let tail: String = convo.chars().skip(start).collect();
        format!("[...earlier turns omitted...]\n{tail}")
    } else {
        convo
    };
    format!(
        "Conversation so far:\n{convo}\n---\n\nUsing the conversation above as context, respond to this:\n\n{prompt}"
    )
}

/// The judge prompt — kept identical to `par fuse`'s synthesizer so the desktop
/// fusion produces the same shape of answer as the CLI.
fn build_judge_prompt(prompt: &str, answers: &[(String, String)]) -> String {
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

// ---- streaming a single child ----------------------------------------------

/// Spawn `inv` and stream its stdout (as `chunk`s) and stderr (as `status`) to
/// the UI under `pane`. Returns the full stdout text (for the fuse judge).
async fn stream_command(
    app: &AppHandle,
    chat_id: &str,
    msg_id: &str,
    pane: &str,
    agent: &str,
    inv: &Invocation,
    cwd: &PathBuf,
) -> Result<String, String> {
    emit(
        app,
        ChatEvent {
            chat_id: chat_id.into(),
            msg_id: msg_id.into(),
            pane: pane.into(),
            kind: "start".into(),
            text: None,
            agent: Some(agent.into()),
            code: None,
        },
    );

    let mut child = Command::new(&inv.command)
        .args(&inv.args)
        .envs(&inv.env)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", inv.command))?;

    let stdout = child.stdout.take().ok_or("no stdout handle")?;
    let stderr = child.stderr.take().ok_or("no stderr handle")?;

    let app_out = app.clone();
    let (cid, mid, pn) = (chat_id.to_string(), msg_id.to_string(), pane.to_string());
    let out_task = tokio::spawn(async move {
        let mut acc = String::new();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            acc.push_str(&line);
            acc.push('\n');
            emit(
                &app_out,
                ChatEvent {
                    chat_id: cid.clone(),
                    msg_id: mid.clone(),
                    pane: pn.clone(),
                    kind: "chunk".into(),
                    text: Some(format!("{line}\n")),
                    agent: None,
                    code: None,
                },
            );
        }
        acc
    });

    let app_err = app.clone();
    let (cid, mid, pn) = (chat_id.to_string(), msg_id.to_string(), pane.to_string());
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            emit(
                &app_err,
                ChatEvent {
                    chat_id: cid.clone(),
                    msg_id: mid.clone(),
                    pane: pn.clone(),
                    kind: "status".into(),
                    text: Some(line),
                    agent: None,
                    code: None,
                },
            );
        }
    });

    let full = out_task.await.unwrap_or_default();
    let _ = err_task.await;
    let status = child.wait().await.map_err(|e| e.to_string())?;

    emit(
        app,
        ChatEvent {
            chat_id: chat_id.into(),
            msg_id: msg_id.into(),
            pane: pane.into(),
            kind: "done".into(),
            text: None,
            agent: None,
            code: status.code(),
        },
    );
    Ok(full)
}

fn emit_error(app: &AppHandle, chat_id: &str, msg_id: &str, pane: &str, message: String) {
    emit(
        app,
        ChatEvent {
            chat_id: chat_id.into(),
            msg_id: msg_id.into(),
            pane: pane.into(),
            kind: "error".into(),
            text: Some(message),
            agent: None,
            code: None,
        },
    );
}

// ---- commands --------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInfo {
    name: String,
    installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentList {
    /// Meta-harnesses that compose the others.
    meta: Vec<String>,
    agents: Vec<AgentInfo>,
    default_panel: Vec<String>,
}

/// Discover selectable targets by asking `par route --json` (its candidate list
/// already carries an `installed` flag per agent).
#[tauri::command]
async fn list_agents() -> Result<AgentList, String> {
    let output = Command::new(par_bin())
        .args(["route", "--json", "ping availability"])
        .output()
        .await
        .map_err(|e| format!("could not run `par route` (is par on PATH?): {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("parse par route: {e}"))?;

    let mut agents = Vec::new();
    if let Some(cands) = parsed.get("candidates").and_then(|c| c.as_array()) {
        for c in cands {
            if let Some(name) = c.get("name").and_then(|n| n.as_str()) {
                agents.push(AgentInfo {
                    name: name.to_string(),
                    installed: c
                        .get("installed")
                        .and_then(|i| i.as_bool())
                        .unwrap_or(false),
                });
            }
        }
    }
    Ok(AgentList {
        meta: vec!["auto".into(), "fuse".into(), "solve".into()],
        agents,
        default_panel: DEFAULT_PANEL.iter().map(|s| s.to_string()).collect(),
    })
}

/// Send one chat message. Streams the answer back as `chat-event`s and resolves
/// when every stream is finished.
#[tauri::command]
async fn send_message(app: AppHandle, req: SendReq) -> Result<(), String> {
    let cwd = resolve_cwd(&req.cwd);
    let prompt = build_prompt(&req.history, &req.prompt);

    let result = match req.target.as_str() {
        "fuse" => run_fuse(&app, &req, &prompt, &cwd).await,
        "solve" => run_solve(&app, &req, &prompt, &cwd).await,
        // "auto" and every real agent go through the same single-stream path;
        // `par` resolves "auto" to the chosen agent's argv for us.
        _ => run_single(&app, &req, &prompt, &cwd).await,
    };

    if let Err(e) = &result {
        emit_error(&app, &req.chat_id, &req.msg_id, "main", e.clone());
    }
    result
}

async fn run_single(
    app: &AppHandle,
    req: &SendReq,
    prompt: &str,
    cwd: &PathBuf,
) -> Result<(), String> {
    let inv = resolve_invocation(&req.target, prompt, &req.model, req.yolo).await?;
    let agent = basename(&inv.command);
    stream_command(app, &req.chat_id, &req.msg_id, "main", &agent, &inv, cwd).await?;
    Ok(())
}

async fn run_solve(
    app: &AppHandle,
    req: &SendReq,
    prompt: &str,
    cwd: &PathBuf,
) -> Result<(), String> {
    // `par solve` owns the route-then-escalate logic; run it directly and stream
    // its stdout (the answer) plus stderr (its routing/escalation notes).
    let mut args = vec!["solve".to_string(), "-p".to_string(), prompt.to_string()];
    if !req.yolo {
        args.push("--no-yolo".to_string());
    }
    let inv = Invocation {
        command: par_bin(),
        args,
        env: BTreeMap::new(),
    };
    stream_command(app, &req.chat_id, &req.msg_id, "main", "solve", &inv, cwd).await?;
    Ok(())
}

async fn run_fuse(
    app: &AppHandle,
    req: &SendReq,
    prompt: &str,
    cwd: &PathBuf,
) -> Result<(), String> {
    let panel: Vec<String> = if req.panel.is_empty() {
        DEFAULT_PANEL.iter().map(|s| s.to_string()).collect()
    } else {
        req.panel.clone()
    };

    // Run every panelist concurrently, each streaming into its own pane.
    let mut handles = Vec::new();
    for agent in panel {
        let app = app.clone();
        let req = req.clone();
        let prompt = prompt.to_string();
        let cwd = cwd.clone();
        handles.push(tokio::spawn(async move {
            let pane = agent.clone();
            match resolve_invocation(&agent, &prompt, &req.model, req.yolo).await {
                Ok(inv) => {
                    let label = basename(&inv.command);
                    let text =
                        stream_command(&app, &req.chat_id, &req.msg_id, &pane, &label, &inv, &cwd)
                            .await
                            .unwrap_or_default();
                    (pane, text)
                }
                Err(e) => {
                    emit_error(&app, &req.chat_id, &req.msg_id, &pane, e);
                    (pane, String::new())
                }
            }
        }));
    }

    let mut answers: Vec<(String, String)> = Vec::new();
    for h in handles {
        if let Ok((pane, text)) = h.await {
            if !text.trim().is_empty() {
                answers.push((pane, text));
            }
        }
    }

    if answers.len() < 2 {
        return Err(format!(
            "fusion needs at least 2 panelists to answer; got {}. Check the panel agents are installed.",
            answers.len()
        ));
    }

    // Judge synthesizes the panel into the "fused" pane.
    let judge = req
        .judge
        .clone()
        .unwrap_or_else(|| DEFAULT_JUDGE.to_string());
    let judge_prompt = build_judge_prompt(&req.prompt, &answers);
    let inv = resolve_invocation(&judge, &judge_prompt, &req.model, req.yolo).await?;
    let label = basename(&inv.command);
    stream_command(app, &req.chat_id, &req.msg_id, "fused", &label, &inv, cwd).await?;
    Ok(())
}

fn basename(command: &str) -> String {
    command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .to_string()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_agents, send_message])
        .run(tauri::generate_context!())
        .expect("error while running Parley desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_passes_through_without_history() {
        assert_eq!(build_prompt(&[], "hello"), "hello");
    }

    #[test]
    fn build_prompt_includes_history() {
        let h = vec![
            Turn {
                role: "you".into(),
                text: "hi".into(),
            },
            Turn {
                role: "claude".into(),
                text: "hello".into(),
            },
        ];
        let p = build_prompt(&h, "next?");
        assert!(p.contains("Conversation so far"));
        assert!(p.contains("[claude]"));
        assert!(p.contains("next?"));
    }

    #[test]
    fn judge_prompt_has_structure_and_replies() {
        let answers = vec![
            ("claude".into(), "token bucket".into()),
            ("codex".into(), "leaky bucket".into()),
        ];
        let p = build_judge_prompt("rate limiter?", &answers);
        assert!(p.contains("CONSENSUS"));
        assert!(p.contains("FINAL ANSWER"));
        assert!(p.contains("token bucket"));
        assert!(p.contains("Reply from codex"));
    }

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/claude"), "claude");
        assert_eq!(basename("codex"), "codex");
    }

    #[test]
    fn invocation_parses_par_dry_run() {
        let json =
            r#"{"command":"claude","args":["-p","hi","--dangerously-skip-permissions"],"env":{}}"#;
        let inv: Invocation = serde_json::from_str(json).unwrap();
        assert_eq!(inv.command, "claude");
        assert_eq!(inv.args.len(), 3);
    }
}
