//! Minimal MCP server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Lets any MCP-capable agent ask "what's my last conversation here?" and get
//! back resumable session details plus the exact native resume command —
//! scoped, like every harness's own `--resume`, to a working directory.
//!
//! Built on the crate's zero-dependency `Json` type. The protocol surface is
//! intentionally small: `initialize`, `tools/list`, and `tools/call` with five
//! tools — three for session discovery/resume, `ask_agent` for one-shot
//! agent-to-agent calls, and `fuse` to convene a panel of agents on one prompt.
//! Resume tools return *commands* as text and never spawn an interactive
//! harness; `ask_agent` and `fuse` run the target agents headless.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::ask::{self, AskRequest, ContextRef};
use crate::cli::McpOptions;
use crate::fuse;
use crate::harness::{normalize_harness, Invocation};
use crate::json::Json;
use crate::process::run_invocation;
use crate::session;

const PROTOCOL_VERSION: &str = "2024-11-05";

pub(crate) fn run(_options: McpOptions) -> Result<(), String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin read error: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match Json::parse(trimmed) {
            Ok(request) => handle_request(&request, &cwd),
            Err(_) => Some(error_response(&Json::Null, -32700, "parse error")),
        };
        if let Some(response) = response {
            writeln!(out, "{}", response.to_compact_string())
                .map_err(|e| format!("stdout write error: {e}"))?;
            out.flush()
                .map_err(|e| format!("stdout flush error: {e}"))?;
        }
    }
    Ok(())
}

/// Register `par mcp` as an MCP server inside a harness. For harnesses with a
/// native `mcp add` we run it (some, like opencode, open their own add TUI);
/// cursor has no add command, so we merge `~/.cursor/mcp.json` directly.
pub(crate) fn connect(harness: &str, dry_run: bool) -> Result<(), String> {
    let normalized = normalize_harness(harness);
    let bin = par_bin();
    let b = bin.as_str();

    match normalized.as_str() {
        "claude" => exec_or_print(
            Invocation::new("claude", argv(&["mcp", "add", "-s", "user", "par", "--", b, "mcp"])),
            dry_run,
        ),
        "codex" => exec_or_print(
            Invocation::new("codex", argv(&["mcp", "add", "par", "--", b, "mcp"])),
            dry_run,
        ),
        "gemini" => exec_or_print(
            Invocation::new("gemini", argv(&["mcp", "add", "par", b, "mcp"])),
            dry_run,
        ),
        "opencode" => {
            eprintln!("opencode registers MCP servers interactively; launching `opencode mcp add`.");
            eprintln!("  When prompted: name = par, type = local, command = {b} mcp");
            exec_or_print(Invocation::new("opencode", argv(&["mcp", "add"])), dry_run)
        }
        "cursor" => connect_cursor(b, dry_run),
        other => Err(format!(
            "mcp connect does not support \"{other}\" yet (supported: claude, codex, gemini, opencode, cursor)"
        )),
    }
}

/// Run a registration command, or print it under `--dry-run`. Running replaces
/// the process (inheriting stdio, so a harness's add TUI works).
fn exec_or_print(inv: Invocation, dry_run: bool) -> Result<(), String> {
    if dry_run {
        println!("{}", session::render_command(&inv));
        Ok(())
    } else {
        run_invocation(inv, None, true)
    }
}

