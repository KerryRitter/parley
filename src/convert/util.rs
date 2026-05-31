/// UTF-8-safe truncation. The old implementation sliced on a byte offset and
/// panicked when the cut landed inside a multibyte char (e.g. emoji in the
/// `spur` commands). This counts characters and never splits a grapheme byte.
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    if max_chars <= 3 {
        return s.chars().take(max_chars).collect();
    }
    let kept: String = s.chars().take(max_chars - 3).collect();
    format!("{}...", kept.trim_end())
}

/// Lowercase kebab slug: alphanumerics kept, every other run collapsed to a
/// single `-`, no leading/trailing `-`.
pub(crate) fn slugify(value: &str) -> String {
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

/// Marker embedded (as a YAML comment) in every skill `par convert` generates so
/// re-converting can delete only its own output and never a hand-authored skill.
pub(crate) const GENERATED_MARKER: &str = "par-convert:generated";

/// Build a skill directory name from a command path, capped at 64 chars to
/// satisfy the agent-skill name limit. Path separators collapse into `-` so
/// `autonomous/auto-dev` becomes `autonomous-auto-dev` (unique across the tree).
pub(crate) fn skill_dir_name(name: &str) -> String {
    let raw = slugify(name);
    if raw.len() <= 64 {
        raw
    } else {
        raw[..64].trim_end_matches('-').to_string()
    }
}

/// Remove only the skill subdirectories that `par convert` previously generated,
/// identified by [`GENERATED_MARKER`] in their `SKILL.md`. Hand-authored skills
/// (without the marker) are left untouched.
pub(crate) fn clean_generated_skills(skills_dir: &std::path::Path) -> Result<(), String> {
    if !skills_dir.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(skills_dir)
        .map_err(|e| format!("failed to read {}: {e}", skills_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry error: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        let is_generated = std::fs::read_to_string(&skill_md)
            .map(|c| c.contains(GENERATED_MARKER))
            .unwrap_or(false);
        if is_generated {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 240), "hello");
    }

    #[test]
    fn truncate_does_not_panic_on_emoji() {
        let s = "⚖️ The Spur Standard ".repeat(40);
        let out = truncate(&s, 50);
        assert!(out.chars().count() <= 50);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        // 10 emoji = 10 chars but many bytes; must keep <= 8 chars.
        let s = "🚀".repeat(10);
        let out = truncate(&s, 8);
        assert!(out.chars().count() <= 8);
    }

    #[test]
    fn slugify_collapses_separators() {
        assert_eq!(slugify("autonomous/auto-pm"), "autonomous-auto-pm");
        assert_eq!(slugify("Git: Merge Branch"), "git-merge-branch");
    }

    #[test]
    fn skill_dir_name_drops_prefix_and_caps_length() {
        assert_eq!(skill_dir_name("autonomous/auto-dev"), "autonomous-auto-dev");
        assert_eq!(skill_dir_name("dev"), "dev");
        let long = "a".repeat(100);
        assert!(skill_dir_name(&long).len() <= 64);
    }
}
