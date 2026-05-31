use std::fs;
use std::path::Path;

use crate::json::Json;

use super::project::{CommandFile, ContentFile, McpServer, PersonaFile, ProjectConfig};

pub(crate) fn read(root: &Path) -> Result<ProjectConfig, String> {
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    let instructions = read_optional(root, "CLAUDE.md")?;
    let commands = read_commands(root)?;
    let skills = read_skills(root)?;
    let references = read_content_dir(root, ".claude/references")?;
    let personas = read_personas(root)?;
    let mcp_servers = read_mcp_json(root)?;

    Ok(ProjectConfig {
        name,
        instructions,
        commands,
        skills,
        references,
        personas,
        mcp_servers,
    })
}

fn read_optional(root: &Path, rel: &str) -> Result<Option<String>, String> {
    let path = root.join(rel);
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn read_commands(root: &Path) -> Result<Vec<CommandFile>, String> {
    let dir = root.join(".claude/commands");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_md_files(&dir, &dir, &mut files)?;
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(files)
}

fn collect_md_files(
    base: &Path,
    dir: &Path,
    out: &mut Vec<CommandFile>,
) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(base, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(base)
                .map_err(|_| "path prefix error")?
                .to_string_lossy()
                .to_string();
            let body = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            out.push(CommandFile::from_path_and_body(rel, body));
        }
    }

    Ok(())
}

fn read_content_dir(root: &Path, rel_dir: &str) -> Result<Vec<ContentFile>, String> {
    let dir = root.join(rel_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if filename == "README.md" || !filename.ends_with(".md") {
            continue;
        }
        let body = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        files.push(ContentFile::from_filename_and_body(filename, body));
    }

    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(files)
}

/// Read `.claude/skills`. Supports both flat `name.md` files and skill
/// directories holding a `SKILL.md` (Claude's progressive-disclosure layout).
fn read_skills(root: &Path) -> Result<Vec<ContentFile>, String> {
    let dir = root.join(".claude/skills");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                let body = fs::read_to_string(&skill_md)
                    .map_err(|e| format!("failed to read {}: {e}", skill_md.display()))?;
                files.push(ContentFile::from_filename_and_body(
                    format!("{name}.md"),
                    body,
                ));
            }
            continue;
        }

        if !path.is_file() || name == "README.md" || !name.ends_with(".md") {
            continue;
        }
        let body = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        files.push(ContentFile::from_filename_and_body(name, body));
    }

    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(files)
}

/// Read `.claude/agents/**` persona files (recursively), capturing their
/// frontmatter (`tools:`, `model:`) so targets can render them as subagents.
fn read_personas(root: &Path) -> Result<Vec<PersonaFile>, String> {
    let dir = root.join(".claude/agents");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_md_paths(&dir, &mut paths)?;
    paths.sort();

    let mut personas = Vec::new();
    for path in paths {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if filename == "README.md" {
            continue;
        }
        let body = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        personas.push(PersonaFile::from_filename_and_body(filename, body));
    }
    Ok(personas)
}

fn collect_md_paths(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_md_paths(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn read_mcp_json(root: &Path) -> Result<Vec<(String, McpServer)>, String> {
    let path = root.join(".mcp.json");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let json = Json::parse(&text).map_err(|e| format!("failed to parse .mcp.json: {e}"))?;

    let servers_obj = match json.get("mcpServers") {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };

    let map = match servers_obj.as_object() {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut servers = Vec::new();
    for (name, value) in map {
        let command = value
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let args = value
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let env = value
            .get("env")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let cwd = value
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(String::from);

        let disabled = value
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        servers.push((
            name.clone(),
            McpServer {
                command,
                args,
                env,
                cwd,
                disabled,
            },
        ));
    }

    Ok(servers)
}
