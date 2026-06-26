use crate::convert::{ConvertOptions, ConvertTarget};
use crate::harness::{ShimCommand, ShimOptions};
use crate::installer::{InstallOptions, InstallTarget, UpdateOptions, UpdateTarget};
use crate::EnvDefaults;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CliOptions {
    pub harness: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub output_format: Option<String>,
    pub input_format: Option<String>,
    pub permission_mode: Option<String>,
    pub max_turns: Option<String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub prompt: Option<String>,
    pub passthrough: Vec<String>,
    pub dry_run: bool,
    pub yolo: bool,
}

/// Options for `par resume` — browse and resume sessions across harnesses,
/// scoped to a working directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResumeOptions {
    /// Restrict to one harness (shorthands allowed, e.g. `k` → kimi, `cl` → claude).
    pub harness: Option<String>,
    /// A 1-based list index or a raw session id.
    pub selector: Option<String>,
    pub cwd: Option<String>,
    /// Resume the newest match without prompting.
    pub latest: bool,
    /// Print the listing and exit (no resume).
    pub list: bool,
    /// With `--list`, emit JSON instead of a human table.
    pub json: bool,
    /// Print the resolved resume command instead of running it.
    pub print: bool,
    /// Append the harness's permission-bypass flag to the resume command.
    pub yolo: bool,
}

/// Options for `par ask` — run one agent headless and capture its reply,
/// optionally seeded with another agent's session transcript.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AskOptions {
    pub harness: Option<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub cwd: Option<String>,
    pub yolo: bool,
    /// `harness[:session]` of a transcript to inject as context.
    pub context_from: Option<String>,
    pub max_context_chars: Option<usize>,
    pub dry_run: bool,
}

/// Options for `par converse` — two agents take turns in one conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConverseOptions {
    pub a: Option<String>,
    pub b: Option<String>,
    pub a_model: Option<String>,
    pub b_model: Option<String>,
    pub topic: Option<String>,
    pub turns: usize,
    /// `harness[:session]` of a transcript to seed the first turn.
    pub context_from: Option<String>,
    /// Stop early when a reply contains this phrase.
    pub until: Option<String>,
    pub max_context_chars: Option<usize>,
    pub cwd: Option<String>,
    pub yolo: bool,
    pub dry_run: bool,
}

/// Options for `par fuse` — ask a panel of agents the same prompt in parallel,
/// then have a judge agent synthesize one answer from their replies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FuseOptions {
    pub prompt: Option<String>,
    /// Panel agents (short codes allowed). Empty => the default trio.
    pub panel: Vec<String>,
    /// Judge agent that synthesizes the panel. None => claude.
    pub judge: Option<String>,
    pub judge_model: Option<String>,
    /// `harness[:session]` of a transcript to seed every panelist.
    pub context_from: Option<String>,
    pub max_context_chars: Option<usize>,
    pub cwd: Option<String>,
    pub yolo: bool,
    pub dry_run: bool,
}

/// What a `par mcp ...` invocation should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum McpMode {
    /// Run the stdio MCP server (plain `par mcp`).
    Serve,
    /// Register `par mcp` into a harness (`par mcp connect -h <agent>`).
    Connect,
    /// Re-register after an `off` (`par mcp on -h <agent>`).
    On,
    /// Unregister `par mcp` from a harness (`par mcp off -h <agent>`).
    Off,
    /// Report whether `par mcp` is registered (`par mcp status -h <agent>`).
    Status,
}

/// Options for `par mcp`. With no subcommand it runs the stdio server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct McpOptions {
    pub mode: McpMode,
    pub harness: Option<String>,
    pub dry_run: bool,
}

impl Default for McpOptions {
    fn default() -> Self {
        Self {
            mode: McpMode::Serve,
            harness: None,
            dry_run: false,
        }
    }
}

/// Options for `par route` — explain which agent a prompt would route to.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RouteOptions {
    pub prompt: String,
    pub bias: Option<f64>,
    /// How many agents to suggest for a `fuse` panel.
    pub panel_size: usize,
    pub json: bool,
}

/// Options for `par solve` — route to one agent, escalate to a panel on failure.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SolveOptions {
    pub prompt: Option<String>,
    /// Explicit first agent; `None`/`auto` => auto-route.
    pub harness: Option<String>,
    /// Explicit escalation panel; empty => auto-select a diverse one.
    pub panel: Vec<String>,
    pub judge: Option<String>,
    pub judge_model: Option<String>,
    pub bias: Option<f64>,
    /// Detect-only: report what would escalate, take no action.
    pub shadow: bool,
    /// Force the "no file change = no progress" check on/off; default = on for
    /// code-shaped tasks.
    pub expect_changes: Option<bool>,
    pub cwd: Option<String>,
    pub yolo: bool,
    pub dry_run: bool,
}

/// Options for `par stats` — print the telemetry scoreboard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatsOptions {
    pub json: bool,
}

/// Options for `par rate` — attach feedback to the most recent run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RateOptions {
    /// true = 👍, false = 👎.
    pub sign: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandsCommand {
    Install,
    List,
}

