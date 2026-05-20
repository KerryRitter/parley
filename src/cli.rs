use crate::installer::{InstallOptions, InstallTarget};
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CliAction {
    Help,
    Version,
    Install(InstallOptions),
    Run(Box<CliOptions>),
}

pub(crate) fn parse_args<I>(argv: I, defaults: EnvDefaults) -> Result<CliAction, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = argv.into_iter().peekable();

    if matches!(args.peek().map(String::as_str), Some("install")) {
        args.next();
        return parse_install_args(args);
    }

    let mut options = CliOptions {
        harness: defaults.harness.unwrap_or_else(|| "claude".to_string()),
        provider: defaults.provider,
        model: defaults.model,
        ..CliOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
            "--version" | "-v" => return Ok(CliAction::Version),
            "--dry-run" => options.dry_run = true,
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
  par -p \"prompt\" --harness codex --model gpt-5.4
  par --harness opencode --provider anthropic --model claude-sonnet-4-6 \"fix the failing tests\"
  cat README.md | par -p \"summarize this\" --harness gemini --output-format json
  par install claude
  par install list

Options:
  --harness <name>        claude, codex, cursor, gemini, goose, opencode, qwen, aider, amazon-q, copilot, antigravity
  --provider <name>       Provider namespace when the target CLI supports one
  --model, -m <name>      Model name to pass through
  --agent <name>          Agent/persona name for harnesses that support it
  --output-format <fmt>   text, json, stream-json when supported by target
  --cwd <path>            Working directory for the target CLI
  --dry-run               Print the routed command as JSON
  --                      Pass remaining flags through to the target CLI

Install:
  install <name>          Install one supported downstream harness CLI
  install list            List supported harness installers
  install all             Run every supported installer
  install --dry-run <name> Print installer commands without running them

Environment defaults:
  AGENT_ROUTER_HARNESS, AGENT_ROUTER_PROVIDER, AGENT_ROUTER_MODEL
"
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
                ..CliOptions::default()
            }))
        );
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
}
