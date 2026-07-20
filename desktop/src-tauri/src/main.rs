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

use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// In-flight runs per chat, so a chat can be killed mid-processing.
    runs: Mutex<HashMap<String, RunState>>,
}

/// Tracks the live child processes of one chat's current turn. Children are spawned
/// as process-group leaders, so we cancel by signalling the whole group (which also
/// reaps any grandchildren, e.g. `par solve` spawning the underlying agent).
#[derive(Default)]
struct RunState {
    pids: HashSet<u32>,
    canceled: bool,
}

/// Begin a fresh run for a chat (clears any stale cancel flag/pids).
fn run_begin(state: &AppState, chat_id: &str) {
    if let Ok(mut runs) = state.runs.lock() {
        runs.insert(chat_id.to_string(), RunState::default());
    }
}

/// Has this chat's current run been canceled?
fn run_canceled(state: &AppState, chat_id: &str) -> bool {
    state
        .runs
        .lock()
        .map(|r| r.get(chat_id).map(|s| s.canceled).unwrap_or(false))
        .unwrap_or(false)
}

/// Register a freshly spawned child pid. Returns true if the run was already
/// canceled (caller should kill it immediately and bail).
fn run_register(state: &AppState, chat_id: &str, pid: u32) -> bool {
    if let Ok(mut runs) = state.runs.lock() {
        let s = runs.entry(chat_id.to_string()).or_default();
        s.pids.insert(pid);
        return s.canceled;
    }
    false
}

fn run_unregister(state: &AppState, chat_id: &str, pid: u32) {
    if let Ok(mut runs) = state.runs.lock() {
        if let Some(s) = runs.get_mut(chat_id) {
            s.pids.remove(&pid);
        }
    }
}

/// Signal a process group (the child is its own group leader, so pgid == pid).
fn kill_group(pid: u32, sig: i32) {
    #[cfg(unix)]
    unsafe {
        libc::killpg(pid as libc::pid_t, sig);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, sig);
    }
}

#[cfg(unix)]
const SIG_TERM: i32 = libc::SIGTERM;
#[cfg(unix)]
const SIG_KILL: i32 = libc::SIGKILL;
#[cfg(not(unix))]
const SIG_TERM: i32 = 15;
#[cfg(not(unix))]
const SIG_KILL: i32 = 9;

/// Sentinel "done" code the UI renders as "stopped" (a user-killed run).
const CODE_STOPPED: i32 = -15;

#[derive(Default)]
struct ChatState {
    /// The one canonical conversation, shared by every agent in this chat.
    transcript: Vec<Turn>,
    /// Per-agent warm-session bookkeeping.
    pins: HashMap<String, Pin>,
}

#[derive(Default, Clone)]
struct Pin {
    /// The agent this slot belongs to (a slot is `agent|model|provider`, so two
    /// instances of the same agent at different models are separate slots).
    agent: String,
    /// A session id we own (claude `--session-id`); None for agents that only
    /// support "resume the most recent" (codex `--last`, gemini `latest`).
    session_id: Option<String>,
    /// True once this slot has run at least once in this chat (so the next turn
    /// can resume warm instead of cold-starting).
    started: bool,
    /// How many transcript turns this slot has already incorporated. The slice
    /// after this index is the delta it needs to catch up on.
    seen: usize,
    calls: u32,
    total_ms: u128,
}

/// A unique session "slot": the same agent at a different model/provider — or in
/// a different working directory — is a different slot, so each keeps its own warm
/// session. Cwd matters because agent CLIs scope sessions to the directory they
/// ran in (e.g. claude `--resume <id>` only finds the session under that cwd's
/// project), so resuming a pin across a folder change fails. Partitioning by cwd
/// makes a folder switch cold-start a fresh session instead.
fn slot_of(agent: &str, model: &Option<String>, provider: &Option<String>, cwd: &str) -> String {
    format!(
        "{agent}|{}|{}|{cwd}",
        model.as_deref().unwrap_or(""),
        provider.as_deref().unwrap_or("")
    )
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
    /// The resolved command being run, with long args (the prompt) redacted —
    /// surfaced in the UI so you can see the actual invocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    cmd: Option<String>,
}

/// A human-readable command line with long args (prompt/context) redacted, so
/// the UI can show the real invocation without leaking prompt text.
fn redacted_cmd(inv: &Invocation) -> String {
    let mut parts = vec![inv.command.clone()];
    for a in &inv.args {
        parts.push(if a.chars().count() > 40 {
            "«prompt»".to_string()
        } else {
            a.clone()
        });
    }
    parts.join(" ")
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
    /// Fuse panel — each entry is a configurable instance (duplicates allowed:
    /// e.g. claude/opus + claude/sonnet).
    #[serde(default)]
    panel: Vec<Panelist>,
    #[serde(default)]
    judge: Option<String>,
    #[serde(default = "default_true")]
    yolo: bool,
}

