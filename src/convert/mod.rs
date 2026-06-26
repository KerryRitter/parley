mod antigravity;
mod claude;
mod codex;
mod cursor;
mod frontmatter;
mod gemini;
mod kimi;
mod links;
mod opencode;
pub(crate) mod project;
mod util;

use std::env;
use std::path::PathBuf;

use crate::harness::normalize_harness;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConvertOptions {
    pub from: Option<String>,
    pub to: ConvertTarget,
    pub cwd: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConvertTarget {
    All,
    One(String),
}

const ALL_TARGETS: &[&str] = &[
    "gemini",
    "codex",
    "antigravity",
    "opencode",
    "cursor",
    "kimi",
];

pub(crate) fn run_convert(options: ConvertOptions) -> Result<(), String> {
    let root = resolve_root(&options)?;
    let source = validate_source(options.from.as_deref().unwrap_or("claude"))?;

    println!("=== par convert ===");
    println!("  source: {source}");

    let config = read_source(&root, &source)?;
    println!(
        "  read: {} commands, {} skills, {} personas, {} references, {} MCP servers",
        config.commands.len(),
        config.skills.len(),
        config.personas.len(),
        config.references.len(),
        config.mcp_servers.len(),
    );

    let (resolved, unresolved) = links::resolve_project(&config);
    println!("{}\n", links::format_report(resolved, &unresolved));

    let targets = resolve_targets(&options.to, &source)?;
    println!("  targets: {}\n", targets.join(", "));

    for target in &targets {
        let files = write_target(&root, &config, target, options.dry_run)?;
        if files.is_empty() {
            println!("  {target}: (no files generated)");
        } else {
            println!("  {target}:");
            for file in &files {
                println!("    {file}");
            }
        }
    }

    println!("\n=== Done ===");

    if !unresolved.is_empty() {
        return Err(format!(
            "{} unresolved cross-reference(s) — see report above. Fix the source pack, then re-run.",
            unresolved.len()
        ));
    }
    Ok(())
}

fn resolve_root(options: &ConvertOptions) -> Result<PathBuf, String> {
    match &options.cwd {
        Some(path) => Ok(PathBuf::from(path)),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}")),
    }
}

fn validate_source(name: &str) -> Result<String, String> {
    let normalized = normalize_convert_name(name);
    match normalized.as_str() {
        "claude" => Ok(normalized),
        other => Err(format!(
            "reader for \"{other}\" is not yet implemented. Currently supported: claude"
        )),
    }
}

fn read_source(root: &std::path::Path, source: &str) -> Result<project::ProjectConfig, String> {
    match source {
        "claude" => claude::read(root),
        _ => Err(format!("no reader for \"{source}\"")),
    }
}

fn resolve_targets(target: &ConvertTarget, source: &str) -> Result<Vec<String>, String> {
    match target {
        ConvertTarget::All => Ok(ALL_TARGETS
            .iter()
            .filter(|t| **t != source)
            .map(|s| s.to_string())
            .collect()),
        ConvertTarget::One(name) => {
            let normalized = normalize_convert_name(name);
            validate_target(&normalized)?;
            Ok(vec![normalized])
        }
    }
}

fn validate_target(name: &str) -> Result<(), String> {
    if ALL_TARGETS.contains(&name) {
        Ok(())
    } else {
        Err(format!(
            "unknown target \"{name}\". Supported: {}",
            ALL_TARGETS.join(", ")
        ))
    }
}

fn write_target(
    root: &std::path::Path,
    config: &project::ProjectConfig,
    target: &str,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    match target {
        "gemini" => gemini::write(root, config, dry_run),
        "codex" => codex::write(root, config, dry_run),
        "antigravity" => antigravity::write(root, config, dry_run),
        "opencode" => opencode::write(root, config, dry_run),
        "cursor" => cursor::write(root, config, dry_run),
        "kimi" => kimi::write(root, config, dry_run),
        _ => Err(format!("no writer for \"{target}\"")),
    }
}

