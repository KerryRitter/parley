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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CliAction {
    Help,
    Version,
    Default(DefaultCommand),
    Shims(ShimOptions),
    Install(InstallOptions),
    Update(UpdateOptions),
    Convert(ConvertOptions),
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
        _ => {}
    }

    let mut options = CliOptions {
        harness: defaults.harness.unwrap_or_else(|| "claude".to_string()),
        provider: defaults.provider,
        model: defaults.model,
        yolo: defaults.yolo,
        ..CliOptions::default()
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CliAction::Help),
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
  --harness <name>        claude, codex, cursor, gemini, goose, opencode, qwen, aider, amazon-q, copilot, kimi, antigravity
  --provider <name>       Provider namespace when the target CLI supports one
  --model, -m <name>      Model name to pass through
  --agent <name>          Agent/persona name for harnesses that support it
  --output-format <fmt>   text, json, stream-json when supported by target
  --cwd <path>            Working directory for the target CLI
  --yolo                  Add the harness-specific permission bypass flag where supported
  --no-yolo               Disable a persisted yolo default for this run
  --dry-run               Print the routed command as JSON
  --                      Pass remaining flags through to the target CLI

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

Environment defaults:
  AGENT_ROUTER_HARNESS, AGENT_ROUTER_PROVIDER, AGENT_ROUTER_MODEL, AGENT_ROUTER_YOLO
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
            yolo: false,
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
        let action =
            parse_args(["convert", "opencode"].map(String::from), defaults()).unwrap();

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
