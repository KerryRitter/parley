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

    write_instructions_rule(root, config, dry_run, &mut created)?;
    write_command_rules(root, config, dry_run, &mut created)?;
    write_mcp_json(root, config, dry_run, &mut created)?;

    Ok(created)
}

fn write_instructions_rule(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    let body = config.build_self_contained();
    if body.trim().is_empty() {
        return Ok(());
    }

    let content = format!(
        "---\ndescription: \"Project instructions and reference documentation\"\nalwaysApply: true\n---\n\n{}\n",
        body.trim_end()
    );
    write_file(
        root,
        ".cursor/rules/instructions.mdc",
        &content,
        dry_run,
        created,
    )
}

fn write_command_rules(
    root: &Path,
    config: &ProjectConfig,
    dry_run: bool,
    created: &mut Vec<String>,
) -> Result<(), String> {
    if config.commands.is_empty() {
        return Ok(());
    }

    if !dry_run {
        let rules_dir = root.join(".cursor/rules/commands");
        if rules_dir.exists() {
            fs::remove_dir_all(&rules_dir)
                .map_err(|e| format!("failed to remove .cursor/rules/commands: {e}"))?;
        }
    }

    for cmd in &config.commands {
        let mdc_rel = cmd.rel_path.trim_end_matches(".md").to_string() + ".mdc";
        let mdc_path = format!(".cursor/rules/commands/{mdc_rel}");
        let desc = cmd.description.replace('"', "\\\"");

        let content = format!(
            "---\ndescription: \"/{name}: {desc}\"\nalwaysApply: false\n---\n\n{}\n",
            cmd.body.trim_end(),
            name = cmd.name,
        );
        write_file(root, &mdc_path, &content, dry_run, created)?;
    }

    Ok(())
}

fn write_mcp_json(
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

    write_file(
        root,
        ".cursor/mcp.json",
        &json.to_pretty_string(),
        dry_run,
        created,
    )
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
