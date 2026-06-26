use std::env;
use std::fs;
use std::path::PathBuf;

use crate::cli::DefaultCommand;
use crate::harness::{normalize_harness, spec_for_harness};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DefaultSelection {
    pub(crate) harness: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) yolo: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DefaultConfig {
    pub(crate) selection: DefaultSelection,
}

impl DefaultConfig {
    pub(crate) fn load() -> Result<Self, String> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        Ok(Self {
            selection: parse_selection(&text)?,
        })
    }

    fn save(&self) -> Result<(), String> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::write(&path, format_selection(&self.selection))
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }
}

pub(crate) fn run_default_command(command: DefaultCommand) -> Result<(), String> {
    match command {
        DefaultCommand::Show => print_current(),
        DefaultCommand::Path => {
            println!("{}", config_path()?.display());
            Ok(())
        }
        DefaultCommand::Set {
            harness,
            provider,
            model,
            yolo,
        } => {
            let mut config = DefaultConfig::load()?;
            if let Some(harness) = harness {
                let normalized = normalize_harness(&harness);
                // `auto` is a valid default: it defers the choice to the
                // per-prompt router at run time.
                if normalized != "auto" && spec_for_harness(&normalized).is_none() {
                    return Err(format!("unknown harness \"{harness}\""));
                }
                config.selection.harness = Some(normalized);
            }
            if let Some(provider) = provider {
                config.selection.provider = Some(provider);
            }
            if let Some(model) = model {
                config.selection.model = Some(model);
            }
            if let Some(yolo) = yolo {
                config.selection.yolo = yolo;
            }
            config.save()?;
            print_selection(&config.selection);
            Ok(())
        }
        DefaultCommand::List => {
            for harness in crate::harness::known_harnesses() {
                println!("{harness}");
            }
            Ok(())
        }
    }
}

fn print_current() -> Result<(), String> {
    let config = DefaultConfig::load()?;
    print_selection(&config.selection);
    Ok(())
}

fn print_selection(selection: &DefaultSelection) {
    println!(
        "harness={}",
        selection.harness.as_deref().unwrap_or("claude")
    );
    if let Some(provider) = &selection.provider {
        println!("provider={provider}");
    }
    if let Some(model) = &selection.model {
        println!("model={model}");
    }
    println!("yolo={}", selection.yolo);
}

fn config_path() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("PAR_DEFAULT_FILE") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(home) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(home).join("par").join("default"));
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set; cannot find config dir")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("par")
        .join("default"))
}

fn parse_selection(text: &str) -> Result<DefaultSelection, String> {
    let mut selection = DefaultSelection::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid default config line: {line}"));
        };
        match key.trim() {
            "harness" => selection.harness = Some(normalize_harness(value.trim())),
            "provider" => selection.provider = Some(value.trim().to_string()),
            "model" => selection.model = Some(value.trim().to_string()),
            "yolo" => selection.yolo = parse_bool(value.trim())?,
            _ => return Err(format!("unknown default config key: {}", key.trim())),
        }
    }
    Ok(selection)
}

fn format_selection(selection: &DefaultSelection) -> String {
    let mut lines = Vec::new();
    if let Some(harness) = &selection.harness {
        lines.push(format!("harness={harness}"));
    }
    if let Some(provider) = &selection.provider {
        lines.push(format!("provider={provider}"));
    }
    if let Some(model) = &selection.model {
        lines.push(format!("model={model}"));
    }
    lines.push(format!("yolo={}", selection.yolo));
    lines.push(String::new());
    lines.join("\n")
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_file() {
        let selection = parse_selection("harness=openai\nmodel=gpt-5.4\nyolo=true\n").unwrap();

        assert_eq!(selection.harness, Some("codex".to_string()));
        assert_eq!(selection.model, Some("gpt-5.4".to_string()));
        assert!(selection.yolo);
    }

    #[test]
    fn formats_default_file() {
        let selection = DefaultSelection {
            harness: Some("claude".to_string()),
            provider: None,
            model: Some("sonnet".to_string()),
            yolo: false,
        };

        assert_eq!(
            format_selection(&selection),
            "harness=claude\nmodel=sonnet\nyolo=false\n"
        );
    }
}