/// Options for `par commands` — generate slash commands into an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandsOptions {
    pub command: CommandsCommand,
    pub agent: Option<String>,
    pub dir: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CliAction {
    Help,
    Version,
    Default(DefaultCommand),
    Shims(ShimOptions),
    Install(InstallOptions),
    Update(UpdateOptions),
    Convert(ConvertOptions),
    Resume(ResumeOptions),
    Ask(AskOptions),
    Converse(ConverseOptions),
    Fuse(FuseOptions),
    Solve(Box<SolveOptions>),
    Route(RouteOptions),
    Stats(StatsOptions),
    Rate(RateOptions),
    Statusline,
    Commands(CommandsOptions),
    Mcp(McpOptions),
    Run(Box<CliOptions>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DefaultCommand {
    Show,
    Path,
    List,
    Set {
        harness: Option<String>,
        provider: Option<String>,
        model: Option<String>,
        yolo: Option<bool>,
    },
}

pub(crate) fn parse_args<I>(argv: I, defaults: EnvDefaults) -> Result<CliAction, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = argv.into_iter().peekable();

    match args.peek().map(String::as_str) {
        Some("install") => {
            args.next();
            return parse_install_args(args);
        }
        Some("update" | "upgrade") => {
            args.next();
            return parse_update_args(args);
        }
        Some("default" | "use") => {
            args.next();
            return parse_default_args(args);
        }
        Some("current") => {
            args.next();
            return Ok(CliAction::Default(DefaultCommand::Show));
        }
        Some("list" | "ls") => {
            args.next();
            return Ok(CliAction::Default(DefaultCommand::List));
        }
        Some("shims") => {
            args.next();
            return parse_shim_args(args);
        }
        Some("convert") => {
            args.next();
            return parse_convert_args(args);
        }
        Some("resume") => {
            args.next();
            return parse_resume_args(args);
        }
        Some("ask") => {
            args.next();
            return parse_ask_args(args);
        }
        Some("converse" | "debate" | "relay") => {
            args.next();
            return parse_converse_args(args);
        }
        Some("fuse" | "panel") => {
            args.next();
            return parse_fuse_args(args);
        }
        Some("solve") => {
            args.next();
            return parse_solve_args(args);
        }
        Some("route") => {
            args.next();
            return parse_route_args(args);
        }
        Some("stats" | "gain") => {
            args.next();
            return parse_stats_args(args);
        }
        Some("rate") => {
            args.next();
            return parse_rate_args(args);
        }
        Some("statusline") => {
            return Ok(CliAction::Statusline);
        }
        Some("commands") => {
            args.next();
            return parse_commands_args(args);
        }
        Some("mcp") => {
            args.next();
            return parse_mcp_args(args);
        }
        _ => {}
    }

    let mut options = CliOptions {
        harness: defaults.harness.unwrap_or_else(|| "claude".to_string()),
        provider: defaults.provider,
        model: defaults.model,
        // Yolo is on by default — pass --no-yolo to opt out for a single run, or
        // set PARLEY_YOLO=false to opt out persistently.
        yolo: defaults.yolo.unwrap_or(true),
        ..CliOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            // `-h <name>` is a harness shorthand (e.g. `-h cl`); bare `-h` is help.
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => {
                    options.harness = args.next().unwrap();
                }
                _ => return Ok(CliAction::Help),
            },
            "--version" | "-v" => return Ok(CliAction::Version),
            "--dry-run" => options.dry_run = true,
            "--yolo" => options.yolo = true,
            "--no-yolo" => options.yolo = false,
            "--" => {
                options.passthrough.extend(args);
                break;
            }
            "--print" => {}
            "-p" => {
                if let Some(next) = args.peek() {
                    if !next.starts_with('-') {
                        options.prompt = args.next();
                    }
                }
            }
            "--prompt" => options.prompt = Some(require_value(&mut args, "--prompt")?),
            "--harness" => options.harness = require_value(&mut args, "--harness")?,
            "--provider" => options.provider = Some(require_value(&mut args, "--provider")?),
            "--model" | "-m" => options.model = Some(require_value(&mut args, "--model")?),
            "--cwd" => options.cwd = Some(require_value(&mut args, "--cwd")?),
            "--output-format" => {
                options.output_format = Some(require_value(&mut args, "--output-format")?)
            }
            "--input-format" => {
                options.input_format = Some(require_value(&mut args, "--input-format")?)
            }
            "--permission-mode" => {
                options.permission_mode = Some(require_value(&mut args, "--permission-mode")?)
            }
            "--max-turns" => options.max_turns = Some(require_value(&mut args, "--max-turns")?),
            "--agent" => options.agent = Some(require_value(&mut args, "--agent")?),
            _ if arg.starts_with("--prompt=") => {
                options.prompt = Some(value_after_equals(&arg, "--prompt="));
            }
            _ if arg.starts_with("--harness=") => {
                options.harness = value_after_equals(&arg, "--harness=");
            }
            _ if arg.starts_with("--provider=") => {
                options.provider = Some(value_after_equals(&arg, "--provider="));
            }
            _ if arg.starts_with("--model=") => {
                options.model = Some(value_after_equals(&arg, "--model="));
            }
            _ if arg.starts_with("--cwd=") => {
                options.cwd = Some(value_after_equals(&arg, "--cwd="));
            }
            _ if arg.starts_with("--output-format=") => {
                options.output_format = Some(value_after_equals(&arg, "--output-format="));
            }
            _ if arg.starts_with("--input-format=") => {
                options.input_format = Some(value_after_equals(&arg, "--input-format="));
            }
            _ if arg.starts_with("--permission-mode=") => {
                options.permission_mode = Some(value_after_equals(&arg, "--permission-mode="));
            }
            _ if arg.starts_with("--max-turns=") => {
                options.max_turns = Some(value_after_equals(&arg, "--max-turns="));
            }
            _ if arg.starts_with("--agent=") => {
                options.agent = Some(value_after_equals(&arg, "--agent="));
            }
            _ if arg.starts_with('-') => options.passthrough.push(arg),
            _ => {
                options.prompt = Some(match options.prompt {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    Ok(CliAction::Run(Box::new(options)))
}

pub(crate) fn usage() -> &'static str {
    "Usage:
  par
  par -p \"prompt\" --harness codex --model gpt-5.4
  par --harness opencode --provider anthropic --model claude-sonnet-4-6 \"fix the failing tests\"
  cat README.md | par -p \"summarize this\" --harness gemini --output-format json
  par default codex --yolo
  par current
  par shims install
  par install claude
  par install list

Options:
  --harness, -h <name>    claude, codex, cursor, gemini, goose, opencode, qwen, aider, amazon-q, copilot, kimi, antigravity
                          Shorthands: cl=claude co=codex cu=cursor g=gemini go=goose
                          oc=opencode q=qwen k=kimi a/ai=aider aq=amazon-q cp=copilot ag=antigravity
                          Meta-harnesses (call back into par, compose anywhere a harness is taken):
                          auto = route to the best agent · fuse = run a panel · solve = route+escalate
                          e.g. par fuse --panel auto,auto · par converse --a fuse --b claude
  --provider <name>       Provider namespace when the target CLI supports one
  --model, -m <name>      Model name to pass through
  --agent <name>          Agent/persona name for harnesses that support it
  --output-format <fmt>   text, json, stream-json when supported by target
  --cwd <path>            Working directory for the target CLI
  --no-yolo               Yolo (permission bypass) is ON by default; this opts out for the run
  --yolo                  Explicitly enable yolo (already the default)
  --dry-run               Print the routed command as JSON
  --                      Pass remaining flags through to the target CLI

  Note: -h <name> selects a harness; bare -h (no value) prints this help.
  Examples: par -h cl \"review\"   par -h co -m gpt-5.4 \"fix tests\"   par -h k

Defaults:
  default [name]          Show or set the persisted default harness
  default <name> --yolo   Set default harness and enable yolo by default
  default --no-yolo       Disable yolo in the persisted default
  current                 Show persisted defaults
  list                    List supported harness names

Shims:
  shims install           Write *y shortcut scripts such as claudey and codexy
  shims install --dir <d> Write shims to a specific directory
  shims list              Print generated shim names and commands

Install:
  install <name>          Install one supported downstream harness CLI
  install list            List supported harness installers
  install all             Run every supported installer
  install --dry-run <name> Print installer commands without running them

Update:
  update                  Update par itself
  update self             Update par itself (explicit)
  update <name>           Update a specific harness CLI
  update all              Update par and all harness CLIs
  update --dry-run <name> Print update commands without running them

Convert:
  convert                         Auto-detect source, convert to all targets
  convert --to gemini             Convert to a specific target
  convert --to all                Convert to all targets (default)
  convert --from claude --to codex Explicit source and target
  convert --dry-run               Show what files would be created
  convert --cwd <path>            Run in a different directory

  Supported sources: claude
  Supported targets: gemini, codex, antigravity, opencode, cursor, kimi

Resume:
  par resume                      List sessions for this folder (any agent), pick one to resume
  par resume -h cl                Resume a claude session in this folder (picker if several)
  par resume -h co --latest       Resume the newest codex session, no prompt
  par resume --list               Print resumable sessions, do not resume
  par resume --list --json        Machine-readable listing
  par resume -h cl <id> --print   Print the resume command for a session id
  par resume --cwd <path>         Scope to another directory

  Native listing: claude, codex, opencode. Delegate resume (best-effort listing,
  marked ~): cursor, gemini — resume runs the native CLI's own cwd-scoped resume.

Ask (agent-to-agent):
  par ask -h g -p \"...\"           Run another agent headless, print its reply
  par ask -h g -p \"...\" --context-from cl
                                  Seed the call with your latest claude session here
  par ask -h cl --context-from co:<id> -p \"...\"
                                  Use a specific source session id as context
  par ask -h g -p \"...\" --max-context 8000 --dry-run
                                  Cap injected context; show the command, run nothing
  Context sources: claude, codex, opencode (cursor/gemini cannot export transcripts)

Converse (multi-turn, two agents):
  par converse --a cl --b g -p \"<task>\"    Two agents take turns (default 6 turns)
  par converse --a cl --b g -p \"...\" --turns 8 --until DONE
                                  Run up to 8 turns, stop when a reply says DONE
  par converse --a cl --b g -p \"...\" --context-from co
                                  Seed turn 1 with your latest codex session here
  --a-model / --b-model <m>       Per-agent model;  also: --max-context, --cwd, --dry-run
  (aliases: par debate, par relay)

Fuse (panel + judge — make answers smarter):
  par fuse -p \"<task>\"            Ask a panel in parallel; a judge synthesizes one answer
  par fuse \"<task>\" --panel cl,co,g
                                  Pick the panel (default: claude,codex,gemini; needs >=2)
  par fuse \"...\" --judge co --judge-model gpt-5.4
                                  Choose the judge agent/model (default judge: claude)
  par fuse \"...\" --context-from cl
                                  Seed every panelist with your latest claude session here
  --max-context, --cwd, --no-yolo, --dry-run as in ask   (alias: par panel)
  par fuse --panel auto -p \"...\"   Let the router pick a diverse panel for the prompt

Route (which agent should answer this?):
  par route \"<task>\"              Show the best agent for a prompt, why, and a suggested panel
  par route \"...\" --bias 1.0      0 = cheapest/fastest, 1 = strongest (default 0.7)
  par route \"...\" --json          Machine-readable decision + candidate scores
  par -h auto -p \"<task>\"         Route, then run the chosen agent (also: PARLEY_HARNESS=auto)

Solve (route, then auto-escalate to a panel if the agent gets stuck):
  par solve \"<task>\"             Run the routed agent; on a stuck/failed/empty reply, convene a panel
  par solve \"...\" -h co          Force the first agent; escalate to others on failure
  par solve \"...\" --panel cl,g   Force the escalation panel;  --judge <agent>
  par solve \"...\" --shadow       Detect-only: report what would escalate, take no action
  --bias, --no-yolo, --cwd, --dry-run as elsewhere

Stats & feedback (local, learns better routing over time):
  par stats                       Per-(task, agent) scoreboard from local telemetry  (alias: par gain)
  par stats --json                Machine-readable scoreboard
  par rate + [note]               Thumbs-up the last run;  par rate - [note] for thumbs-down

Slash commands & statusline (for the agents par drives):
  par commands install -h cl      Write /fuse, /solve, /route slash commands into .claude/commands
  par commands install -h co      ...or into .codex/prompts   (--dir <path>, --dry-run)
  par statusline                  Status-line badge for Claude Code (reads its stdin JSON)

MCP:
  par mcp                         Run the stdio MCP server (JSON-RPC over stdin/stdout)
                                  Tools: list_sessions, get_last_session, resume_command, ask_agent, fuse
                                  fuse = convene a panel of agents on one prompt; a judge synthesizes
  par mcp connect -h cl           Register this server into a harness (runs its native mcp add)
  par mcp connect -h oc           opencode/others may open their own add TUI
  par mcp connect -h cu           cursor has no add command; merges ~/.cursor/mcp.json
  par mcp status -h cl            Report whether par is registered (on/off)
  par mcp off -h cl               Unregister par;  par mcp on -h cl re-registers
  par mcp connect -h cl --dry-run Show what would run / be written, change nothing
                                  Supported: claude, codex, gemini, opencode, cursor

Environment defaults:
  PARLEY_HARNESS, PARLEY_PROVIDER, PARLEY_MODEL, PARLEY_YOLO
  PARLEY_QUALITY_BIAS   routing dial 0..1 (default 0.7)
  PARLEY_TIMEOUT, PARLEY_IDLE_TIMEOUT   captured-run watchdog seconds (0 = off)
  PARLEY_TELEMETRY=off  disable local telemetry; PARLEY_TELEMETRY_PROMPTS=1 store raw prompts
  (legacy AGENT_ROUTER_* names still work)
"
}

fn parse_update_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut dry_run = false;
    let mut target: Option<UpdateTarget> = None;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--dry-run" => dry_run = true,
            "self" | "par" => target = Some(UpdateTarget::Self_),
            "all" => target = Some(UpdateTarget::All),
            _ if arg.starts_with('-') => return Err(format!("unknown update option: {arg}")),
            _ => target = Some(UpdateTarget::One(arg)),
        }
    }

    Ok(CliAction::Update(UpdateOptions {
        target: target.unwrap_or(UpdateTarget::Self_),
        dry_run,
    }))
}

