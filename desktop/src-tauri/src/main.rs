//! Parley desktop — one unified chat, many harnesses underneath.
//!
//! The backend is a stateful orchestrator over the local agent CLIs, driven
//! through the `par` binary (so all harness/route/fuse logic stays single-source
//! in Rust). It adds the three things a great multi-agent chat needs that a
//! one-shot CLI call can't give you:
//!
//!  1. **Warm session pinning.** Each agent keeps a *resumed* session per chat
//!     (`par … --session-id`/`--resume-id`), so follow-ups reuse the agent's own
//!     warm prompt cache instead of re-sending the whole transcript cold.
//!  2. **Shared cross-agent memory.** One canonical transcript per chat. When a
//!     message goes to an agent, it resumes its own warm thread and is fed only
//!     the *delta* — what other agents said since it last spoke — so every agent
//!     stays in one conversation without paying to replay all of it.
//!  3. **Live fan-out + fusion.** Panelists stream into their own panes
//!     concurrently; a judge synthesizes — the `par fuse` engine, but live.
//!
//! Because it drives the real CLIs, it inherits each agent's own auth,
//! subscription, and caching for free — Parley's whole thesis.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Cap on how much catch-up context is replayed in one turn, to stay responsive.
const MAX_CONTEXT_CHARS: usize = 12_000;
const DEFAULT_PANEL: &[&str] = &["claude", "codex", "antigravity"];
const DEFAULT_JUDGE: &str = "claude";

// ---- shared, warm chat state ----------------------------------------------

#[derive(Default)]
struct AppState {
    chats: Mutex<HashMap<String, ChatState>>,
}

#[derive(Default)]
struct ChatState {
    /// The one canonical conversation, shared by every agent in this chat.
    transcript: Vec<Turn>,
    /// Per-agent warm-session bookkeeping.
    pins: HashMap<String, Pin>,
}

