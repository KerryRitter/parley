use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::json::Json;

use super::project::ProjectConfig;

pub(crate) fn write(root: &Path, config: &ProjectConfig, dry_run: bool) -> Result<Vec<String>, String> {
    let mut created = Vec::new();

    let mut top = BTreeMap::new();
    top.insert(
        "$schema".to_string(),
        Json::Str("https://opencode.ai/config.json".to_string()),
    );

    // MCP servers
    if !config.mcp_servers.is_empty() {
        let mut mcp = BTreeMap::new();
        for (name, server) in &config.mcp_servers {
            let mut entry = BTreeMap::new();
            entry.insert("type".to_string(), Json::Str("local".to_string()));
            entry.insert(
                "command".to_string(),
                Json::Array(vec![Json::Str(server.command.clone())]),
            );
            entry.insert(
                "args".to_string(),
                Json::Array(server.args.iter().map(|a| Json::Str(a.clone())).collect()),
            );
            mcp.insert(name.clone(), Json::Object(entry));
        }
        top.insert("mcp".to_string(), Json::Object(mcp));
    }

    // Commands from .claude/commands
    if !config.commands.is_empty() {
        let mut commands = BTreeMap::new();
        for cmd in &config.commands {
            let cmd_name = cmd.name.replace('/', ":");
            let mut entry = BTreeMap::new();
            entry.insert(
                "description".to_string(),
                Json::Str(cmd.description.clone()),
            );
            entry.insert(
                "template".to_string(),
                Json::Str(format!(
                    "Please read and execute the instructions in .claude/commands/{}. User arguments: $ARGUMENTS",
                    cmd.rel_path
                )),
            );
            commands.insert(cmd_name, Json::Object(entry));
        }
        top.insert("command".to_string(), Json::Object(commands));
    }

    let json = Json::Object(top);

    if !dry_run {
        fs::create_dir_all(root.join(".opencode"))
            .map_err(|e| format!("failed to create .opencode: {e}"))?;
    }
    write_file(root, ".opencode/config.json", &json.to_pretty_string(), dry_run, &mut created)?;

    Ok(created)
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
