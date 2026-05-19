mod cli;
mod harness;
mod model;
mod process;

use std::env;
use std::io::{self, IsTerminal, Read, Write};

use cli::{parse_args, usage, CliAction};
use harness::{HarnessFactory, Request};
use process::run_invocation;

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(io::stderr(), "agent-router: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let action = parse_args(env::args().skip(1), EnvDefaults::from_env())?;

    match action {
        CliAction::Help => {
            print!("{}", usage());
            Ok(())
        }
        CliAction::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
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

            run_invocation(invocation, request.cwd.as_deref())
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
}

impl EnvDefaults {
    fn from_env() -> Self {
        Self {
            harness: env::var("AGENT_ROUTER_HARNESS").ok(),
            provider: env::var("AGENT_ROUTER_PROVIDER").ok(),
            model: env::var("AGENT_ROUTER_MODEL").ok(),
        }
    }
}
