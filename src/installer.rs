use std::env;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstallOptions {
    pub(crate) target: InstallTarget,
    pub(crate) dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateOptions {
    pub(crate) target: UpdateTarget,
    pub(crate) dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateTarget {
    Self_,
    One(String),
    All,
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
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://claude.ai/install.sh | bash",
        },
        source: "https://code.claude.com/docs/en/setup",
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
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen.sh | bash",
        },
        source: "https://qwenlm.github.io/qwen-code-docs/en/users/quickstart/",
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
        name: "kimi",
        binary: "kimi",
        kind: InstallerKind::Shell {
            command: "curl -LsSf https://code.kimi.com/install.sh | bash",
        },
        source: "https://kimi.ai/code",
    },
    Installer {
        name: "copilot",
        binary: "copilot",
        kind: InstallerKind::Shell {
            command: "curl -fsSL https://gh.io/copilot-install | bash",
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
            run_one(installer, options.dry_run, false)
        }
        InstallTarget::All => {
            for installer in INSTALLERS {
                run_one(installer, options.dry_run, false)?;
            }
            Ok(())
        }
    }
}

pub(crate) fn run_update(options: UpdateOptions) -> Result<(), String> {
    match options.target {
        UpdateTarget::Self_ => update_self(options.dry_run),
        UpdateTarget::One(name) => {
            let installer = find_installer(&name)?;
            run_one(installer, options.dry_run, true)
        }
        UpdateTarget::All => {
            update_self(options.dry_run)?;
            for installer in INSTALLERS {
                run_one(installer, options.dry_run, true)?;
            }
            Ok(())
        }
    }
}

/// Self-update: pull the latest prebuilt release binary and replace the running
/// `par` in place. Reuses the platform install script (which already handles
/// target detection, the musl mapping, download, and the atomic overwrite) with
/// `PAR_INSTALL_DIR` pinned to this binary's own directory so it updates the
/// copy you're actually running rather than dropping a second one elsewhere.
fn update_self(dry_run: bool) -> Result<(), String> {
    let dir = self_install_dir();
    println!("updating par in {dir}...");

    if cfg!(windows) {
        // PowerShell drives install.ps1; single-quotes in the dir are doubled.
        let script = format!(
            "$env:PAR_INSTALL_DIR='{}'; irm https://raw.githubusercontent.com/KerryRitter/parley/main/install.ps1 | iex",
            dir.replace('\'', "''")
        );
        if dry_run {
            println!("dry-run: powershell -NoProfile -Command \"{script}\"");
            return Ok(());
        }
        return run_updater(
            "powershell",
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ],
            &dir,
        );
    }

    // Unix: curl|sh the install script. Overwriting the running binary is safe —
    // `install` unlinks and recreates, so this process keeps its old inode.
    let command =
        "curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/KerryRitter/parley/main/install.sh | sh";
    if dry_run {
        println!("dry-run: PAR_INSTALL_DIR={dir} sh -c '{command}'");
        return Ok(());
    }
    run_updater("sh", &["-c", command], &dir)
}

/// Directory the running `par` lives in — where a self-update should land.
/// Resolves symlinks (e.g. the `agent-router` alias) via `current_exe`.
fn self_install_dir() -> String {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.to_string_lossy().into_owned();
        }
    }
    // Fall back to the default install location if the exe path is unavailable.
    if cfg!(windows) {
        match env::var("LOCALAPPDATA") {
            Ok(base) => format!("{base}\\Programs\\par"),
            Err(_) => ".".to_string(),
        }
    } else {
        match env::var("HOME") {
            Ok(home) => format!("{home}/.local/bin"),
            Err(_) => ".".to_string(),
        }
    }
}

/// Run the updater command with `PAR_INSTALL_DIR` set and stdio inherited.
fn run_updater(command: &str, args: &[&str], install_dir: &str) -> Result<(), String> {
    let status = Command::new(command)
        .args(args)
        .env("PAR_INSTALL_DIR", install_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to start the updater ({command}): {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "self-update failed: {command} exited with {status}"
        ))
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
        "moonshot" | "kimi-code" => "kimi".to_string(),
        "github-copilot" => "copilot".to_string(),
        "agy" | "google-antigravity" => "antigravity".to_string(),
        value => value.to_string(),
    }
}

fn run_one(installer: &Installer, dry_run: bool, force: bool) -> Result<(), String> {
    println!("installer: {} ({})", installer.name, installer.source);

    match installer.kind {
        InstallerKind::Direct { command, args } => {
            if dry_run {
                println!("dry-run: {} {}", command, args.join(" "));
                return Ok(());
            }
            if !force && command_exists(installer.binary) {
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
            if !force && command_exists(installer.binary) {
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
        assert_eq!(
            known_installers(),
            vec![
                "claude",
                "codex",
                "cursor",
                "gemini",
                "goose",
                "opencode",
                "qwen",
                "aider",
                "amazon-q",
                "kimi",
                "copilot",
                "antigravity",
            ]
        );
    }
}