fn parse_install_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut dry_run = false;
    let mut target: Option<InstallTarget> = None;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--dry-run" => dry_run = true,
            "list" => target = Some(InstallTarget::List),
            "all" => target = Some(InstallTarget::All),
            _ if arg.starts_with('-') => return Err(format!("unknown install option: {arg}")),
            _ => target = Some(InstallTarget::One(arg)),
        }
    }

    Ok(CliAction::Install(InstallOptions {
        target: target.unwrap_or(InstallTarget::List),
        dry_run,
    }))
}

fn parse_default_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut harness = None;
    let mut provider = None;
    let mut model = None;
    let mut yolo = None;
    let mut saw_update = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--path" => return Ok(CliAction::Default(DefaultCommand::Path)),
            "--yolo" => {
                yolo = Some(true);
                saw_update = true;
            }
            "--no-yolo" => {
                yolo = Some(false);
                saw_update = true;
            }
            "--provider" => {
                provider = Some(require_value(&mut args, "--provider")?);
                saw_update = true;
            }
            "--model" | "-m" => {
                model = Some(require_value(&mut args, "--model")?);
                saw_update = true;
            }
            _ if arg.starts_with("--provider=") => {
                provider = Some(value_after_equals(&arg, "--provider="));
                saw_update = true;
            }
            _ if arg.starts_with("--model=") => {
                model = Some(value_after_equals(&arg, "--model="));
                saw_update = true;
            }
            _ if arg.starts_with('-') => return Err(format!("unknown default option: {arg}")),
            _ => {
                harness = Some(arg);
                saw_update = true;
            }
        }
    }

    if saw_update {
        Ok(CliAction::Default(DefaultCommand::Set {
            harness,
            provider,
            model,
            yolo,
        }))
    } else {
        Ok(CliAction::Default(DefaultCommand::Show))
    }
}

