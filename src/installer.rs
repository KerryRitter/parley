use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstallOptions {
    pub(crate) target: InstallTarget,
    pub(crate) dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstallTarget {
    One(String),
    All,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallerKind {
    Direct {
        command: &'static str,
        args: &'static [&'static str],
    },
    Shell {
        command: &'static str,
    },
    Manual {
        instructions: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Installer {
    name: &'static str,
    binary: &'static str,
    kind: InstallerKind,
    source: &'static str,
}

const INSTALLERS: &[Installer] = &[
    Installer {
        name: "claude",
        binary: "claude",
        kind: InstallerKind::Direct {
            command: "npm",
            args: &["install", "-g", "@anthropic-ai/claude-code"],
        },
        source: "https://docs.claude.com/en/docs/claude-code/setup",
    },
    Installer {
        name: "codex",
        binary: "codex",
        kind: InstallerKind::Direct {
            command: "npm",
            args: &["install", "-g", "@openai/codex"],
        },
        source: "https://help.openai.com/en/articles/11096431-openai-codex-cli-getting-started",
    },
    Installer {
        name: "cursor",
        binary: "cursor-agent",
        kind: InstallerKind::Shell {
            command: "curl https://cursor.com/install -fsS | bash",
        },
        source: "https://docs.cursor.com/en/cli/installation",
    },
    Installer {
        name: "gemini",
        binary: "gemini",
        kind: InstallerKind::Direct {
            command: "npm",
            args: &["install", "-g", "@google/gemini-cli"],
        },
        source: "https://google-gemini.github.io/gemini-cli/docs/get-started/",
    },
    Installer {
        name: "goose",
        binary: "goose",
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | bash",
        },
        source: "https://block.github.io/goose/docs/getting-started/installation/",
    },
    Installer {
        name: "opencode",
        binary: "opencode",
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://opencode.ai/install | bash",
        },
        source: "https://opencode.ai/download",
    },
    Installer {
        name: "qwen",
        binary: "qwen",
        kind: InstallerKind::Direct {
            command: "npm",
            args: &["install", "-g", "@qwen-code/qwen-code"],
        },
        source: "https://qwenlm.github.io/qwen-code-docs/en/deployment/",
    },
    Installer {
        name: "aider",
        binary: "aider",
        kind: InstallerKind::Shell {
            command: "curl -LsSf https://aider.chat/install.sh | sh",
        },
        source: "https://aider.chat/docs/install.html",
    },
    Installer {
        name: "amazon-q",
        binary: "q",
        kind: InstallerKind::Manual {
            instructions: "Install Amazon Q Developer for CLI from https://aws.amazon.com/developer/learning/q-developer-cli/ and then verify with `q --version`.",
        },
        source: "https://aws.amazon.com/developer/learning/q-developer-cli/",
    },
    Installer {
        name: "copilot",
        binary: "copilot",
        kind: InstallerKind::Direct {
            command: "npm",
            args: &["install", "-g", "@github/copilot"],
        },
        source: "https://docs.github.com/copilot/how-tos/copilot-cli/install-copilot-cli",
    },
    Installer {
        name: "antigravity",
        binary: "agy",
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://antigravity.google/cli/install.sh | bash",
        },
        source: "https://antigravity.google/download",
    },
];

pub(crate) fn run_install(options: InstallOptions) -> Result<(), String> {
    match options.target {
        InstallTarget::List => {
            print_installers();
            Ok(())
        }
        InstallTarget::One(name) => {
            let installer = find_installer(&name)?;
            run_one(installer, options.dry_run)
        }
        InstallTarget::All => {
            for installer in INSTALLERS {
                run_one(installer, options.dry_run)?;
            }
            Ok(())
        }
    }
}

pub(crate) fn known_installers() -> Vec<&'static str> {
    INSTALLERS.iter().map(|installer| installer.name).collect()
}

fn print_installers() {
    for installer in INSTALLERS {
        println!(
            "{:<12} binary={:<13} source={}",
            installer.name, installer.binary, installer.source
        );
    }
}

fn find_installer(name: &str) -> Result<&'static Installer, String> {
    let normalized = normalize_installer_name(name);
    INSTALLERS
        .iter()
        .find(|installer| installer.name == normalized)
        .ok_or_else(|| {
            format!(
                "unknown installer \"{name}\". Known installers: {}",
                known_installers().join(", ")
            )
        })
}

fn normalize_installer_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "openai" => "codex".to_string(),
        "cursor-agent" => "cursor".to_string(),
        "google" | "google-gemini" => "gemini".to_string(),
        "open-code" => "opencode".to_string(),
        "amazonq" | "aws-q" | "amazon" => "amazon-q".to_string(),
        "github-copilot" => "copilot".to_string(),
        "agy" | "google-antigravity" => "antigravity".to_string(),
        value => value.to_string(),
    }
}

fn run_one(installer: &Installer, dry_run: bool) -> Result<(), String> {
    println!("installer: {} ({})", installer.name, installer.source);

    match installer.kind {
        InstallerKind::Direct { command, args } => {
            if dry_run {
                println!("dry-run: {} {}", command, args.join(" "));
                return Ok(());
            }
            if command_exists(installer.binary) {
                println!("already installed: {}", installer.binary);
                return Ok(());
            }
            run_command(command, args)
        }
        InstallerKind::Shell { command } => {
            if dry_run {
                println!("dry-run: sh -c '{}'", command);
                return Ok(());
            }
            if command_exists(installer.binary) {
                println!("already installed: {}", installer.binary);
                return Ok(());
            }
            run_shell(command)
        }
        InstallerKind::Manual { instructions } => {
            println!("{instructions}");
            Ok(())
        }
    }
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "command -v '{}' >/dev/null 2>&1",
            shell_escape(command)
        ))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_command(command: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(command)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to start {command}: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} exited with status {status}"))
    }
}

fn run_shell(command: &str) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to start shell installer: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("installer exited with status {status}"))
    }
}

fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_installer_aliases() {
        assert_eq!(normalize_installer_name("openai"), "codex");
        assert_eq!(normalize_installer_name("cursor-agent"), "cursor");
        assert_eq!(normalize_installer_name("agy"), "antigravity");
    }

    #[test]
    fn exposes_expected_installers() {
        assert!(known_installers().contains(&"claude"));
        assert!(known_installers().contains(&"antigravity"));
    }
}
