mod ask;
mod cli;
mod config;
mod converse;
mod convert;
mod fuse;
mod harness;
mod installer;
mod json;
mod mcp;
mod model;
mod process;
mod session;

use std::env;
use std::io::{self, IsTerminal, Read, Write};

use cli::{parse_args, usage, CliAction};
use config::DefaultConfig;
use harness::{HarnessFactory, Request};
use installer::{run_install, run_update};
use process::run_invocation;

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
        CliAction::Mcp(options) => match &options.connect {
            Some(harness) => mcp::connect(harness, options.dry_run),
            None => mcp::run(options),
        },
        CliAction::Run(options) => {
            let stdin_text = read_stdin_if_piped()?;
            let request = Request::from_options(*options, stdin_text)?;
            let factory = HarnessFactory::default();
            let harness = factory.create(&request.harness)?;
            let invocation = harness.build(&request)?;

            if request.dry_run {
                println!("{}", invocation.to_json());
                return Ok(());
            }

            let inherit_stdin = request.prompt.is_none();
            run_invocation(invocation, request.cwd.as_deref(), inherit_stdin)
        }
    }
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