/// One configured panel instance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Panelist {
    /// Stable id from the UI; used as the pane key (unique even for duplicates).
    id: String,
    agent: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
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
    pane: String,
    agent: String,
    slot: String,
    model: Option<String>,
    provider: Option<String>,
    prompt: String,
    session: SessionFlags,
    warm: bool,
}

/// One thing to run this turn: a pane, an agent, and its model/provider.
#[derive(Clone)]
struct Target {
    pane: String,
    agent: String,
    model: Option<String>,
    provider: Option<String>,
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

/// Plan one target's turn given the chat state. `user_idx` is the index of the
/// just-appended user message in the transcript. Sessions are keyed by *slot*
/// (agent|model|provider|cwd) so the same agent at different models — or after a
/// folder change — stays separate.
fn plan_agent(
    chat: &ChatState,
    target: &Target,
    user_idx: usize,
    message: &str,
    cwd: &str,
) -> AgentPlan {
    let agent = target.agent.as_str();
    let slot = slot_of(agent, &target.model, &target.provider, cwd);
    let pin = chat.pins.get(&slot).cloned().unwrap_or_default();
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
        pane: target.pane.clone(),
        agent: agent.to_string(),
        slot,
        model: target.model.clone(),
        provider: target.provider.clone(),
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
            cmd: Some(redacted_cmd(inv)),
        },
    );

    let mut cmd = Command::new(&inv.command);
    cmd.args(&inv.args)
        .envs(&inv.env)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // Own process group so cancel can signal the whole tree (incl. any agent that
    // `par` itself spawns), not just the direct child.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", inv.command))?;

    // Register the live child so `cancel_chat` can kill it. If the run was already
    // canceled before we got here, kill it now.
    let state = app.state::<AppState>();
    let pid = child.id();
    if let Some(pid) = pid {
        if run_register(&state, chat_id, pid) {
            kill_group(pid, SIG_KILL);
        }
    }

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
                    cmd: None,
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
                    cmd: None,
                },
            );
        }
    });

    let full = out_task.await.unwrap_or_default();
    let _ = err_task.await;
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let ms = started.elapsed().as_millis();
    if let Some(pid) = pid {
        run_unregister(&state, chat_id, pid);
    }
    // A canceled run was killed by a signal (no exit code); flag it with a sentinel
    // so the UI shows "stopped" rather than a scary "exit 1".
    let code = if run_canceled(&state, chat_id) {
        Some(CODE_STOPPED)
    } else {
        status.code()
    };

    // A clean exit with no output at all is a silent failure for a chat turn:
    // some agents swallow backend errors this way (antigravity exits 0 with empty
    // stdout *and* stderr when it hits a rate limit or quota, logging the 429 only
    // to its own file). Without this the pane just renders blank under a green
    // "✓ exit 0", which reads as a hang. Surface it instead.
    if full.trim().is_empty() && code != Some(CODE_STOPPED) {
        emit(
            app,
            ChatEvent {
                chat_id: chat_id.into(),
                msg_id: msg_id.into(),
                pane: pane.into(),
                // A "chunk" lands in the pane body (kept after "done"); a "status"
                // event is transient and only shows while the run is live, so it
                // would vanish on a finished empty pane.
                kind: "chunk".into(),
                text: Some(format!(
                    "⚠ no output (exit {}) — the agent returned nothing. It may have errored, been rate-limited, or hit its quota; check that agent's own logs.",
                    code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
                )),
                agent: None,
                warm: None,
                code: None,
                ms: None,
                cmd: None,
            },
        );
    }

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
            code,
            ms: Some(ms),
            cmd: None,
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
            cmd: None,
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
    run_begin(&state, &req.chat_id);

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
        if run_canceled(&state, &req.chat_id) {
            return Ok(());
        }
        push_assistant(&state, &req.chat_id, "solve", &text, &[])?;
        return Ok(());
    }

    // Build the targets to run this turn. Fuse → one per configured panelist
    // (duplicates allowed, each with its own model/provider). Single/auto → one,
    // carrying the chosen model/provider.
    let targets: Vec<Target> = if req.target == "fuse" {
        if req.panel.is_empty() {
            DEFAULT_PANEL
                .iter()
                .map(|a| Target {
                    pane: (*a).to_string(),
                    agent: (*a).to_string(),
                    model: None,
                    provider: None,
                })
                .collect()
        } else {
            req.panel
                .iter()
                .map(|p| Target {
                    pane: p.id.clone(),
                    agent: p.agent.clone(),
                    model: p.model.clone(),
                    provider: p.provider.clone(),
                })
                .collect()
        }
    } else if req.target == "auto" {
        let inv = resolve_invocation(
            "auto",
            &req.prompt,
            &req.model,
            &req.provider,
            req.yolo,
            &SessionFlags::default(),
        )
        .await?;
        vec![Target {
            pane: "main".into(),
            agent: basename(&inv.command),
            model: req.model.clone(),
            provider: req.provider.clone(),
        }]
    } else {
        vec![Target {
            pane: "main".into(),
            agent: req.target.clone(),
            model: req.model.clone(),
            provider: req.provider.clone(),
        }]
    };

    // Append the user turn, then plan every target under one lock.
    let plans: Vec<AgentPlan> = {
        let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
        let chat = chats.entry(req.chat_id.clone()).or_default();
        chat.transcript.push(Turn {
            role: "you".into(),
            text: req.prompt.clone(),
        });
        let user_idx = chat.transcript.len() - 1;
        let cwd_key = cwd.to_string_lossy();
        targets
            .iter()
            .map(|t| plan_agent(chat, t, user_idx, &req.prompt, &cwd_key))
            .collect()
    };

    // Run every target concurrently, each streaming into its own pane.
    let mut handles = Vec::new();
    for plan in plans {
        let app = app.clone();
        let req = req.clone();
        let cwd = cwd.clone();
        handles.push(tokio::spawn(async move {
            match resolve_invocation(
                &plan.agent,
                &plan.prompt,
                &plan.model,
                &plan.provider,
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

    // Killed mid-run: don't synthesize a fused answer or commit warm pins from a
    // partial turn.
    if run_canceled(&state, &req.chat_id) {
        return Ok(());
    }

    if req.target == "fuse" {
        // Label distinguishes duplicate agents (claude (opus) vs claude (sonnet)).
        let answers: Vec<(String, String)> = results
            .iter()
            .filter(|(_, t, _)| !t.trim().is_empty())
            .map(|(p, t, _)| (panelist_label(&p.agent, &p.model), t.clone()))
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
        let slots: Vec<String> = results.iter().map(|(p, _, _)| p.slot.clone()).collect();
        push_assistant(&state, &req.chat_id, "fused", &fused, &slots)?;
    } else {
        // Single agent (or resolved auto).
        if let Some((plan, text, ms)) = results.into_iter().next() {
            commit_pin(
                &state,
                &req.chat_id,
                &plan.slot,
                &plan.agent,
                &plan.session,
                ms,
            )?;
            push_assistant(
                &state,
                &req.chat_id,
                &plan.agent,
                &text,
                std::slice::from_ref(&plan.slot),
            )?;
        }
    }
    Ok(())
}

/// Display label for a panelist, distinguishing duplicate agents by model.
fn panelist_label(agent: &str, model: &Option<String>) -> String {
    match model {
        Some(m) if !m.trim().is_empty() => format!("{agent} ({m})"),
        _ => agent.to_string(),
    }
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

/// Append an assistant turn and advance the `seen` cursor for the listed slots
/// so they don't re-read their own contribution as catch-up next turn.
fn push_assistant(
    state: &State<'_, AppState>,
    chat_id: &str,
    role: &str,
    text: &str,
    seen_slots: &[String],
) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    let chat = chats.entry(chat_id.to_string()).or_default();
    chat.transcript.push(Turn {
        role: role.into(),
        text: text.trim().into(),
    });
    let len = chat.transcript.len();
    for s in seen_slots {
        chat.pins.entry(s.clone()).or_default().seen = len;
    }
    Ok(())
}

fn commit_pin(
    state: &State<'_, AppState>,
    chat_id: &str,
    slot: &str,
    agent: &str,
    session: &SessionFlags,
    ms: u128,
) -> Result<(), String> {
    let mut chats = state.chats.lock().map_err(|_| "state lock poisoned")?;
    let chat = chats.entry(chat_id.to_string()).or_default();
    let pin = chat.pins.entry(slot.to_string()).or_default();
    pin.agent = agent.to_string();
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
        commit_pin(state, chat_id, &plan.slot, &plan.agent, &plan.session, *ms)?;
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
        for pin in chat.pins.values() {
            if pin.agent.is_empty() {
                continue;
            }
            let e = agg.entry(pin.agent.clone()).or_insert(AgentUsage {
                agent: pin.agent.clone(),
                calls: 0,
                total_ms: 0,
                warm: false,
            });
            e.calls += pin.calls;
            e.total_ms += pin.total_ms;
            e.warm = e.warm || pin.started;
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
    branch: String,
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
            branch: String::new(),
            files: vec![],
            diff: String::new(),
        });
    }
    let files: Vec<String> = String::from_utf8_lossy(&status.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&dir)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let diff = Command::new("git")
        .args(["diff", "--stat", "--patch"])
        .current_dir(&dir)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    Ok(GitDiff {
        is_repo: true,
        branch,
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

/// Kill the chat's in-flight run: mark it canceled and signal every live child's
/// process group (TERM then KILL). Safe to call when nothing is running.
#[tauri::command]
fn cancel_chat(state: State<'_, AppState>, chat_id: String) -> Result<(), String> {
    let pids: Vec<u32> = {
        let mut runs = state.runs.lock().map_err(|_| "state lock poisoned")?;
        let s = runs.entry(chat_id).or_default();
        s.canceled = true;
        s.pids.iter().copied().collect()
    };
    for pid in pids {
        kill_group(pid, SIG_TERM);
        kill_group(pid, SIG_KILL);
    }
    Ok(())
}

/// List the harness's own slash commands available in `cwd` (for `/` autocomplete
/// in the composer). Claude/Codex read their project (and user) command dirs;
/// the command text is passed through to the agent, which runs it.
#[tauri::command]
fn list_slash_commands(cwd: Option<String>, harness: String) -> Result<Vec<String>, String> {
    let base = resolve_cwd(&cwd);
    let mut out = Vec::new();
    match harness.as_str() {
        "claude" => {
            let proj = base.join(".claude").join("commands");
            collect_md_commands(&proj, &proj, &mut out);
            if let Ok(home) = std::env::var("HOME") {
                let user = PathBuf::from(home).join(".claude").join("commands");
                collect_md_commands(&user, &user, &mut out);
            }
        }
        "codex" => {
            let p = base.join(".codex").join("prompts");
            collect_md_commands(&p, &p, &mut out);
        }
        _ => {}
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn collect_md_commands(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_md_commands(root, &p, out);
        } else if p.extension().map(|x| x == "md").unwrap_or(false) {
            if let Ok(rel) = p.strip_prefix(root) {
                let name = rel
                    .with_extension("")
                    .to_string_lossy()
                    .replace(['/', '\\'], ":");
                out.push(format!("/{name}"));
            }
        }
    }
}

/// Search files under `cwd` for `@`-file references. Skips VCS/build/hidden dirs;
/// bounded so a huge tree stays responsive.
#[tauri::command]
fn list_files(
    cwd: Option<String>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let base = resolve_cwd(&cwd);
    let q = query.to_lowercase();
    let cap = limit.unwrap_or(60);
    let mut out = Vec::new();
    let mut budget: usize = 12_000;
    walk_files(&base, &base, &q, cap, &mut out, &mut budget);
    out.sort();
    Ok(out)
}

fn walk_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    q: &str,
    cap: usize,
    out: &mut Vec<String>,
    budget: &mut usize,
) {
    if out.len() >= cap || *budget == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if out.len() >= cap || *budget == 0 {
            break;
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            walk_files(root, &p, q, cap, out, budget);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rs = rel.to_string_lossy().into_owned();
            if q.is_empty() || rs.to_lowercase().contains(q) {
                out.push(rs);
            }
        }
    }
}

/// Read a file's current contents (for the in-app editor). Bounded.
#[tauri::command]
fn read_file(cwd: Option<String>, path: String) -> Result<String, String> {
    let p = resolve_cwd(&cwd).join(&path);
    let meta = std::fs::metadata(&p).map_err(|e| format!("stat {}: {e}", p.display()))?;
    if meta.len() > 600_000 {
        return Err("file too large to view".into());
    }
    std::fs::read_to_string(&p).map_err(|e| format!("read {}: {e}", p.display()))
}

/// The committed (HEAD) version of a file, for the diff editor. Empty string
/// when the file is new/untracked.
#[tauri::command]
async fn git_head_file(cwd: Option<String>, path: String) -> Result<String, String> {
    let dir = resolve_cwd(&cwd);
    let out = Command::new("git")
        .args(["show", &format!("HEAD:{path}")])
        .current_dir(&dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    Ok(if out.status.success() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        String::new()
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirEntry {
    name: String,
    path: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirListing {
    path: String,
    parent: Option<String>,
    dirs: Vec<DirEntry>,
}

/// List sub-directories of `path` (or $HOME when empty) for the in-app folder
/// explorer. Directories only; hidden entries skipped; sorted.
#[tauri::command]
fn list_dir(path: Option<String>) -> Result<DirListing, String> {
    let dir = match path.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/")),
    };
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let mut dirs = Vec::new();
    let read = std::fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push(DirEntry {
                name,
                path: entry.path().to_string_lossy().into_owned(),
            });
        }
    }
    dirs.sort_by_key(|d| d.name.to_lowercase());
    Ok(DirListing {
        path: dir.to_string_lossy().into_owned(),
        parent: dir.parent().map(|p| p.to_string_lossy().into_owned()),
        dirs,
    })
}

/// Save a pasted/attached image to a temp file and return its absolute path, so
/// the prompt can reference it for the agent to open (with its own file tools).
#[tauri::command]
fn save_paste(name: String, data: Vec<u8>) -> Result<String, String> {
    let dir = std::env::temp_dir().join("parley-attachments");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create attachments dir: {e}"))?;
    // Sanitize the name to a safe basename.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file = dir.join(format!(
        "{stamp}-{}",
        if safe.is_empty() {
            "paste.png".into()
        } else {
            safe
        }
    ));
    std::fs::write(&file, &data).map_err(|e| format!("write attachment: {e}"))?;
    Ok(file.to_string_lossy().into_owned())
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
        .plugin(tauri_plugin_dialog::init())
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
            reset_chat,
            cancel_chat,
            save_paste,
            list_dir,
            list_slash_commands,
            list_files,
            read_file,
            git_head_file
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

    fn target(agent: &str) -> Target {
        Target {
            pane: agent.into(),
            agent: agent.into(),
            model: None,
            provider: None,
        }
    }

    #[test]
    fn cold_start_plans_full_prior_context_and_sets_claude_id() {
        let mut chat = ChatState::default();
        chat.transcript.push(turn("you", "first"));
        chat.transcript.push(turn("gemini", "an answer"));
        chat.transcript.push(turn("you", "second")); // user_idx = 2
        let plan = plan_agent(&chat, &target("claude"), 2, "second", "/repo");
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
            slot_of("claude", &None, &None, "/repo"),
            Pin {
                agent: "claude".into(),
                session_id: Some("sid".into()),
                started: true,
                seen: 2,
                calls: 1,
                total_ms: 10,
            },
        );
        chat.transcript.push(turn("gemini", "g-said-this")); // other agent spoke
        chat.transcript.push(turn("you", "q2")); // user_idx = 3
        let plan = plan_agent(&chat, &target("claude"), 3, "q2", "/repo");
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
            slot_of("codex", &None, &None, "/repo"),
            Pin {
                agent: "codex".into(),
                session_id: None,
                started: true,
                seen: 2,
                calls: 1,
                total_ms: 5,
            },
        );
        chat.transcript.push(turn("you", "q2"));
        let plan = plan_agent(&chat, &target("codex"), 2, "q2", "/repo");
        assert_eq!(plan.session.resume.as_deref(), Some("latest"));
    }

    #[test]
    fn same_agent_different_model_are_separate_slots() {
        let opus = slot_of("claude", &Some("opus".into()), &None, "/repo");
        let sonnet = slot_of("claude", &Some("sonnet".into()), &None, "/repo");
        assert_ne!(opus, sonnet);
        assert_eq!(
            panelist_label("claude", &Some("opus".into())),
            "claude (opus)"
        );
        assert_eq!(panelist_label("claude", &None), "claude");
    }

    #[test]
    fn changing_cwd_cold_starts_a_fresh_session() {
        // A warm claude pin from one folder must NOT be resumed in another folder
        // (agent sessions are dir-scoped — resuming there fails). The new cwd is a
        // separate slot, so it cold-starts with a fresh id and the full transcript.
        let mut chat = ChatState::default();
        chat.transcript.push(turn("you", "q1"));
        chat.transcript.push(turn("claude", "a1"));
        chat.pins.insert(
            slot_of("claude", &None, &None, "/home/me"),
            Pin {
                agent: "claude".into(),
                session_id: Some("home-sid".into()),
                started: true,
                seen: 2,
                calls: 1,
                total_ms: 10,
            },
        );
        chat.transcript.push(turn("you", "q2")); // user_idx = 2
                                                 // Same agent, but now in a different directory.
        let plan = plan_agent(&chat, &target("claude"), 2, "q2", "/home/me/project");
        assert!(!plan.warm); // cold start, not a resume
        assert!(plan.session.resume.is_none());
        assert!(plan.session.set_id.is_some());
        assert_ne!(plan.session.set_id.as_deref(), Some("home-sid"));
        assert!(plan.prompt.contains("a1")); // full prior context replayed as preamble
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