fn parse_shim_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let command = match args.next().as_deref() {
        Some("install") => ShimCommand::Install,
        Some("list" | "ls") | None => ShimCommand::List,
        Some("--help" | "-h") => return Ok(CliAction::Help),
        Some(value) => return Err(format!("unknown shims command: {value}")),
    };
    let mut dir = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--dir" => dir = Some(require_value(&mut args, "--dir")?),
            "--dry-run" => dry_run = true,
            _ if arg.starts_with("--dir=") => dir = Some(value_after_equals(&arg, "--dir=")),
            _ => return Err(format!("unknown shims option: {arg}")),
        }
    }

    Ok(CliAction::Shims(ShimOptions {
        command,
        dir,
        dry_run,
    }))
}

fn parse_convert_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut from = None;
    let mut to = None;
    let mut cwd = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--dry-run" => dry_run = true,
            "--from" => from = Some(require_value(&mut args, "--from")?),
            "--to" => to = Some(require_value(&mut args, "--to")?),
            "--cwd" => cwd = Some(require_value(&mut args, "--cwd")?),
            _ if arg.starts_with("--from=") => from = Some(value_after_equals(&arg, "--from=")),
            _ if arg.starts_with("--to=") => to = Some(value_after_equals(&arg, "--to=")),
            _ if arg.starts_with("--cwd=") => cwd = Some(value_after_equals(&arg, "--cwd=")),
            _ if arg.starts_with('-') => return Err(format!("unknown convert option: {arg}")),
            _ => {
                if to.is_none() {
                    to = Some(arg);
                } else {
                    return Err(format!("unexpected argument: {arg}"));
                }
            }
        }
    }

    let target = match to.as_deref() {
        None | Some("all") => ConvertTarget::All,
        Some(name) => ConvertTarget::One(name.to_string()),
    };

    Ok(CliAction::Convert(ConvertOptions {
        from,
        to: target,
        cwd,
        dry_run,
    }))
}

