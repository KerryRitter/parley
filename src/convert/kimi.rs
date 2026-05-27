use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::json::Json;

use super::project::ProjectConfig;

pub(crate) fn write(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let mut created = Vec::new();

    write_agents_md(root, config, dry_run, &mut created)?;
    write_kimi_skills(root, config, dry_run, &mut created)?;
    write_kimi_mcp(root, config, dry_run, &mut created)?;

    Ok(created)
}

fn write_agents_md(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    let content = format!(
        "{}\n{}\n",
        HEADER,
        config.build_self_contained().trim_end()
    );
    write_file(root, "AGENTS.md", &content, dry_run, created)
}

fn write_kimi_skills(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    if config.commands.is_empty() {
        return Ok(());
    }

    if !dry_run {
        let skills_dir = root.join(".kimi/skills");
        if skills_dir.exists() {
            let entries = fs::read_dir(&skills_dir)
                .map_err(|e| format!("failed to read .kimi/skills: {e}"))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("dir entry error: {e}"))?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("source-command-") && entry.path().is_dir() {
                    fs::remove_dir_all(entry.path())
                        .map_err(|e| format!("failed to remove skill dir: {e}"))?;
                }
            }
        }
    }

    for cmd in &config.commands {
        let skill_name = kimi_skill_name(&cmd.name);
        let command_name = format!("/{}", cmd.name);
        let desc = if cmd.description.starts_with("Run ") {
            cmd.description.clone()
        } else {
            format!("Run {command_name}: {}", cmd.description)
        };
        let desc = truncate(&desc, 240);

        let skill_content = format!(
            "---\nname: {skill_name}\ndescription: |\n  {desc}\n---\n\n# {command_name}\n\nUse this skill when the user asks to run `{command_name}`.\n\n{}\n",
            cmd.body.trim(),
        );

        write_file(
            root,
            &format!(".kimi/skills/{skill_name}/SKILL.md"),
            &skill_content,
            dry_run,
            created,
        )?;
    }

    Ok(())
}

fn write_kimi_mcp(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    if config.mcp_servers.is_empty() {
        return Ok(());
    }

    let mut mcp_map = BTreeMap::new();
    for (name, server) in &config.mcp_servers {
        let mut entry = BTreeMap::new();
        entry.insert("command".to_string(), Json::Str(server.command.clone()));
        if !server.args.is_empty() {
            entry.insert(
                "args".to_string(),
                Json::Array(server.args.iter().map(|a| Json::Str(a.clone())).collect()),
            );
        }
        if !server.env.is_empty() {
            let env_map: BTreeMap<String, Json> = server
                .env
                .iter()
                .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                .collect();
            entry.insert("env".to_string(), Json::Object(env_map));
        }
        mcp_map.insert(name.clone(), Json::Object(entry));
    }

    let json = Json::Object({
        let mut m = BTreeMap::new();
        m.insert("mcpServers".to_string(), Json::Object(mcp_map));
        m
    });

    write_file(root, ".kimi/mcp.json", &json.to_pretty_string(), dry_run, created)
}

fn kimi_skill_name(name: &str) -> String {
    let raw = format!("source-command-{}", slugify(name));
    if raw.len() <= 64 {
        raw
    } else {
        raw[..64].trim_end_matches('-').to_string()
    }
}

fn slugify(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}...", s[..max - 3].trim_end())
}

fn write_file(
    root: &Path,
    rel: &str,
    content: &str,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    if dry_run {
        created.push(format!("(dry-run) {rel}"));
        return Ok(());
    }
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    fs::write(&path, content).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    created.push(rel.to_string());
    Ok(())
}

const HEADER: &str =
    "<!-- AUTO-GENERATED by par convert — do not edit. Edit source files, then re-run: par convert -->";
