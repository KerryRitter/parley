//! `par commands install` — generate slash commands into the agents `par`
//! drives, so a `/fuse`, `/solve`, or `/route` is one keystroke away from inside
//! Claude Code or Codex. This is the router's "generated slash commands" idea
//! (`/router-off`, `/force-model`, …) pointed at Parley's verbs.
//!
//! Each file carries a `par-generated` marker so re-running replaces only its
//! own output, never a hand-authored command — the same contract as
//! `par convert`. Commit `.claude/` and git-ignore the generated files, or
//! regenerate on demand.

use std::env;
use std::path::PathBuf;

use crate::cli::{CommandsCommand, CommandsOptions};
use crate::fsx;
use crate::harness::normalize_harness;

const MARKER: &str = "par-generated: regenerate with `par commands install`";

/// One generated command: its slash name and what `par` invocation it runs.
struct CommandDef {
    name: &'static str,
    description: &'static str,
    par_command: &'static str,
}

const COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "fuse",
        description: "Convene a panel of agents and fuse their answers into one (Parley)",
        par_command: "fuse",
    },
    CommandDef {
        name: "solve",
        description: "Route to the best agent; auto-escalate to a panel if it gets stuck (Parley)",
        par_command: "solve",
    },
    CommandDef {
        name: "route",
        description: "Show which agent Parley would route a task to, and why",
        par_command: "route",
    },
];

pub(crate) fn run(options: CommandsOptions) -> Result<(), String> {
    match options.command {
        CommandsCommand::List => {
            for c in COMMANDS {
                println!(
                    "/{:<8} -> par {} \"$ARGUMENTS\"   ({})",
                    c.name, c.par_command, c.description
                );
            }
            Ok(())
        }
        CommandsCommand::Install => install(options),
    }
}

fn install(options: CommandsOptions) -> Result<(), String> {
    let target = normalize_harness(
        &options
            .agent
            .clone()
            .unwrap_or_else(|| "claude".to_string()),
    );
    let base = match &options.dir {
        Some(dir) => PathBuf::from(dir),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };

    let dir = match target.as_str() {
        "claude" => base.join(".claude").join("commands"),
        "codex" => base.join(".codex").join("prompts"),
        other => {
            return Err(format!(
                "commands install supports claude and codex (got \"{other}\")"
            ))
        }
    };

    for c in COMMANDS {
        let path = dir.join(format!("{}.md", c.name));
        let body = render(&target, c);
        if options.dry_run {
            println!("dry-run: write {}", path.display());
            continue;
        }
        fsx::write(&path, &body)?;
        println!("installed: {}", path.display());
    }
    if !options.dry_run {
        println!("\nSlash commands ready: {}", commands_summary());
        println!(
            "(git-ignore {} or commit it; regenerate any time.)",
            dir.display()
        );
    }
    Ok(())
}

/// Render a command file. Claude reads YAML frontmatter (description,
/// allowed-tools); Codex prompt files are plain markdown, so we skip it there.
fn render(target: &str, c: &CommandDef) -> String {
    let invocation = format!("par {} \"$ARGUMENTS\"", c.par_command);
    if target == "claude" {
        format!(
            "---\ndescription: {}\nargument-hint: <prompt>\nallowed-tools: Bash(par:*)\n---\n<!-- {MARKER} -->\nUse Parley for this task. Run:\n\n```sh\n{invocation}\n```\n\nThen report the result to the user.\n",
            c.description
        )
    } else {
        format!(
            "<!-- {MARKER} -->\n{}\n\nRun:\n\n```sh\n{invocation}\n```\n\nThen report the result.\n",
            c.description
        )
    }
}

fn commands_summary() -> String {
    COMMANDS
        .iter()
        .map(|c| format!("/{}", c.name))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_render_has_frontmatter_and_marker() {
        let body = render("claude", &COMMANDS[0]);
        assert!(body.starts_with("---\n"));
        assert!(body.contains("allowed-tools: Bash(par:*)"));
        assert!(body.contains(MARKER));
        assert!(body.contains("par fuse \"$ARGUMENTS\""));
    }

    #[test]
    fn codex_render_is_plain_markdown() {
        let body = render("codex", &COMMANDS[1]);
        assert!(!body.starts_with("---"));
        assert!(body.contains(MARKER));
        assert!(body.contains("par solve \"$ARGUMENTS\""));
    }
}