/// Merge a `par` server entry into `~/.cursor/mcp.json` (cursor has no
/// `mcp add` subcommand), preserving any existing servers.
fn connect_cursor(bin: &str, dry_run: bool) -> Result<(), String> {
    let home = session::home_dir().ok_or("cannot resolve HOME")?;
    let path = home.join(".cursor").join("mcp.json");

    let server = obj(vec![
        ("command", Json::Str(bin.to_string())),
        ("args", Json::Array(vec![Json::Str("mcp".to_string())])),
    ]);

    let mut root = match fs::read_to_string(&path)
        .ok()
        .and_then(|raw| Json::parse(&raw).ok())
    {
        Some(Json::Object(map)) => map,
        _ => BTreeMap::new(),
    };
    let mut servers = match root.get("mcpServers") {
        Some(Json::Object(map)) => map.clone(),
        _ => BTreeMap::new(),
    };
    servers.insert("par".to_string(), server);
    root.insert("mcpServers".to_string(), Json::Object(servers));
    let merged = Json::Object(root);

    if dry_run {
        println!("# would write {}", path.display());
        print!("{}", merged.to_pretty_string());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(&path, merged.to_pretty_string())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("Registered par MCP server in {}", path.display());
    println!("  command: {bin} mcp");
    Ok(())
}

/// Absolute path to the running `par` binary, so the registered command works
/// regardless of the caller's PATH. Falls back to the bare name.
fn par_bin() -> String {
    env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "par".to_string())
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Handle one JSON-RPC request. Returns `None` for notifications (no `id`),
/// which take no response.
pub(crate) fn handle_request(request: &Json, default_cwd: &Path) -> Option<Json> {
    let method = request.get("method").and_then(Json::as_str).unwrap_or("");
    // Notifications carry no id and expect no reply.
    let id = request.get("id")?;

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => call_tool(request, default_cwd),
        "ping" => Ok(obj(vec![])),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(value) => success_response(id, value),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn initialize_result() -> Json {
    obj(vec![
        ("protocolVersion", Json::Str(PROTOCOL_VERSION.to_string())),
        ("capabilities", obj(vec![("tools", obj(vec![]))])),
        (
            "serverInfo",
            obj(vec![
                ("name", Json::Str("parley".to_string())),
                ("version", Json::Str(env!("CARGO_PKG_VERSION").to_string())),
            ]),
        ),
    ])
}

fn tools_list_result() -> Json {
    let cwd_prop = obj(vec![
        ("type", Json::Str("string".to_string())),
        (
            "description",
            Json::Str(
                "Working directory to scope sessions to (defaults to the server's cwd)."
                    .to_string(),
            ),
        ),
    ]);
    let harness_prop = obj(vec![
        ("type", Json::Str("string".to_string())),
        (
            "description",
            Json::Str("Optional harness filter: claude, codex, opencode, cursor, gemini (shorthands allowed).".to_string()),
        ),
    ]);

    let list_tool = tool(
        "list_sessions",
        "List resumable agent sessions for a directory across all harnesses, newest first.",
        obj(vec![
            ("cwd", cwd_prop.clone()),
            ("harness", harness_prop.clone()),
        ]),
        vec![],
    );
    let last_tool = tool(
        "get_last_session",
        "Get the most recent resumable session for a directory, with a ready-to-run resume command. Use this for 'pick up my last conversation from <agent>'.",
        obj(vec![(
            "cwd",
            cwd_prop.clone(),
        ), (
            "harness",
            harness_prop.clone(),
        )]),
        vec![],
    );
    let resume_tool = tool(
        "resume_command",
        "Build the native resume command for a specific harness + session id (does not run it).",
        obj(vec![
            ("harness", harness_prop),
            (
                "id",
                obj(vec![
                    ("type", Json::Str("string".to_string())),
                    (
                        "description",
                        Json::Str("Session id to resume.".to_string()),
                    ),
                ]),
            ),
            ("cwd", cwd_prop),
            (
                "yolo",
                obj(vec![
                    ("type", Json::Str("boolean".to_string())),
                    (
                        "description",
                        Json::Str("Append the harness's permission-bypass flag.".to_string()),
                    ),
                ]),
            ),
        ]),
        vec!["harness", "id"],
    );

    let str_prop = |desc: &str| {
        obj(vec![
            ("type", Json::Str("string".to_string())),
            ("description", Json::Str(desc.to_string())),
        ])
    };
    let ask_tool = tool(
        "ask_agent",
        "Ask another agent (claude, codex, gemini, opencode, cursor, ...) a one-shot question headless and return its reply. Optionally seed it with a prior session's transcript via `context_from` — this is how one agent hands its conversation to another.",
        obj(vec![
            ("harness", str_prop("Target agent to ask (shorthands allowed).")),
            ("prompt", str_prop("The question or task for the target agent.")),
            ("model", str_prop("Optional model override.")),
            ("provider", str_prop("Optional provider override.")),
            ("cwd", str_prop("Working directory (defaults to the server's cwd).")),
            (
                "context_from",
                obj(vec![
                    ("type", Json::Str("object".to_string())),
                    (
                        "description",
                        Json::Str("Seed the call with another agent's session transcript.".to_string()),
                    ),
                    (
                        "properties",
                        obj(vec![
                            ("harness", str_prop("Source agent (claude, codex, opencode).")),
                            ("session", str_prop("Session id, or 'latest' / omitted for the newest in cwd.")),
                        ]),
                    ),
                    ("required", Json::Array(vec![Json::Str("harness".to_string())])),
                ]),
            ),
        ]),
        vec!["harness", "prompt"],
    );

    let fuse_tool = tool(
        "fuse",
        "Convene a PANEL of agents on one prompt and get back a single, stronger synthesized answer. Sends `prompt` to every agent in `panel` in parallel, then a judge agent (Claude by default) fuses their replies: consensus is high-confidence, contradictions are resolved, gaps filled, blind spots flagged. Use on high-stakes questions where being wrong is expensive (design, security, migrations, hard trade-offs) and a diverse panel (different vendors) beats any single model. Seed the panel with a prior session via `context_from`. Returns the fused answer as text.",
        obj(vec![
            ("prompt", str_prop("The question or task to put to the whole panel.")),
            (
                "panel",
                obj(vec![
                    ("type", Json::Str("array".to_string())),
                    ("items", obj(vec![("type", Json::Str("string".to_string()))])),
                    (
                        "description",
                        Json::Str("Panel agents (shorthands allowed), e.g. [\"claude\",\"codex\",\"gemini\"]. Needs at least 2; duplicates allowed. Defaults to claude,codex,gemini.".to_string()),
                    ),
                ]),
            ),
            ("judge", str_prop("Agent that synthesizes the panel into the final answer. Defaults to claude.")),
            ("judge_model", str_prop("Optional model override for the judge.")),
            ("cwd", str_prop("Working directory (defaults to the server's cwd).")),
            (
                "context_from",
                obj(vec![
                    ("type", Json::Str("object".to_string())),
                    (
                        "description",
                        Json::Str("Seed every panelist with another agent's session transcript.".to_string()),
                    ),
                    (
                        "properties",
                        obj(vec![
                            ("harness", str_prop("Source agent (claude, codex, opencode).")),
                            ("session", str_prop("Session id, or 'latest' / omitted for the newest in cwd.")),
                        ]),
                    ),
                    ("required", Json::Array(vec![Json::Str("harness".to_string())])),
                ]),
            ),
        ]),
        vec!["prompt"],
    );

    obj(vec![(
        "tools",
        Json::Array(vec![list_tool, last_tool, resume_tool, ask_tool, fuse_tool]),
    )])
}

fn call_tool(request: &Json, default_cwd: &Path) -> Result<Json, (i64, String)> {
    let params = request
        .get("params")
        .ok_or((-32602, "missing params".to_string()))?;
    let name = params
        .get("name")
        .and_then(Json::as_str)
        .ok_or((-32602, "missing tool name".to_string()))?;
    let empty = Json::Object(BTreeMap::new());
    let args = params.get("arguments").unwrap_or(&empty);

    let cwd = arg_cwd(args, default_cwd);
    let harness = args.get("harness").and_then(Json::as_str);

    match name {
        "list_sessions" => {
            let json = session::list_sessions_json(&cwd, harness);
            Ok(text_content(&json.to_pretty_string(), false))
        }
        "get_last_session" => match session::last_session_json(&cwd, harness) {
            Some(json) => Ok(text_content(&json.to_pretty_string(), false)),
            None => Ok(text_content(
                &format!("No resumable sessions for {}", cwd.display()),
                false,
            )),
        },
        "resume_command" => {
            let harness = harness.ok_or((-32602, "missing harness".to_string()))?;
            let id = args.get("id").and_then(Json::as_str).unwrap_or("");
            let yolo = args.get("yolo").and_then(Json::as_bool).unwrap_or(false);
            match session::resume_command_string(harness, id, &cwd, yolo) {
                Ok(cmd) => Ok(text_content(&cmd, false)),
                Err(e) => Ok(text_content(&e, true)),
            }
        }
        "ask_agent" => {
            let harness = harness.ok_or((-32602, "missing harness".to_string()))?;
            let prompt = args
                .get("prompt")
                .and_then(Json::as_str)
                .ok_or((-32602, "missing prompt".to_string()))?;
            let context = args.get("context_from").and_then(|c| {
                c.get("harness").and_then(Json::as_str).map(|h| ContextRef {
                    harness: h.to_string(),
                    session: c
                        .get("session")
                        .and_then(Json::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
            });
            let request = AskRequest {
                harness: harness.to_string(),
                prompt: prompt.to_string(),
                model: args.get("model").and_then(Json::as_str).map(str::to_string),
                provider: args
                    .get("provider")
                    .and_then(Json::as_str)
                    .map(str::to_string),
                cwd: cwd.clone(),
                // Headless capture: yolo on so the agent can't block on a prompt.
                yolo: args.get("yolo").and_then(Json::as_bool).unwrap_or(true),
                context,
                max_context_chars: args
                    .get("max_context_chars")
                    .and_then(Json::as_number)
                    .map(|n| n as usize)
                    .unwrap_or(session::DEFAULT_CONTEXT_CHARS),
            };
            match ask::run(&request) {
                Ok(out) if out.success => Ok(text_content(out.stdout.trim(), false)),
                Ok(out) => {
                    let msg = if out.stderr.trim().is_empty() {
                        out.stdout.trim().to_string()
                    } else {
                        out.stderr.trim().to_string()
                    };
                    Ok(text_content(&format!("{harness} failed: {msg}"), true))
                }
                Err(e) => Ok(text_content(&e, true)),
            }
        }
        "fuse" => {
            let prompt = args
                .get("prompt")
                .and_then(Json::as_str)
                .ok_or((-32602, "missing prompt".to_string()))?;

            // Panel: explicit array of agent codes, else the default trio.
            let panel_arg: Vec<String> = match args.get("panel").and_then(Json::as_array) {
                Some(items) => items
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_string)
                    .collect(),
                None => Vec::new(),
            };
            let panel = match fuse::resolve_panel(&panel_arg) {
                Ok(p) => p,
                Err(e) => return Ok(text_content(&e, true)),
            };

            let context = args.get("context_from").and_then(|c| {
                c.get("harness").and_then(Json::as_str).map(|h| ContextRef {
                    harness: h.to_string(),
                    session: c
                        .get("session")
                        .and_then(Json::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
            });
            let max_context = args
                .get("max_context")
                .and_then(Json::as_number)
                .map(|n| n as usize)
                .unwrap_or(session::DEFAULT_CONTEXT_CHARS);
            let judge = normalize_harness(
                args.get("judge")
                    .and_then(Json::as_str)
                    .unwrap_or(fuse::DEFAULT_JUDGE),
            );
            let judge_model = args
                .get("judge_model")
                .and_then(Json::as_str)
                .map(str::to_string);

            // Fan the prompt out to the panel in parallel, then have the judge
            // (Claude by default) synthesize one answer from the replies.
            let (answers, skipped) = fuse::split_replies(fuse::run_panel(
                prompt,
                &panel,
                context,
                &cwd,
                max_context,
                true,
            ));
            if answers.len() < 2 {
                return Ok(text_content(
                    &fuse::insufficient_panel_message(answers.len(), &skipped),
                    true,
                ));
            }
            let note = if skipped.is_empty() {
                String::new()
            } else {
                format!("(skipped: {})\n\n", skipped.join(", "))
            };
            match fuse::run_judge(prompt, &answers, &judge, judge_model, &cwd, max_context) {
                Ok(fused) => Ok(text_content(&format!("{note}{fused}"), false)),
                Err(e) => Ok(text_content(&format!("judge {judge} failed: {e}"), true)),
            }
        }
        other => Err((-32602, format!("unknown tool: {other}"))),
    }
}

fn arg_cwd(args: &Json, default_cwd: &Path) -> PathBuf {
    args.get("cwd")
        .and_then(Json::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_cwd.to_path_buf())
}

// ---- JSON-RPC envelope + MCP content helpers -------------------------------

fn success_response(id: &Json, result: Json) -> Json {
    obj(vec![
        ("jsonrpc", Json::Str("2.0".to_string())),
        ("id", id.clone()),
        ("result", result),
    ])
}

fn error_response(id: &Json, code: i64, message: &str) -> Json {
    obj(vec![
        ("jsonrpc", Json::Str("2.0".to_string())),
        ("id", id.clone()),
        (
            "error",
            obj(vec![
                ("code", Json::Number(code as f64)),
                ("message", Json::Str(message.to_string())),
            ]),
        ),
    ])
}

/// An MCP `tools/call` result: a single text content block.
fn text_content(text: &str, is_error: bool) -> Json {
    obj(vec![
        (
            "content",
            Json::Array(vec![obj(vec![
                ("type", Json::Str("text".to_string())),
                ("text", Json::Str(text.to_string())),
            ])]),
        ),
        ("isError", Json::Bool(is_error)),
    ])
}

fn tool(name: &str, description: &str, properties: Json, required: Vec<&str>) -> Json {
    let schema = obj(vec![
        ("type", Json::Str("object".to_string())),
        ("properties", properties),
        (
            "required",
            Json::Array(
                required
                    .into_iter()
                    .map(|r| Json::Str(r.to_string()))
                    .collect(),
            ),
        ),
    ]);
    obj(vec![
        ("name", Json::Str(name.to_string())),
        ("description", Json::Str(description.to_string())),
        ("inputSchema", schema),
    ])
}

fn obj(pairs: Vec<(&str, Json)>) -> Json {
    let mut map = BTreeMap::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Json::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cwd() -> PathBuf {
        PathBuf::from("/tmp/nonexistent-par-test-dir")
    }

    #[test]
    fn notification_gets_no_reply() {
        let req = Json::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(handle_request(&req, &cwd()).is_none());
    }

    #[test]
    fn initialize_reports_server_and_tools_capability() {
        let req =
            Json::parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#).unwrap();
        let resp = handle_request(&req, &cwd()).unwrap();
        let result = resp.get("result").unwrap();
        assert_eq!(
            result.get("protocolVersion").and_then(Json::as_str),
            Some(PROTOCOL_VERSION)
        );
        assert_eq!(
            result
                .get("serverInfo")
                .and_then(|s| s.get("name"))
                .and_then(Json::as_str),
            Some("parley")
        );
        assert!(result
            .get("capabilities")
            .and_then(|c| c.get("tools"))
            .is_some());
    }

    #[test]
    fn tools_list_exposes_all_tools() {
        let req =
            Json::parse(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#).unwrap();
        let resp = handle_request(&req, &cwd()).unwrap();
        let tools = resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(Json::as_array)
            .unwrap();
        assert_eq!(tools.len(), 5);
        let names: Vec<_> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Json::as_str))
            .collect();
        assert!(names.contains(&"list_sessions"));
        assert!(names.contains(&"get_last_session"));
        assert!(names.contains(&"resume_command"));
        assert!(names.contains(&"ask_agent"));
        assert!(names.contains(&"fuse"));
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let req = Json::parse(r#"{"jsonrpc":"2.0","id":3,"method":"bogus"}"#).unwrap();
        let resp = handle_request(&req, &cwd()).unwrap();
        assert_eq!(
            resp.get("error")
                .and_then(|e| e.get("code"))
                .and_then(Json::as_number),
            Some(-32601.0)
        );
    }

    #[test]
    fn resume_command_builds_native_command() {
        let req = Json::parse(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"resume_command","arguments":{"harness":"claude","id":"abc-123"}}}"#,
        )
        .unwrap();
        let resp = handle_request(&req, &cwd()).unwrap();
        let text = resp
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(Json::as_array)
            .and_then(|c| c.first())
            .and_then(|b| b.get("text"))
            .and_then(Json::as_str)
            .unwrap();
        assert_eq!(text, "claude --resume abc-123");
    }

    #[test]
    fn list_sessions_returns_array_text() {
        let req = Json::parse(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_sessions","arguments":{}}}"#,
        )
        .unwrap();
        let resp = handle_request(&req, &cwd()).unwrap();
        let is_error = resp
            .get("result")
            .and_then(|r| r.get("isError"))
            .and_then(Json::as_bool)
            .unwrap();
        assert!(!is_error);
    }
}