#[derive(Default, Clone)]
struct Pin {
    /// A session id we own (claude `--session-id`); None for agents that only
    /// support "resume the most recent" (codex `--last`, gemini `latest`).
    session_id: Option<String>,
    /// True once this agent has run at least once in this chat (so the next turn
    /// can resume warm instead of cold-starting).
    started: bool,
    /// How many transcript turns this agent has already incorporated. The slice
    /// after this index is the delta it needs to catch up on.
    seen: usize,
    calls: u32,
    total_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Turn {
    role: String,
    text: String,
}

// ---- engine: drive `par` ---------------------------------------------------

fn par_bin() -> String {
    std::env::var("PARLEY_BIN").unwrap_or_else(|_| "par".to_string())
}

#[derive(Debug, Clone, Deserialize)]
struct Invocation {
    command: String,
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// How an agent should be run this turn with respect to its session.
#[derive(Debug, Clone, Default)]
struct SessionFlags {
    /// Set a specific id (claude first turn).
    set_id: Option<String>,
    /// Resume an id, or "latest" (warm follow-ups).
    resume: Option<String>,
}

/// Ask `par` for the exact argv for a single agent — the integration seam that
/// keeps all harness/model/session logic in `par`.
async fn resolve_invocation(
    target: &str,
    prompt: &str,
    model: &Option<String>,
    provider: &Option<String>,
    yolo: bool,
    session: &SessionFlags,
) -> Result<Invocation, String> {
    let mut args = vec![
        "-h".into(),
        target.to_string(),
        "-p".into(),
        prompt.to_string(),
        "--dry-run".into(),
    ];
    if let Some(provider) = provider {
        if !provider.trim().is_empty() {
            args.push("--provider".into());
            args.push(provider.clone());
        }
    }
    if let Some(model) = model {
        if !model.trim().is_empty() {
            args.push("-m".into());
            args.push(model.clone());
        }
    }
    if let Some(resume) = &session.resume {
        args.push("--resume-id".into());
        args.push(resume.clone());
    } else if let Some(id) = &session.set_id {
        args.push("--session-id".into());
        args.push(id.clone());
    }
    if !yolo {
        args.push("--no-yolo".into());
    }

    let out = Command::new(par_bin())
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("could not run `par` (is it on PATH?): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "par --dry-run failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .map_err(|e| format!("could not parse par --dry-run output: {e}"))
}

// ---- events to the UI ------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent {
    chat_id: String,
    msg_id: String,
    pane: String,
    kind: String, // start | chunk | status | done | error
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warm: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<u128>,
}

fn emit(app: &AppHandle, ev: ChatEvent) {
    let _ = app.emit("chat-event", ev);
}

// ---- request from the UI ---------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendReq {
    chat_id: String,
    msg_id: String,
    target: String, // auto | fuse | solve | <agent>
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    prompt: String,
    #[serde(default)]
    cwd: Option<String>,
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

// ---- per-agent plan (computed under lock, run without it) -------------------

#[derive(Clone)]
struct AgentPlan {
    agent: String,
    pane: String,
    prompt: String,
    session: SessionFlags,
    warm: bool,
}

/// Build the prompt for an agent: its catch-up delta (if any) + the new message.
fn compose(preamble: &str, message: &str) -> String {
    if preamble.trim().is_empty() {
        return message.to_string();
    }
    let preamble = clamp(preamble, MAX_CONTEXT_CHARS);
    format!("(Context you may have missed in this conversation:)\n{preamble}\n---\n\n{message}")
}

fn clamp(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let start = s.chars().count() - max;
    format!(
        "[...earlier context omitted...]\n{}",
        s.chars().skip(start).collect::<String>()
    )
}

fn render_turns(turns: &[Turn]) -> String {
    turns
        .iter()
        .map(|t| format!("[{}]\n{}", t.role, t.text.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Plan one agent's turn given the chat state. `user_idx` is the index of the
/// just-appended user message in the transcript.
fn plan_agent(chat: &ChatState, agent: &str, user_idx: usize, message: &str) -> AgentPlan {
    let pin = chat.pins.get(agent).cloned().unwrap_or_default();
    let (preamble, session, warm) = if pin.started {
        // Warm: the agent's own thread already holds its history; feed only what
        // happened since it last spoke (other agents' turns), excluding the new
        // user message which is the prompt itself.
        let delta_end = user_idx.min(chat.transcript.len());
        let delta = if pin.seen < delta_end {
            render_turns(&chat.transcript[pin.seen..delta_end])
        } else {
            String::new()
        };
        let resume = Some(pin.session_id.clone().unwrap_or_else(|| "latest".into()));
        (
            delta,
            SessionFlags {
                set_id: None,
                resume,
            },
            true,
        )
    } else {
        // Cold start: bring the agent fully up to speed with the prior transcript
        // (everything before the new user message).
        let prior = render_turns(&chat.transcript[..user_idx]);
        let set_id = if agent == "claude" {
            Some(uuid_v4_like(agent))
        } else {
            None
        };
        (
            prior,
            SessionFlags {
                set_id,
                resume: None,
            },
            false,
        )
    };
    AgentPlan {
        agent: agent.to_string(),
        pane: agent.to_string(),
        prompt: compose(&preamble, message),
        session,
        warm,
    }
}

// ---- streaming a single child ----------------------------------------------

#[allow(clippy::too_many_arguments)] // a cohesive streaming call; bundling would obscure it
async fn stream_command(
    app: &AppHandle,
    chat_id: &str,
    msg_id: &str,
    pane: &str,
    agent: &str,
    warm: bool,
    inv: &Invocation,
    cwd: &PathBuf,
) -> Result<(String, u128), String> {
    let started = Instant::now();
    emit(
        app,
        ChatEvent {
            chat_id: chat_id.into(),
            msg_id: msg_id.into(),
            pane: pane.into(),
            kind: "start".into(),
            text: None,
            agent: Some(agent.into()),
            warm: Some(warm),
            code: None,
            ms: None,
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

    let app2 = app.clone();
    let (c, m, p) = (chat_id.to_string(), msg_id.to_string(), pane.to_string());
    let out_task = tokio::spawn(async move {
        let mut acc = String::new();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            acc.push_str(&line);
            acc.push('\n');
            emit(
                &app2,
                ChatEvent {
                    chat_id: c.clone(),
                    msg_id: m.clone(),
                    pane: p.clone(),
                    kind: "chunk".into(),
                    text: Some(format!("{line}\n")),
                    agent: None,
                    warm: None,
                    code: None,
                    ms: None,
                },
            );
        }
        acc
    });

    let app3 = app.clone();
    let (c, m, p) = (chat_id.to_string(), msg_id.to_string(), pane.to_string());
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            emit(
                &app3,
                ChatEvent {
                    chat_id: c.clone(),
                    msg_id: m.clone(),
                    pane: p.clone(),
                    kind: "status".into(),
                    text: Some(line),
                    agent: None,
                    warm: None,
                    code: None,
                    ms: None,
                },
            );
        }
    });

    let full = out_task.await.unwrap_or_default();
    let _ = err_task.await;
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let ms = started.elapsed().as_millis();

    emit(
        app,
        ChatEvent {
            chat_id: chat_id.into(),
            msg_id: msg_id.into(),
            pane: pane.into(),
            kind: "done".into(),
            text: None,
            agent: None,
            warm: None,
            code: status.code(),
            ms: Some(ms),
        },
    );
    Ok((full, ms))
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
            warm: None,
            code: None,
            ms: None,
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
    meta: Vec<String>,
    agents: Vec<AgentInfo>,
    default_panel: Vec<String>,
}

#[tauri::command]
async fn list_agents() -> Result<AgentList, String> {
    let out = Command::new(par_bin())
        .args(["route", "--json", "ping availability"])
        .output()
        .await
        .map_err(|e| format!("could not run `par route` (is par on PATH?): {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
            .map_err(|e| format!("parse par route: {e}"))?;
    let mut agents = Vec::new();
    if let Some(cands) = parsed.get("candidates").and_then(|c| c.as_array()) {
        for c in cands {
            if let Some(name) = c.get("name").and_then(|n| n.as_str()) {
                agents.push(AgentInfo {
                    name: name.into(),
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

#[tauri::command]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SendReq,
) -> Result<(), String> {
    let cwd = resolve_cwd(&req.cwd);

    // `solve` runs `par solve` end-to-end (it owns route+escalate); it can switch
    // agents mid-run, so it isn't pinned. Feed it the full transcript as context.
    if req.target == "solve" {
        let preamble = {
            let chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
            let chat = chats.get(&req.chat_id);
            chat.map(|c| render_turns(&c.transcript))
                .unwrap_or_default()
        };
        push_user(&state, &req.chat_id, &req.prompt)?;
        let prompt = compose(&preamble, &req.prompt);
        let mut args = vec!["solve".to_string(), "-p".to_string(), prompt];
        if !req.yolo {
            args.push("--no-yolo".into());
        }
        let inv = Invocation {
            command: par_bin(),
            args,
            env: BTreeMap::new(),
        };
        let (text, _) = stream_command(
            &app,
            &req.chat_id,
            &req.msg_id,
            "main",
            "solve",
            false,
            &inv,
            &cwd,
        )
        .await?;
        push_assistant(&state, &req.chat_id, "solve", &text, &[])?;
        return Ok(());
    }

    // Resolve `auto` to a concrete agent up front (so it gets its own warm pin).
    let agents: Vec<String> = if req.target == "fuse" {
        if req.panel.is_empty() {
            DEFAULT_PANEL.iter().map(|s| s.to_string()).collect()
        } else {
            req.panel.clone()
        }
    } else if req.target == "auto" {
        let inv = resolve_invocation(
            "auto",
            &req.prompt,
            &None,
            &None,
            req.yolo,
            &SessionFlags::default(),
        )
        .await?;
        vec![basename(&inv.command)]
    } else {
        vec![req.target.clone()]
    };

    // Append the user turn, then plan every agent under one lock.
    let plans: Vec<AgentPlan> = {
        let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
        let chat = chats.entry(req.chat_id.clone()).or_default();
        chat.transcript.push(Turn {
            role: "you".into(),
            text: req.prompt.clone(),
        });
        let user_idx = chat.transcript.len() - 1;
        agents
            .iter()
            .map(|a| plan_agent(chat, a, user_idx, &req.prompt))
            .collect()
    };

    // Run every agent concurrently, each streaming into its own pane.
    let mut handles = Vec::new();
    for plan in plans {
        let app = app.clone();
        let req = req.clone();
        let cwd = cwd.clone();
        handles.push(tokio::spawn(async move {
            // model/provider apply to a single primary agent; in fuse mode the
            // panelists use their defaults and the judge gets the override.
            let (model, provider) = if req.target == "fuse" {
                (None, None)
            } else {
                (req.model.clone(), req.provider.clone())
            };
            match resolve_invocation(
                &plan.agent,
                &plan.prompt,
                &model,
                &provider,
                req.yolo,
                &plan.session,
            )
            .await
            {
                Ok(inv) => {
                    let (text, ms) = stream_command(
                        &app,
                        &req.chat_id,
                        &req.msg_id,
                        &plan.pane,
                        &plan.agent,
                        plan.warm,
                        &inv,
                        &cwd,
                    )
                    .await
                    .unwrap_or_default();
                    Some((plan, text, ms))
                }
                Err(e) => {
                    emit_error(&app, &req.chat_id, &req.msg_id, &plan.pane, e);
                    None
                }
            }
        }));
    }

    let mut results = Vec::new();
    for h in handles {
        if let Ok(Some(r)) = h.await {
            results.push(r);
        }
    }

    if req.target == "fuse" {
        let answers: Vec<(String, String)> = results
            .iter()
            .filter(|(_, t, _)| !t.trim().is_empty())
            .map(|(p, t, _)| (p.agent.clone(), t.clone()))
            .collect();
        // Commit each panelist's warm pin first.
        commit_pins(&state, &req.chat_id, &results)?;
        if answers.len() < 2 {
            return Err(format!(
                "fusion needs at least 2 panelists; got {}. Check the panel agents are installed.",
                answers.len()
            ));
        }
        let judge = req
            .judge
            .clone()
            .unwrap_or_else(|| DEFAULT_JUDGE.to_string());
        let judge_prompt = build_judge_prompt(&req.prompt, &answers);
        // The judge is the primary harness, so it carries the chosen model/provider.
        let inv = resolve_invocation(
            &judge,
            &judge_prompt,
            &req.model,
            &req.provider,
            req.yolo,
            &SessionFlags::default(),
        )
        .await?;
        let label = basename(&inv.command);
        let (fused, _) = stream_command(
            &app,
            &req.chat_id,
            &req.msg_id,
            "fused",
            &label,
            false,
            &inv,
            &cwd,
        )
        .await?;
        // The fused answer is the canonical turn the whole panel shares next time.
        push_assistant(&state, &req.chat_id, "fused", &fused, &agents)?;
    } else {
        // Single agent (or resolved auto).
        if let Some((plan, text, ms)) = results.into_iter().next() {
            commit_pin(&state, &req.chat_id, &plan.agent, &plan.session, ms)?;
            push_assistant(
                &state,
                &req.chat_id,
                &plan.agent,
                &text,
                std::slice::from_ref(&plan.agent),
            )?;
        }
    }
    Ok(())
}

// ---- state mutators --------------------------------------------------------

fn push_user(state: &State<'_, AppState>, chat_id: &str, text: &str) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    chats
        .entry(chat_id.to_string())
        .or_default()
        .transcript
        .push(Turn {
            role: "you".into(),
            text: text.into(),
        });
    Ok(())
}

/// Append an assistant turn and advance the `seen` cursor for the listed agents
/// so they don't re-read their own contribution as catch-up next turn.
fn push_assistant(
    state: &State<'_, AppState>,
    chat_id: &str,
    role: &str,
    text: &str,
    seen_agents: &[String],
) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    let chat = chats.entry(chat_id.to_string()).or_default();
    chat.transcript.push(Turn {
        role: role.into(),
        text: text.trim().into(),
    });
    let len = chat.transcript.len();
    for a in seen_agents {
        chat.pins.entry(a.clone()).or_default().seen = len;
    }
    Ok(())
}

fn commit_pin(
    state: &State<'_, AppState>,
    chat_id: &str,
    agent: &str,
    session: &SessionFlags,
    ms: u128,
) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    let chat = chats.entry(chat_id.to_string()).or_default();
    let pin = chat.pins.entry(agent.to_string()).or_default();
    pin.started = true;
    pin.calls += 1;
    pin.total_ms += ms;
    if let Some(id) = &session.set_id {
        pin.session_id = Some(id.clone());
    }
    Ok(())
}

fn commit_pins(
    state: &State<'_, AppState>,
    chat_id: &str,
    results: &[(AgentPlan, String, u128)],
) -> Result<(), String> {
    for (plan, _, ms) in results {
        commit_pin(state, chat_id, &plan.agent, &plan.session, *ms)?;
    }
    Ok(())
}

// ---- usage + cockpit commands ---------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentUsage {
    agent: String,
    calls: u32,
    total_ms: u128,
    warm: bool,
}

#[tauri::command]
fn usage_stats(state: State<'_, AppState>) -> Result<Vec<AgentUsage>, String> {
    let chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    let mut agg: BTreeMap<String, AgentUsage> = BTreeMap::new();
    for chat in chats.values() {
        for (agent, pin) in &chat.pins {
            let e = agg.entry(agent.clone()).or_insert(AgentUsage {
                agent: agent.clone(),
                calls: 0,
                total_ms: 0,
                warm: false,
            });
            e.calls += pin.calls;
            e.total_ms += pin.total_ms;
            e.warm = e.warm || (pin.started && pin.session_id.is_some());
        }
    }
    let mut v: Vec<AgentUsage> = agg.into_values().collect();
    v.sort_by_key(|u| std::cmp::Reverse(u.calls));
    Ok(v)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitDiff {
    is_repo: bool,
    files: Vec<String>,
    diff: String,
}

#[tauri::command]
async fn git_diff(cwd: Option<String>) -> Result<GitDiff, String> {
    let dir = resolve_cwd(&cwd);
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !status.status.success() {
        return Ok(GitDiff {
            is_repo: false,
            files: vec![],
            diff: String::new(),
        });
    }
    let files: Vec<String> = String::from_utf8_lossy(&status.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();
    let diff = Command::new("git")
        .args(["diff", "--stat", "--patch"])
        .current_dir(&dir)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    Ok(GitDiff {
        is_repo: true,
        files,
        diff,
    })
}

#[tauri::command]
async fn git_discard(cwd: Option<String>) -> Result<(), String> {
    let dir = resolve_cwd(&cwd);
    let out = Command::new("git")
        .args(["checkout", "--", "."])
        .current_dir(&dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command]
fn reset_chat(state: State<'_, AppState>, chat_id: String) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    chats.remove(&chat_id);
    Ok(())
}

// ---- helpers ---------------------------------------------------------------

fn basename(command: &str) -> String {
    command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .to_string()
}

fn build_judge_prompt(prompt: &str, answers: &[(String, String)]) -> String {
    let mut p = String::new();
    p.push_str(&format!(
        "You are the judge of a multi-model panel. {} agents independently answered the QUESTION below.\n\n",
        answers.len()
    ));
    p.push_str("Produce, in this order, with these exact headings:\n");
    p.push_str("CONSENSUS — claims most or all agents agree on (treat as high-confidence).\n");
    p.push_str("CONTRADICTIONS — where they disagree, and which side is right and why.\n");
    p.push_str("GAPS — important points only one agent raised.\n");
    p.push_str("BLIND SPOTS — anything important that NO agent addressed.\n");
    p.push_str(
        "FINAL ANSWER — the single best answer. Do not average; pick the strongest reasoning.\n\n",
    );
    p.push_str("=== QUESTION ===\n");
    p.push_str(prompt);
    p.push('\n');
    for (label, text) in answers {
        p.push_str(&format!("\n=== Reply from {label} ===\n{text}\n"));
    }
    p
}

/// A v4-shaped uuid from time + a counter + a seed hash. Not cryptographic —
/// just unique enough to name a session and valid in shape for `--session-id`.
fn uuid_v4_like(seed: &str) -> String {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0) as u64;
    let c = CTR.fetch_add(1, Ordering::Relaxed);
    let h = fnv1a(seed);
    let a = n ^ h;
    let b = c.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ h.rotate_left(17);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        (a & 0xfff) as u16,
        0x8000u16 | ((b >> 48) as u16 & 0x3fff),
        b & 0xffff_ffff_ffff
    )
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            send_message,
            usage_stats,
            git_diff,
            git_discard,
            reset_chat
        ])
        .run(tauri::generate_context!())
        .expect("error while running Parley desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: &str, text: &str) -> Turn {
        Turn {
            role: role.into(),
            text: text.into(),
        }
    }

    #[test]
    fn cold_start_plans_full_prior_context_and_sets_claude_id() {
        let mut chat = ChatState::default();
        chat.transcript.push(turn("you", "first"));
        chat.transcript.push(turn("gemini", "an answer"));
        chat.transcript.push(turn("you", "second")); // user_idx = 2
        let plan = plan_agent(&chat, "claude", 2, "second");
        assert!(!plan.warm);
        assert!(plan.prompt.contains("first"));
        assert!(plan.prompt.contains("an answer"));
        assert!(plan.prompt.contains("second"));
        assert!(plan.session.set_id.is_some()); // claude gets a session id
        assert!(plan.session.resume.is_none());
    }

    #[test]
    fn warm_turn_sends_only_the_delta_and_resumes() {
        let mut chat = ChatState::default();
        chat.transcript.push(turn("you", "q1"));
        chat.transcript.push(turn("claude", "a1"));
        // claude is warm, has seen up through index 2 (its own reply)
        chat.pins.insert(
            "claude".into(),
            Pin {
                session_id: Some("sid".into()),
                started: true,
                seen: 2,
                calls: 1,
                total_ms: 10,
            },
        );
        chat.transcript.push(turn("gemini", "g-said-this")); // other agent spoke
        chat.transcript.push(turn("you", "q2")); // user_idx = 3
        let plan = plan_agent(&chat, "claude", 3, "q2");
        assert!(plan.warm);
        assert_eq!(plan.session.resume.as_deref(), Some("sid"));
        // delta includes gemini's turn but NOT claude's own earlier reply
        assert!(plan.prompt.contains("g-said-this"));
        assert!(!plan.prompt.contains("a1"));
        assert!(plan.prompt.contains("q2"));
    }

    #[test]
    fn non_claude_warm_resumes_latest() {
        let mut chat = ChatState::default();
        chat.transcript.push(turn("you", "q1"));
        chat.transcript.push(turn("codex", "a1"));
        chat.pins.insert(
            "codex".into(),
            Pin {
                session_id: None,
                started: true,
                seen: 2,
                calls: 1,
                total_ms: 5,
            },
        );
        chat.transcript.push(turn("you", "q2"));
        let plan = plan_agent(&chat, "codex", 2, "q2");
        assert_eq!(plan.session.resume.as_deref(), Some("latest"));
    }

    #[test]
    fn compose_skips_empty_preamble() {
        assert_eq!(compose("", "hello"), "hello");
        assert!(compose("ctx", "hello").contains("Context you may have missed"));
    }

    #[test]
    fn uuid_is_v4_shaped_and_unique() {
        let a = uuid_v4_like("claude");
        let b = uuid_v4_like("claude");
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4'); // version nibble
    }

    #[test]
    fn judge_prompt_has_headings_and_replies() {
        let answers = vec![
            ("claude".into(), "token bucket".into()),
            ("codex".into(), "leaky".into()),
        ];
        let p = build_judge_prompt("limiter?", &answers);
        assert!(p.contains("CONSENSUS"));
        assert!(p.contains("FINAL ANSWER"));
        assert!(p.contains("token bucket"));
        assert!(p.contains("Reply from codex"));
    }
}