fn parse_resume_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut options = ResumeOptions::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            // `-h <name>` selects a harness filter (mirrors the run path);
            // bare `-h` prints help.
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => options.harness = args.next(),
                _ => return Ok(CliAction::Help),
            },
            "--harness" => options.harness = Some(require_value(&mut args, "--harness")?),
            "--cwd" => options.cwd = Some(require_value(&mut args, "--cwd")?),
            "--latest" | "--last" => options.latest = true,
            "--list" | "--ls" => options.list = true,
            "--json" => options.json = true,
            "--print" => options.print = true,
            "--yolo" => options.yolo = true,
            "--no-yolo" => options.yolo = false,
            _ if arg.starts_with("--harness=") => {
                options.harness = Some(value_after_equals(&arg, "--harness="))
            }
            _ if arg.starts_with("--cwd=") => {
                options.cwd = Some(value_after_equals(&arg, "--cwd="))
            }
            _ if arg.starts_with('-') => return Err(format!("unknown resume option: {arg}")),
            _ => {
                if options.selector.is_none() {
                    options.selector = Some(arg);
                } else {
                    return Err(format!("unexpected argument: {arg}"));
                }
            }
        }
    }

    Ok(CliAction::Resume(options))
}

fn parse_ask_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    // Yolo defaults on: a captured headless call must not block on a prompt.
    let mut options = AskOptions {
        yolo: true,
        ..AskOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => options.harness = args.next(),
                _ => return Ok(CliAction::Help),
            },
            "--harness" => options.harness = Some(require_value(&mut args, "--harness")?),
            "-p" | "--prompt" => options.prompt = Some(require_value(&mut args, "--prompt")?),
            "-m" | "--model" => options.model = Some(require_value(&mut args, "--model")?),
            "--provider" => options.provider = Some(require_value(&mut args, "--provider")?),
            "--cwd" => options.cwd = Some(require_value(&mut args, "--cwd")?),
            "--context-from" | "--context" => {
                options.context_from = Some(require_value(&mut args, "--context-from")?)
            }
            "--max-context" => {
                let raw = require_value(&mut args, "--max-context")?;
                options.max_context_chars = Some(
                    raw.parse()
                        .map_err(|_| format!("--max-context must be a number, got {raw}"))?,
                );
            }
            "--yolo" => options.yolo = true,
            "--no-yolo" => options.yolo = false,
            "--dry-run" => options.dry_run = true,
            _ if arg.starts_with("--harness=") => {
                options.harness = Some(value_after_equals(&arg, "--harness="))
            }
            _ if arg.starts_with("--context-from=") => {
                options.context_from = Some(value_after_equals(&arg, "--context-from="))
            }
            _ if arg.starts_with("--cwd=") => {
                options.cwd = Some(value_after_equals(&arg, "--cwd="))
            }
            _ if arg.starts_with('-') => return Err(format!("unknown ask option: {arg}")),
            // Bare text accumulates into the prompt.
            _ => {
                options.prompt = Some(match options.prompt {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    if options.harness.is_none() {
        return Err("ask requires a target agent: par ask -h <agent> -p \"<prompt>\"".to_string());
    }
    if options.prompt.is_none() {
        return Err("ask requires a prompt: par ask -h <agent> -p \"<prompt>\"".to_string());
    }
    Ok(CliAction::Ask(options))
}

fn parse_converse_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut options = ConverseOptions {
        turns: 6,
        yolo: true,
        ..ConverseOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "--a" | "-a" => options.a = Some(require_value(&mut args, "--a")?),
            "--b" | "-b" => options.b = Some(require_value(&mut args, "--b")?),
            "--a-model" => options.a_model = Some(require_value(&mut args, "--a-model")?),
            "--b-model" => options.b_model = Some(require_value(&mut args, "--b-model")?),
            "-p" | "--prompt" | "--topic" => {
                options.topic = Some(require_value(&mut args, "--prompt")?)
            }
            "--turns" => {
                let raw = require_value(&mut args, "--turns")?;
                options.turns = raw
                    .parse()
                    .map_err(|_| format!("--turns must be a number, got {raw}"))?;
            }
            "--context-from" | "--context" => {
                options.context_from = Some(require_value(&mut args, "--context-from")?)
            }
            "--until" => options.until = Some(require_value(&mut args, "--until")?),
            "--max-context" => {
                let raw = require_value(&mut args, "--max-context")?;
                options.max_context_chars = Some(
                    raw.parse()
                        .map_err(|_| format!("--max-context must be a number, got {raw}"))?,
                );
            }
            "--cwd" => options.cwd = Some(require_value(&mut args, "--cwd")?),
            "--yolo" => options.yolo = true,
            "--no-yolo" => options.yolo = false,
            "--dry-run" => options.dry_run = true,
            _ if arg.starts_with("--a=") => options.a = Some(value_after_equals(&arg, "--a=")),
            _ if arg.starts_with("--b=") => options.b = Some(value_after_equals(&arg, "--b=")),
            _ if arg.starts_with("--context-from=") => {
                options.context_from = Some(value_after_equals(&arg, "--context-from="))
            }
            _ if arg.starts_with("--cwd=") => {
                options.cwd = Some(value_after_equals(&arg, "--cwd="))
            }
            _ if arg.starts_with('-') => return Err(format!("unknown converse option: {arg}")),
            _ => {
                options.topic = Some(match options.topic {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    if options.a.is_none() || options.b.is_none() {
        return Err(
            "converse requires two agents: par converse --a <agent> --b <agent> -p \"<task>\""
                .to_string(),
        );
    }
    if options.topic.is_none() {
        return Err("converse requires a topic: -p \"<task>\"".to_string());
    }
    Ok(CliAction::Converse(options))
}

fn parse_fuse_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    // Yolo defaults on: captured headless calls must not block on a prompt.
    let mut options = FuseOptions {
        yolo: true,
        ..FuseOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "-p" | "--prompt" => options.prompt = Some(require_value(&mut args, "--prompt")?),
            "--panel" => options.panel = parse_panel(&require_value(&mut args, "--panel")?),
            "--judge" => options.judge = Some(require_value(&mut args, "--judge")?),
            "--judge-model" => {
                options.judge_model = Some(require_value(&mut args, "--judge-model")?)
            }
            "--context-from" | "--context" => {
                options.context_from = Some(require_value(&mut args, "--context-from")?)
            }
            "--max-context" => {
                let raw = require_value(&mut args, "--max-context")?;
                options.max_context_chars = Some(
                    raw.parse()
                        .map_err(|_| format!("--max-context must be a number, got {raw}"))?,
                );
            }
            "--cwd" => options.cwd = Some(require_value(&mut args, "--cwd")?),
            "--yolo" => options.yolo = true,
            "--no-yolo" => options.yolo = false,
            "--dry-run" => options.dry_run = true,
            _ if arg.starts_with("--panel=") => {
                options.panel = parse_panel(&value_after_equals(&arg, "--panel="))
            }
            _ if arg.starts_with("--judge=") => {
                options.judge = Some(value_after_equals(&arg, "--judge="))
            }
            _ if arg.starts_with("--context-from=") => {
                options.context_from = Some(value_after_equals(&arg, "--context-from="))
            }
            _ if arg.starts_with("--cwd=") => {
                options.cwd = Some(value_after_equals(&arg, "--cwd="))
            }
            _ if arg.starts_with('-') => return Err(format!("unknown fuse option: {arg}")),
            // Bare text accumulates into the prompt.
            _ => {
                options.prompt = Some(match options.prompt {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    if options.prompt.is_none() {
        return Err("fuse requires a prompt: par fuse -p \"<task>\" --panel cl,co,g".to_string());
    }
    Ok(CliAction::Fuse(options))
}

/// Split a comma-separated `--panel` value into agent codes, dropping blanks.
fn parse_panel(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_mcp_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    match args.peek().map(String::as_str) {
        Some("connect") => {
            args.next();
            return parse_mcp_sub(args, McpMode::Connect);
        }
        Some("on") => {
            args.next();
            return parse_mcp_sub(args, McpMode::On);
        }
        Some("off") => {
            args.next();
            return parse_mcp_sub(args, McpMode::Off);
        }
        Some("status") => {
            args.next();
            return parse_mcp_sub(args, McpMode::Status);
        }
        _ => {}
    }

    // Plain `par mcp` — run the server.
    let rest: Vec<String> = args.collect();
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(CliAction::Help);
    }
    if let Some(arg) = rest.first() {
        return Err(format!("unknown mcp option: {arg}"));
    }
    Ok(CliAction::Mcp(McpOptions::default()))
}

fn parse_mcp_sub<I>(args: std::iter::Peekable<I>, mode: McpMode) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut harness = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => harness = args.next(),
                _ => return Ok(CliAction::Help),
            },
            "--harness" => harness = Some(require_value(&mut args, "--harness")?),
            "--dry-run" => dry_run = true,
            _ if arg.starts_with("--harness=") => {
                harness = Some(value_after_equals(&arg, "--harness="))
            }
            _ if arg.starts_with('-') => return Err(format!("unknown mcp option: {arg}")),
            // Bare harness name, e.g. `par mcp connect claude`.
            _ if harness.is_none() => harness = Some(arg),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(CliAction::Mcp(McpOptions {
        mode,
        harness,
        dry_run,
    }))
}

fn parse_solve_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut o = SolveOptions {
        yolo: true,
        ..SolveOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => o.harness = args.next(),
                _ => return Ok(CliAction::Help),
            },
            "--harness" => o.harness = Some(require_value(&mut args, "--harness")?),
            "-p" | "--prompt" => o.prompt = Some(require_value(&mut args, "--prompt")?),
            "--panel" => o.panel = parse_panel(&require_value(&mut args, "--panel")?),
            "--judge" => o.judge = Some(require_value(&mut args, "--judge")?),
            "--judge-model" => o.judge_model = Some(require_value(&mut args, "--judge-model")?),
            "--bias" => o.bias = Some(parse_bias(&require_value(&mut args, "--bias")?)?),
            "--shadow" => o.shadow = true,
            "--expect-changes" => o.expect_changes = Some(true),
            "--no-expect-changes" => o.expect_changes = Some(false),
            "--cwd" => o.cwd = Some(require_value(&mut args, "--cwd")?),
            "--yolo" => o.yolo = true,
            "--no-yolo" => o.yolo = false,
            "--dry-run" => o.dry_run = true,
            _ if arg.starts_with("--harness=") => {
                o.harness = Some(value_after_equals(&arg, "--harness="))
            }
            _ if arg.starts_with("--panel=") => {
                o.panel = parse_panel(&value_after_equals(&arg, "--panel="))
            }
            _ if arg.starts_with("--cwd=") => o.cwd = Some(value_after_equals(&arg, "--cwd=")),
            _ if arg.starts_with('-') => return Err(format!("unknown solve option: {arg}")),
            _ => {
                o.prompt = Some(match o.prompt {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    if o.prompt.is_none() {
        return Err("solve requires a prompt: par solve \"<task>\"".to_string());
    }
    Ok(CliAction::Solve(Box::new(o)))
}

fn parse_route_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let mut o = RouteOptions {
        panel_size: 3,
        ..RouteOptions::default()
    };
    let mut prompt: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "-p" | "--prompt" => prompt = Some(require_value(&mut args, "--prompt")?),
            "--bias" => o.bias = Some(parse_bias(&require_value(&mut args, "--bias")?)?),
            "--panel" => {
                let raw = require_value(&mut args, "--panel")?;
                o.panel_size = raw
                    .parse()
                    .map_err(|_| format!("--panel must be a number, got {raw}"))?;
            }
            "--json" => o.json = true,
            _ if arg.starts_with('-') => return Err(format!("unknown route option: {arg}")),
            _ => {
                prompt = Some(match prompt {
                    Some(existing) => format!("{existing} {arg}"),
                    None => arg,
                });
            }
        }
    }

    o.prompt = prompt.ok_or("route requires a prompt: par route \"<task>\"")?;
    Ok(CliAction::Route(o))
}

fn parse_stats_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut o = StatsOptions::default();
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--json" => o.json = true,
            _ => return Err(format!("unknown stats option: {arg}")),
        }
    }
    Ok(CliAction::Stats(o))
}

fn parse_rate_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let first = args
        .next()
        .ok_or("rate needs a sign: par rate + [note]  /  par rate - [note]")?;
    if first == "--help" {
        return Ok(CliAction::Help);
    }
    let sign = match first.to_ascii_lowercase().as_str() {
        "+" | "up" | "good" | "yes" | "y" => true,
        "-" | "down" | "bad" | "no" | "n" => false,
        other => return Err(format!("rate sign must be + or -, got {other}")),
    };
    let note: Vec<String> = args.collect();
    let note = if note.is_empty() {
        None
    } else {
        Some(note.join(" "))
    };
    Ok(CliAction::Rate(RateOptions { sign, note }))
}

