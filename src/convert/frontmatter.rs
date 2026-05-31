use std::collections::BTreeMap;

/// Parsed YAML-ish frontmatter plus the body with the frontmatter block removed.
///
/// Only the small subset Claude command/skill/agent files use is supported:
/// a leading `---` fence, `key: value` scalar lines, and `key: a, b, c` lists.
/// Anything more exotic is left in `extra` untouched so nothing is silently lost.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Frontmatter {
    pub fields: BTreeMap<String, String>,
}

impl Frontmatter {
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|s| s.as_str())
    }

    /// Comma-separated value parsed into a trimmed, non-empty list.
    pub(crate) fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            None => Vec::new(),
            Some(raw) => raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}

/// Split a markdown document into its frontmatter and the remaining body.
///
/// When there is no leading `---` fence the frontmatter is empty and the body is
/// the input unchanged. The returned body has the fence removed and leading blank
/// lines trimmed, so writers never re-embed a stray frontmatter block.
pub(crate) fn split(input: &str) -> (Frontmatter, String) {
    let trimmed_start = input.trim_start_matches('\u{feff}');
    if !starts_with_fence(trimmed_start) {
        return (Frontmatter::default(), input.to_string());
    }

    // Find the closing fence. The opening fence is the first line.
    let after_open = &trimmed_start[first_line_len(trimmed_start)..];
    let mut fields = BTreeMap::new();
    let mut consumed = 0usize;
    let mut closed = false;

    for line in after_open.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        consumed += line.len();
        if is_fence(content) {
            closed = true;
            break;
        }
        if let Some((key, value)) = parse_field(content) {
            fields.insert(key, value);
        }
    }

    if !closed {
        // Malformed (no closing fence): treat whole input as body, parse nothing.
        return (Frontmatter::default(), input.to_string());
    }

    let body = after_open[consumed..].trim_start_matches('\n').to_string();
    (Frontmatter { fields }, body)
}

fn starts_with_fence(s: &str) -> bool {
    let first = s.lines().next().unwrap_or("");
    is_fence(first)
}

fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

fn first_line_len(s: &str) -> usize {
    match s.find('\n') {
        Some(idx) => idx + 1,
        None => s.len(),
    }
}

fn parse_field(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let value = value.trim().trim_matches('"').trim_matches('\'').trim();
    Some((key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_frontmatter() {
        let input = "---\nname: auto-dev\ndescription: Full autonomous workflow\nmodel: sonnet\ncolor: green\n---\n\n# Auto Dev\n\nBody here.\n";
        let (fm, body) = split(input);
        assert_eq!(fm.get("name"), Some("auto-dev"));
        assert_eq!(fm.get("description"), Some("Full autonomous workflow"));
        assert_eq!(fm.get("model"), Some("sonnet"));
        assert!(body.starts_with("# Auto Dev"));
        assert!(!body.contains("model: sonnet"));
    }

    #[test]
    fn parses_tools_list() {
        let input = "---\ntools: Glob, Grep, Read, Write\n---\nbody\n";
        let (fm, _) = split(input);
        assert_eq!(fm.list("tools"), vec!["Glob", "Grep", "Read", "Write"]);
    }

    #[test]
    fn no_frontmatter_returns_body_unchanged() {
        let input = "# Just a heading\n\nNo frontmatter.\n";
        let (fm, body) = split(input);
        assert!(fm.fields.is_empty());
        assert_eq!(body, input);
    }

    #[test]
    fn unterminated_frontmatter_is_left_as_body() {
        let input = "---\nname: broken\n\n# never closed\n";
        let (fm, body) = split(input);
        assert!(fm.fields.is_empty());
        assert_eq!(body, input);
    }

    #[test]
    fn strips_quotes_from_values() {
        let input = "---\ndescription: \"quoted value\"\n---\nx\n";
        let (fm, _) = split(input);
        assert_eq!(fm.get("description"), Some("quoted value"));
    }
}