fn normalize_convert_name(name: &str) -> String {
    let harness = normalize_harness(name);
    match harness.as_str() {
        "codex" => "codex".to_string(),
        _ => harness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn setup_project(root: &Path) {
        fs::create_dir_all(root.join(".claude/commands/git")).unwrap();
        fs::create_dir_all(root.join(".claude/skills")).unwrap();
        fs::create_dir_all(root.join(".claude/references")).unwrap();

        fs::write(
            root.join("CLAUDE.md"),
            "# Test Project\n\nInstructions here.\n",
        )
        .unwrap();
        fs::write(
            root.join(".claude/commands/dev.md"),
            "# /dev - Start development\n\nRun the dev server.\n",
        )
        .unwrap();
        fs::write(
            root.join(".claude/commands/git/merge.md"),
            "# /git/merge - Merge branches\n\nMerge target into current.\n\nArgs: {branch}\n",
        )
        .unwrap();
        fs::write(
            root.join(".claude/skills/sdk-regen.md"),
            "# SDK Regen\n\nRegenerate the SDK.\n",
        )
        .unwrap();
        fs::write(
            root.join(".claude/references/ports.md"),
            "# Ports\n\nFrontend: 3000\nBackend: 4000\n",
        )
        .unwrap();
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"my-tool":{"command":"npx","args":["-y","my-mcp"],"env":{"KEY":"val"}}}}"#,
        )
        .unwrap();
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "par-test-{}-{}",
                std::process::id(),
                Self::counter()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn counter() -> u64 {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            COUNTER.fetch_add(1, Ordering::Relaxed)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir() -> TempDir {
        TempDir::new()
    }

    #[test]
    fn reads_claude_project() {
        let tmp = temp_dir();
        setup_project(tmp.path());

        let config = claude::read(tmp.path()).unwrap();
        assert_eq!(config.commands.len(), 2);
        assert_eq!(config.skills.len(), 1);
        assert_eq!(config.references.len(), 1);
        assert_eq!(config.mcp_servers.len(), 1);
        assert_eq!(config.mcp_servers[0].0, "my-tool");
        assert_eq!(config.mcp_servers[0].1.command, "npx");
        assert_eq!(config.mcp_servers[0].1.env.len(), 1);
    }

    #[test]
    fn writes_gemini_output() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = gemini::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&"GEMINI.md".to_string()));
        assert!(files.contains(&".gemini/settings.json".to_string()));
        assert!(files.iter().any(|f| f.ends_with(".toml")));

        let gemini_md = fs::read_to_string(tmp.path().join("GEMINI.md")).unwrap();
        assert!(gemini_md.contains("Test Project"));
        assert!(gemini_md.contains("Appendix: Skills"));
        assert!(gemini_md.contains("Appendix: Reference Files"));

        let toml = fs::read_to_string(tmp.path().join(".gemini/commands/dev.toml")).unwrap();
        assert!(toml.contains("description = "));
        assert!(toml.contains("{{args}}"));

        let settings = fs::read_to_string(tmp.path().join(".gemini/settings.json")).unwrap();
        assert!(settings.contains("my-tool"));
        assert!(settings.contains("\"trust\": true"));
    }

    #[test]
    fn writes_codex_skills() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = codex::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&"AGENTS.md".to_string()));
        assert!(files.contains(&".codex/config.toml".to_string()));
        assert!(files.iter().any(|f| f.contains(".agents/skills/")));

        // Verify SKILL.md format (no source-command- prefix; marker present)
        let skill = fs::read_to_string(tmp.path().join(".agents/skills/dev/SKILL.md")).unwrap();
        assert!(skill.starts_with("---\nname: dev\n"));
        assert!(skill.contains("description: |"));
        assert!(skill.contains("par-convert:generated"));
        assert!(skill.contains("Run the dev server."));

        // Skill name matches directory (path-slugged, prefix dropped)
        let skill_git =
            fs::read_to_string(tmp.path().join(".agents/skills/git-merge/SKILL.md")).unwrap();
        assert!(skill_git.contains("name: git-merge\n"));

        // No plugin system files
        assert!(!tmp.path().join(".agents/plugins").exists());

        // Codex config has MCP
        let toml = fs::read_to_string(tmp.path().join(".codex/config.toml")).unwrap();
        assert!(toml.contains("[mcp_servers.my_tool]"));
        assert!(toml.contains("project_doc_fallback_filenames"));
    }

    #[test]
    fn writes_antigravity_plugin() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = antigravity::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&"AGENTS.md".to_string()));
        assert!(files.iter().any(|f| f.contains("plugin.json")));
        assert!(files.iter().any(|f| f.contains("mcp_config.json")));
        assert!(files.iter().any(|f| f.contains("/commands/dev.md")));

        // No codex files
        assert!(!files.iter().any(|f| f.contains(".codex")));
        assert!(!files.iter().any(|f| f.contains(".agents/skills")));
    }

    #[test]
    fn writes_opencode_config() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = opencode::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&".opencode/config.json".to_string()));

        let json_str = fs::read_to_string(tmp.path().join(".opencode/config.json")).unwrap();
        assert!(json_str.contains("\"$schema\""));
        assert!(json_str.contains("\"mcp\""));
        assert!(json_str.contains("\"command\""));
        assert!(json_str.contains("dev"));
        assert!(json_str.contains("git:merge"));
    }

    #[test]
    fn writes_cursor_rules() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = cursor::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&".cursor/rules/instructions.mdc".to_string()));
        assert!(files.contains(&".cursor/mcp.json".to_string()));
        assert!(files
            .iter()
            .any(|f| f.ends_with(".mdc") && f.contains("commands/")));

        let instructions =
            fs::read_to_string(tmp.path().join(".cursor/rules/instructions.mdc")).unwrap();
        assert!(instructions.contains("alwaysApply: true"));
        assert!(instructions.contains("Test Project"));

        let cmd_rule =
            fs::read_to_string(tmp.path().join(".cursor/rules/commands/dev.mdc")).unwrap();
        assert!(cmd_rule.contains("alwaysApply: false"));
        assert!(cmd_rule.contains("/dev:"));

        let mcp = fs::read_to_string(tmp.path().join(".cursor/mcp.json")).unwrap();
        assert!(mcp.contains("my-tool"));
    }

    #[test]
    fn writes_kimi_skills_and_mcp() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = kimi::write(tmp.path(), &config, false).unwrap();
        assert!(files.contains(&"AGENTS.md".to_string()));
        assert!(files.contains(&".kimi/mcp.json".to_string()));
        assert!(files.iter().any(|f| f.contains(".kimi/skills/")));

        // No KIMI.md
        assert!(!tmp.path().join("KIMI.md").exists());

        let skill = fs::read_to_string(tmp.path().join(".kimi/skills/dev/SKILL.md")).unwrap();
        assert!(skill.contains("name: dev\n"));
        assert!(skill.contains("description: |"));
        assert!(skill.contains("par-convert:generated"));
    }

    #[test]
    fn dry_run_creates_no_files() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let files = gemini::write(tmp.path(), &config, true).unwrap();
        assert!(files.iter().all(|f| f.starts_with("(dry-run)")));
        assert!(!tmp.path().join("GEMINI.md").exists());
        assert!(!tmp.path().join(".gemini").exists());
    }

    #[test]
    fn resolve_targets_excludes_source() {
        let targets = resolve_targets(&ConvertTarget::All, "claude").unwrap();
        assert!(targets.contains(&"gemini".to_string()));
        assert!(targets.contains(&"codex".to_string()));
        assert!(!targets.contains(&"claude".to_string()));
    }

    #[test]
    fn validates_unknown_target() {
        let result = validate_target("unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown target"));
    }

    #[test]
    fn validates_unsupported_source() {
        let result = validate_source("gemini");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not yet implemented"));
    }

    #[test]
    fn empty_project_produces_minimal_output() {
        let tmp = temp_dir();
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        let config = claude::read(tmp.path()).unwrap();
        assert_eq!(config.commands.len(), 0);
        assert_eq!(config.mcp_servers.len(), 0);

        let codex_files = codex::write(tmp.path(), &config, false).unwrap();
        assert!(codex_files.contains(&"AGENTS.md".to_string()));
        assert!(codex_files.contains(&".codex/config.toml".to_string()));
        // No skills generated
        assert!(!codex_files.iter().any(|f| f.contains("SKILL.md")));
    }

    #[test]
    fn self_contained_includes_refs_and_skills() {
        let tmp = temp_dir();
        setup_project(tmp.path());
        let config = claude::read(tmp.path()).unwrap();

        let content = config.build_self_contained();
        assert!(content.contains("Test Project"));
        assert!(content.contains("Appendix: Reference Files"));
        assert!(content.contains("Reference: ports.md"));
        assert!(content.contains("Frontend: 3000"));
        assert!(content.contains("Appendix: Skills"));
        assert!(content.contains("Skill: sdk-regen.md"));
        assert!(content.contains("Regenerate the SDK"));
    }
}