fn parse_commands_args<I>(args: std::iter::Peekable<I>) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let mut args = args;
    let command = match args.peek().map(String::as_str) {
        Some("install") => {
            args.next();
            CommandsCommand::Install
        }
        Some("list" | "ls") => {
            args.next();
            CommandsCommand::List
        }
        Some("--help") => return Ok(CliAction::Help),
        None => CommandsCommand::List,
        _ => CommandsCommand::Install,
    };
    let mut agent = None;
    let mut dir = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Ok(CliAction::Help),
            "-h" => match args.peek() {
                Some(next) if !next.starts_with('-') => agent = args.next(),
                _ => return Ok(CliAction::Help),
            },
            "--harness" | "--agent" => agent = Some(require_value(&mut args, "--agent")?),
            "--dir" => dir = Some(require_value(&mut args, "--dir")?),
            "--dry-run" => dry_run = true,
            _ if arg.starts_with("--agent=") => agent = Some(value_after_equals(&arg, "--agent=")),
            _ if arg.starts_with("--dir=") => dir = Some(value_after_equals(&arg, "--dir=")),
            _ if arg.starts_with('-') => return Err(format!("unknown commands option: {arg}")),
            _ if agent.is_none() => agent = Some(arg),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(CliAction::Commands(CommandsOptions {
        command,
        agent,
        dir,
        dry_run,
    }))
}

