mod ask;
mod cli;
mod commands;
mod config;
mod converse;
mod convert;
mod fsx;
mod fuse;
mod harness;
mod installer;
mod json;
mod mcp;
mod model;
mod process;
mod route;
mod session;
mod signals;
mod solve;
mod statusline;
mod telemetry;

use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::time::Instant;

use cli::{parse_args, usage, CliAction};
use config::DefaultConfig;
use harness::{normalize_harness, HarnessFactory, Request};
use installer::{run_install, run_update};
use process::{run_invocation, run_invocation_status};

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(io::stderr(), "par: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let action = parse_args(env::args().skip(1), EnvDefaults::load())?;

    match action {
        CliAction::Help => {
            print!("{}", usage());
            Ok(())
        }
        CliAction::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliAction::Install(options) => run_install(options),
        CliAction::Update(options) => run_update(options),
        CliAction::Default(command) => config::run_default_command(command),
        CliAction::Shims(options) => harness::run_shims(options),
        CliAction::Convert(options) => convert::run_convert(options),
        CliAction::Resume(options) => session::run_resume(options),
        CliAction::Ask(options) => ask::run_cli(options),
        CliAction::Converse(options) => converse::run_cli(options),
        CliAction::Fuse(options) => fuse::run_cli(options),
        CliAction::Solve(options) => solve::run_cli(*options),
        CliAction::Route(options) => route::run_cli(
            &options.prompt,
            options.bias,
            options.panel_size,
            options.json,
        ),
        CliAction::Stats(options) => telemetry::run_stats(options.json),
        CliAction::Rate(options) => telemetry::run_rate(options.sign, options.note),
        CliAction::Statusline => statusline::run(),
        CliAction::Commands(options) => commands::run(options),
        CliAction::Mcp(options) => mcp::dispatch(options),
        CliAction::Run(options) => run_prompt(*options),
    }
}

/// The `par -p "..."` path: build the routed invocation, optionally auto-routing
/// the agent, then run it (recording telemetry around the run).
fn run_prompt(options: cli::CliOptions) -> Result<(), String> {
    let stdin_text = read_stdin_if_piped()?;
    let mut request = Request::from_options(options, stdin_text)?;

    // Auto-route: `-h auto` picks the best agent for the prompt. With no prompt
    // to classify (interactive launch), fall back to the default agent.
    let mut route_reason = None;
    if request.harness == "auto" {
        match request.prompt.as_deref() {
            Some(prompt) => {
                let (harness, reason) = route::resolve_harness("auto", prompt, None);
                request.harness = normalize_harness(&harness);
                route_reason = reason;
            }
            None => request.harness = "claude".to_string(),
        }
    }

    let factory = HarnessFactory::default();
    let harness = factory.create(&request.harness)?;
    let invocation = harness.build(&request)?;

    if request.dry_run {
        println!("{}", invocation.to_json());
        return Ok(());
    }

    if let Some(reason) = &route_reason {
        eprintln!("⚖  auto-route → {} — {reason}", request.harness);
    }

    let inherit_stdin = request.prompt.is_none();
    let cwd = request.cwd.clone();

    // With a prompt we can classify the task and time the run for telemetry;
    // without one (interactive launch) just hand off.
    let Some(prompt) = request.prompt.clone() else {
        return run_invocation(invocation, cwd.as_deref(), inherit_stdin);
    };

    let class = route::classify(&prompt);
    let started = Instant::now();
    let status = run_invocation_status(invocation, cwd.as_deref(), inherit_stdin)?;
    telemetry::Event {
        cmd: "run",
        harness: Some(request.harness.clone()),
        task_class: Some(class.name().to_string()),
        prompt: Some(prompt),
        ok: status.success(),
        duration_ms: started.elapsed().as_millis(),
        reason: route_reason,
        ..telemetry::Event::default()
    }
    .record();
    std::process::exit(status.code().unwrap_or(1));
}

fn read_stdin_if_piped() -> Result<String, String> {
    if io::stdin().is_terminal() {
        return Ok(String::new());
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("failed to read stdin: {error}"))?;
    Ok(input)
}

pub(crate) struct EnvDefaults {
    pub(crate) harness: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    /// None = unset (yolo defaults on); Some(false) = an explicit opt-out via
    /// PARLEY_YOLO or a persisted `default --no-yolo`.
    pub(crate) yolo: Option<bool>,
}

impl EnvDefaults {
    fn load() -> Self {
        let file = DefaultConfig::load().unwrap_or_default();
        let selection = file.selection;

        Self {
            harness: env_first("PARLEY_HARNESS", "AGENT_ROUTER_HARNESS").or(selection.harness),
            provider: env_first("PARLEY_PROVIDER", "AGENT_ROUTER_PROVIDER").or(selection.provider),
            model: env_first("PARLEY_MODEL", "AGENT_ROUTER_MODEL").or(selection.model),
            yolo: env_first("PARLEY_YOLO", "AGENT_ROUTER_YOLO")
                .and_then(|value| parse_bool(&value)),
        }
    }
}

/// Read `primary`, falling back to the legacy `AGENT_ROUTER_*` name for
/// backward compatibility after the rename to Parley.
fn env_first(primary: &str, legacy: &str) -> Option<String> {
    env::var(primary).ok().or_else(|| env::var(legacy).ok())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
