pub(crate) struct ProjectConfig {
    pub name: String,
    pub instructions: Option<String>,
    pub commands: Vec<CommandFile>,
    pub skills: Vec<ContentFile>,
    pub references: Vec<ContentFile>,
    pub mcp_servers: Vec<(String, McpServer)>,
}

pub(crate) struct CommandFile {
    pub rel_path: String,
    pub name: String,
    pub description: String,
    pub body: String,
}

pub(crate) struct ContentFile {
    pub filename: String,
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
    pub(crate) fn from_path_and_body(rel_path: String, body: String) -> Self {
        let name = rel_path.trim_end_matches(".md").to_string();
        let description = extract_description(&body, &name);
        Self {
            rel_path,
            name,
            description,
            body,
        }
    }
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
