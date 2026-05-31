use super::frontmatter;

pub(crate) struct ProjectConfig {
    pub name: String,
    pub instructions: Option<String>,
    pub commands: Vec<CommandFile>,
    pub skills: Vec<ContentFile>,
    pub references: Vec<ContentFile>,
    pub personas: Vec<PersonaFile>,
    pub mcp_servers: Vec<(String, McpServer)>,
}

pub(crate) struct CommandFile {
    pub rel_path: String,
    pub name: String,
    pub description: String,
    /// Per-command model hint from frontmatter (`model:`), if any.
    pub model: Option<String>,
    /// Argument placeholders detected in the body (e.g. `{JIRA_KEY}`, `$1`).
    pub args: Vec<String>,
    /// Body with the frontmatter block stripped.
    pub body: String,
}

pub(crate) struct ContentFile {
    pub filename: String,
    /// Skill/reference name (frontmatter `name:` or filename stem).
    pub name: String,
    pub description: String,
    /// Body with the frontmatter block stripped.
    pub body: String,
}

pub(crate) struct PersonaFile {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub body: String,
}

pub(crate) struct McpServer {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
    pub disabled: bool,
}

impl ProjectConfig {
    pub(crate) fn build_self_contained(&self) -> String {
        let mut out = String::new();

        if let Some(instructions) = &self.instructions {
            out.push_str(instructions.trim_end());
            out.push('\n');
        }

        if !self.references.is_empty() {
            out.push_str("\n---\n\n# Appendix: Reference Files\n\n");
            out.push_str(
                "The following reference data is included inline for CLIs that cannot read .claude/ files.\n",
            );
            for reference in &self.references {
                out.push_str("\n---\n\n");
                out.push_str(&format!("## Reference: {}\n\n", reference.filename));
                out.push_str(reference.body.trim_end());
                out.push('\n');
            }
        }

        if !self.skills.is_empty() {
            out.push_str("\n---\n\n# Appendix: Skills\n\n");
            out.push_str(
                "Reusable procedures. When instructions say 'Run **X** skill', follow the procedure below.\n",
            );
            for skill in &self.skills {
                out.push_str("\n---\n\n");
                out.push_str(&format!("## Skill: {}\n\n", skill.filename));
                out.push_str(skill.body.trim_end());
                out.push('\n');
            }
        }

        out
    }
}

impl CommandFile {
    pub(crate) fn from_path_and_body(rel_path: String, raw: String) -> Self {
        let (fm, body) = frontmatter::split(&raw);
        let name = rel_path.trim_end_matches(".md").to_string();
        let description = fm
            .get("description")
            .map(str::to_string)
            .unwrap_or_else(|| extract_description(&body, &name));
        let model = fm.get("model").map(str::to_string);
        let args = detect_args(&body);
        Self {
            rel_path,
            name,
            description,
            model,
            args,
            body,
        }
    }
}

impl ContentFile {
    pub(crate) fn from_filename_and_body(filename: String, raw: String) -> Self {
        let (fm, body) = frontmatter::split(&raw);
        let stem = filename.trim_end_matches(".md").to_string();
        let name = fm.get("name").map(str::to_string).unwrap_or(stem.clone());
        let description = fm
            .get("description")
            .map(str::to_string)
            .unwrap_or_else(|| extract_description(&body, &stem));
        Self {
            filename,
            name,
            description,
            body,
        }
    }
}

impl PersonaFile {
    pub(crate) fn from_filename_and_body(filename: String, raw: String) -> Self {
        let (fm, body) = frontmatter::split(&raw);
        let stem = filename.trim_end_matches(".md").to_string();
        let name = fm.get("name").map(str::to_string).unwrap_or(stem.clone());
        let description = fm
            .get("description")
            .map(str::to_string)
            .unwrap_or_else(|| extract_description(&body, &stem));
        Self {
            name,
            description,
            model: fm.get("model").map(str::to_string),
            tools: fm.list("tools"),
            body,
        }
    }
}

/// Detect argument placeholders used in a command body. Supports `{NAME}` brace
/// style (the zipper convention), positional `$1`..`$9`, and `$ARGUMENTS`.
fn detect_args(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut push = |s: String| {
        if !found.contains(&s) {
            found.push(s);
        }
    };

    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if let Some(end) = body[i + 1..].find('}') {
                    let inner = &body[i + 1..i + 1 + end];
                    if is_placeholder_name(inner) {
                        push(format!("{{{inner}}}"));
                    }
                    i = i + 1 + end + 1;
                    continue;
                }
            }
            b'$' => {
                let rest = &body[i + 1..];
                if let Some(stripped) = rest.strip_prefix("ARGUMENTS") {
                    let _ = stripped;
                    push("$ARGUMENTS".to_string());
                    i += "$ARGUMENTS".len();
                    continue;
                }
                if let Some(d) = rest.chars().next().filter(|c| c.is_ascii_digit()) {
                    push(format!("${d}"));
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// A brace placeholder name: non-empty, uppercase/underscore/digits only. Keeps
/// us from matching JSON/code blocks like `{"key": ...}` or `{ mkdir }`.
fn is_placeholder_name(inner: &str) -> bool {
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
}

fn extract_description(body: &str, fallback: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(after) = trimmed.strip_prefix("# ") {
            let desc = after
                .split(" - ")
                .nth(1)
                .map(|s| s.trim())
                .unwrap_or(after.trim());
            let desc = desc.chars().take(60).collect::<String>();
            if !desc.is_empty() {
                return desc;
            }
        }
    }
    fallback.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_uses_frontmatter_description() {
        let raw = "---\nname: auto-dev\ndescription: Full autonomous workflow\nmodel: sonnet\n---\n\n# Auto Dev Autonomous\n\nProcess ticket {JIRA_KEY}.\n".to_string();
        let cmd = CommandFile::from_path_and_body("autonomous/auto-dev.md".to_string(), raw);
        assert_eq!(cmd.description, "Full autonomous workflow");
        assert_eq!(cmd.model.as_deref(), Some("sonnet"));
        assert_eq!(cmd.args, vec!["{JIRA_KEY}"]);
        assert!(!cmd.body.contains("model: sonnet"));
    }

    #[test]
    fn command_without_frontmatter_falls_back_to_heading() {
        let raw = "# /dev - Start development\n\nRun the dev server.\n".to_string();
        let cmd = CommandFile::from_path_and_body("dev.md".to_string(), raw);
        assert_eq!(cmd.description, "Start development");
        assert!(cmd.model.is_none());
    }

    #[test]
    fn detect_args_ignores_json_blocks() {
        let body = "Write {\"phase\": \"x\"} and process {JIRA_KEY} with $1 and $ARGUMENTS.";
        let args = detect_args(body);
        assert!(args.contains(&"{JIRA_KEY}".to_string()));
        assert!(args.contains(&"$1".to_string()));
        assert!(args.contains(&"$ARGUMENTS".to_string()));
        assert!(!args.iter().any(|a| a.contains("phase")));
    }

    #[test]
    fn persona_parses_tools_and_model() {
        let raw = "---\nname: developer\ndescription: Senior dev\ntools: Glob, Grep, Read\nmodel: sonnet\n---\n\n# Developer Persona\n\nYou are.\n".to_string();
        let p = PersonaFile::from_filename_and_body("developer.md".to_string(), raw);
        assert_eq!(p.name, "developer");
        assert_eq!(p.tools, vec!["Glob", "Grep", "Read"]);
        assert_eq!(p.model.as_deref(), Some("sonnet"));
    }
}