/// Parse a quality/price bias value in [0, 1].
fn parse_bias(raw: &str) -> Result<f64, String> {
    raw.parse::<f64>()
        .map_err(|_| format!("--bias must be a number in [0,1], got {raw}"))
}

fn require_value<I>(args: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    match args.next() {
        Some(value) if !value.starts_with('-') => Ok(value),
        _ => Err(format!("{flag} requires a value")),
    }
}

fn value_after_equals(arg: &str, prefix: &str) -> String {
    arg[prefix.len()..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> EnvDefaults {
        EnvDefaults {
            harness: None,
            provider: None,
            model: None,
            yolo: None,
        }
    }

    #[test]
    fn parses_default_claude_prompt() {
        let action = parse_args(
            ["-p", "hello", "--model", "sonnet"].map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Run(Box::new(CliOptions {
                harness: "claude".to_string(),
                prompt: Some("hello".to_string()),
                model: Some("sonnet".to_string()),
                yolo: true,
                ..CliOptions::default()
            }))
        );
    }

    #[test]
    fn yolo_on_by_default_and_opt_out() {
        let on = parse_args(["-p", "hi"].map(String::from), defaults()).unwrap();
        let CliAction::Run(opts) = on else { panic!() };
        assert!(opts.yolo);

        let off = parse_args(["--no-yolo", "-p", "hi"].map(String::from), defaults()).unwrap();
        let CliAction::Run(opts) = off else { panic!() };
        assert!(!opts.yolo);
    }

    #[test]
    fn dash_h_is_harness_shorthand() {
        let action = parse_args(["-h", "k", "-p", "hi"].map(String::from), defaults()).unwrap();
        let CliAction::Run(opts) = action else {
            panic!("expected run action");
        };
        assert_eq!(opts.harness, "k");
    }

    #[test]
    fn bare_dash_h_is_help() {
        let action = parse_args(["-h"].map(String::from), defaults()).unwrap();
        assert_eq!(action, CliAction::Help);
    }

    #[test]
    fn preserves_passthrough_after_separator() {
        let action = parse_args(
            ["--harness", "codex", "-p", "hello", "--", "--verbose"].map(String::from),
            defaults(),
        )
        .unwrap();

        let CliAction::Run(options) = action else {
            panic!("expected run action");
        };

        assert_eq!(options.passthrough, vec!["--verbose"]);
    }

    #[test]
    fn parses_install_action() {
        let action = parse_args(
            ["install", "--dry-run", "claude"].map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Install(InstallOptions {
                target: InstallTarget::One("claude".to_string()),
                dry_run: true,
            })
        );
    }

    #[test]
    fn parses_default_set_action() {
        let action = parse_args(
            ["default", "codex", "--model", "gpt-5.4", "--yolo"].map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Default(DefaultCommand::Set {
                harness: Some("codex".to_string()),
                provider: None,
                model: Some("gpt-5.4".to_string()),
                yolo: Some(true),
            })
        );
    }

    #[test]
    fn parses_convert_all_default() {
        let action = parse_args(["convert"].map(String::from), defaults()).unwrap();

        assert_eq!(
            action,
            CliAction::Convert(ConvertOptions {
                from: None,
                to: ConvertTarget::All,
                cwd: None,
                dry_run: false,
            })
        );
    }

    #[test]
    fn parses_convert_with_flags() {
        let action = parse_args(
            ["convert", "--from", "claude", "--to", "gemini", "--dry-run"].map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Convert(ConvertOptions {
                from: Some("claude".to_string()),
                to: ConvertTarget::One("gemini".to_string()),
                cwd: None,
                dry_run: true,
            })
        );
    }

    #[test]
    fn parses_convert_positional_target() {
        let action = parse_args(["convert", "opencode"].map(String::from), defaults()).unwrap();

        assert_eq!(
            action,
            CliAction::Convert(ConvertOptions {
                from: None,
                to: ConvertTarget::One("opencode".to_string()),
                cwd: None,
                dry_run: false,
            })
        );
    }

    #[test]
    fn parses_fuse_action() {
        let action = parse_args(
            [
                "fuse",
                "--panel",
                "cl, co , g",
                "--judge",
                "co",
                "-p",
                "design X",
            ]
            .map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Fuse(FuseOptions {
                prompt: Some("design X".to_string()),
                panel: vec!["cl".to_string(), "co".to_string(), "g".to_string()],
                judge: Some("co".to_string()),
                yolo: true,
                ..FuseOptions::default()
            })
        );
    }

    #[test]
    fn fuse_alias_panel_and_bare_prompt() {
        let action = parse_args(
            ["panel", "design", "a", "limiter"].map(String::from),
            defaults(),
        )
        .unwrap();
        let CliAction::Fuse(opts) = action else {
            panic!("expected fuse action");
        };
        assert_eq!(opts.prompt, Some("design a limiter".to_string()));
        assert!(opts.panel.is_empty());
    }

    #[test]
    fn parses_shims_install_action() {
        let action = parse_args(
            ["shims", "install", "--dir", "/tmp/par-shims"].map(String::from),
            defaults(),
        )
        .unwrap();

        assert_eq!(
            action,
            CliAction::Shims(ShimOptions {
                command: ShimCommand::Install,
                dir: Some("/tmp/par-shims".to_string()),
                dry_run: false,
            })
        );
    }
}
