//! Cross-reference resolution for converted command/skill packs.
//!
//! Claude command bodies reference other units four ways:
//!   - command   `/autonomous/auto-pm`
//!   - skill     `Run **checkpoint** skill`
//!   - persona   `.claude/agents/personas/developer.md`
//!   - reference `.claude/references/context.md`
//!
//! The converter keeps `.claude/` as the source of truth, so it does not rewrite
//! these in place. Instead it *resolves* every reference against the project's
//! symbol table and reports any that dead-end, so a typo'd command or a missing
//! skill fails the convert instead of silently shipping a broken pack.

use std::collections::BTreeSet;

use super::project::ProjectConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefKind {
    Command,
    Skill,
    Persona,
    Reference,
}

impl RefKind {
    fn label(self) -> &'static str {
        match self {
            RefKind::Command => "command",
            RefKind::Skill => "skill",
            RefKind::Persona => "persona",
            RefKind::Reference => "reference",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unresolved {
    pub source: String,
    pub kind: RefKind,
    pub raw: String,
}

/// The set of unit identifiers a project exposes, used to tell an intended unit
/// reference apart from an unrelated path or slash token.
pub(crate) struct Symbols {
    commands: BTreeSet<String>,
    command_ns: BTreeSet<String>,
    skills: BTreeSet<String>,
    personas: BTreeSet<String>,
    references: BTreeSet<String>,
}

impl Symbols {
    pub(crate) fn from_config(config: &ProjectConfig) -> Self {
        let commands: BTreeSet<String> = config.commands.iter().map(|c| c.name.clone()).collect();
        let command_ns: BTreeSet<String> = config
            .commands
            .iter()
            .filter_map(|c| c.name.split('/').next().filter(|_| c.name.contains('/')))
            .map(|s| s.to_string())
            .collect();
        let skills: BTreeSet<String> = config.skills.iter().map(|s| s.name.clone()).collect();
        let personas: BTreeSet<String> = config.personas.iter().map(|p| p.name.clone()).collect();
        let references: BTreeSet<String> = config
            .references
            .iter()
            .map(|r| r.filename.clone())
            .collect();
        Self {
            commands,
            command_ns,
            skills,
            personas,
            references,
        }
    }
}

/// Resolve every cross-reference across all command and skill bodies. Returns
/// `(resolved_count, unresolved)`.
pub(crate) fn resolve_project(config: &ProjectConfig) -> (usize, Vec<Unresolved>) {
    let symbols = Symbols::from_config(config);
    let mut resolved = 0usize;
    let mut unresolved = Vec::new();

    let mut visit = |label: &str, body: &str| {
        let (ok, bad) = scan_body(label, body, &symbols);
        resolved += ok;
        unresolved.extend(bad);
    };

    for cmd in &config.commands {
        visit(&cmd.rel_path, &cmd.body);
    }
    for skill in &config.skills {
        visit(&format!("skills/{}", skill.filename), &skill.body);
    }

    (resolved, unresolved)
}

fn scan_body(source: &str, body: &str, symbols: &Symbols) -> (usize, Vec<Unresolved>) {
    let mut resolved = 0usize;
    let mut unresolved = Vec::new();
    let mut record = |kind: RefKind, raw: String, ok: bool| {
        if ok {
            resolved += 1;
        } else {
            unresolved.push(Unresolved {
                source: source.to_string(),
                kind,
                raw,
            });
        }
    };

    for raw in scan_command_refs(body) {
        let path = raw.trim_start_matches('/');
        if symbols.commands.contains(path) {
            record(RefKind::Command, raw.clone(), true);
        } else if looks_like_command_typo(path, symbols) {
            // Namespace matches a real command dir but the leaf doesn't resolve —
            // a probable typo (e.g. /autonomous/auto-pm-typo). Shell/device paths
            // like /dev/null are excluded by the kebab-leaf requirement.
            record(RefKind::Command, raw.clone(), false);
        }
    }

    for name in scan_skill_refs(body) {
        record(RefKind::Skill, name.clone(), symbols.skills.contains(&name));
    }

    for raw in scan_path_refs(body, ".claude/agents/") {
        let stem = path_stem(&raw);
        record(
            RefKind::Persona,
            raw.clone(),
            symbols.personas.contains(&stem),
        );
    }

    for raw in scan_path_refs(body, ".claude/references/") {
        let file = file_name(&raw);
        record(
            RefKind::Reference,
            raw.clone(),
            symbols.references.contains(&file),
        );
    }

    (resolved, unresolved)
}

fn first_segment(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// True when `path` shares a namespace with a real command but does not resolve,
/// and its leaf is a multi-word kebab token — the shape of an actual command
/// name. This deliberately ignores single-word leaves (e.g. `/dev/null`,
/// `/documentation/`) to avoid flagging shell/device/directory paths.
fn looks_like_command_typo(path: &str, symbols: &Symbols) -> bool {
    if !path.contains('/') {
        return false;
    }
    if !symbols.command_ns.contains(first_segment(path)) {
        return false;
    }
    let leaf = path.rsplit('/').next().unwrap_or("");
    !leaf.is_empty() && leaf.contains('-')
}

fn path_stem(raw: &str) -> String {
    file_name(raw).trim_end_matches(".md").to_string()
}

fn file_name(raw: &str) -> String {
    raw.rsplit('/').next().unwrap_or(raw).to_string()
}

/// Find `/segment/segment` slash tokens at a word boundary. Strips trailing
/// punctuation. Skips `//` (URLs) and tokens with no inner `/` segment of letters.
fn scan_command_refs(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && at_boundary(bytes, i) && bytes.get(i + 1) != Some(&b'/') {
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() && is_path_byte(bytes[j]) {
                j += 1;
            }
            let token = &body[start..j];
            let trimmed = token.trim_end_matches(['.', ',', ')']);
            // Require at least one inner letter segment so bare "/" or "/*" is skipped.
            if trimmed.len() > 1 && trimmed[1..].chars().any(|c| c.is_ascii_alphabetic()) {
                out.push(trimmed.to_string());
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn at_boundary(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    matches!(
        bytes[i - 1],
        b' ' | b'\t' | b'\n' | b'(' | b'`' | b'*' | b'"' | b'\''
    )
}

fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.')
}

/// Find `**name** skill` references (case-insensitive on the trailing word).
fn scan_skill_refs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("**") {
        let after_open = &rest[open + 2..];
        if let Some(close) = after_open.find("**") {
            let name = &after_open[..close];
            let tail = after_open[close + 2..].trim_start();
            let is_skill = tail
                .split(|c: char| !c.is_ascii_alphabetic())
                .next()
                .map(|w| w.eq_ignore_ascii_case("skill"))
                .unwrap_or(false);
            if is_skill
                && !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                out.push(name.to_string());
            }
            rest = &after_open[close + 2..];
        } else {
            break;
        }
    }
    out
}

/// Find tokens beginning with `prefix` and ending at `.md`.
fn scan_path_refs(body: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find(prefix) {
        let from = &rest[idx..];
        let end = from
            .find(".md")
            .map(|e| e + 3)
            .unwrap_or_else(|| token_end(from));
        out.push(from[..end].to_string());
        rest = &from[end..];
    }
    out
}

fn token_end(s: &str) -> usize {
    s.find(|c: char| c.is_whitespace() || c == '`' || c == ')' || c == '"')
        .unwrap_or(s.len())
}

/// Render the unresolved report for the convert summary.
pub(crate) fn format_report(resolved: usize, unresolved: &[Unresolved]) -> String {
    let mut out = format!("  resolved: {resolved} references");
    if unresolved.is_empty() {
        out.push_str("\n  unresolved: none");
        return out;
    }
    out.push_str(&format!("\n  UNRESOLVED ({}):", unresolved.len()));
    for u in unresolved {
        out.push_str(&format!(
            "\n    {} -> {} ({})",
            u.source,
            u.raw,
            u.kind.label()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols() -> Symbols {
        Symbols {
            commands: [
                "autonomous/auto-pm",
                "autonomous/auto-dev",
                "dev",
                "git/commit",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            command_ns: ["autonomous", "git"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            skills: ["checkpoint", "browser-smoke-test"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            personas: ["developer", "ux-designer"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            references: ["context.md", "config.md"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    #[test]
    fn resolves_valid_command_ref() {
        let (ok, bad) = scan_body("x", "Run /autonomous/auto-pm {JIRA_KEY} now.", &symbols());
        assert_eq!(ok, 1);
        assert!(bad.is_empty());
    }

    #[test]
    fn flags_typo_command_in_known_namespace() {
        let (_, bad) = scan_body(
            "auto-dev.md",
            "Run /autonomous/auto-pm-typo here.",
            &symbols(),
        );
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].kind, RefKind::Command);
        assert_eq!(bad[0].raw, "/autonomous/auto-pm-typo");
    }

    #[test]
    fn ignores_unrelated_slash_paths() {
        // /dashboard/* and apps/api and URLs must not be treated as command refs.
        let (ok, bad) = scan_body(
            "x",
            "Visit http://localhost:3000/dashboard/home and edit apps/api/main.ts and /dashboard/*",
            &symbols(),
        );
        assert_eq!(ok, 0);
        assert!(bad.is_empty());
    }

    #[test]
    fn resolves_and_flags_skill_refs() {
        let (ok, bad) = scan_body(
            "x",
            "Run **checkpoint** skill then run **missing-skill** skill.",
            &symbols(),
        );
        assert_eq!(ok, 1);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].kind, RefKind::Skill);
        assert_eq!(bad[0].raw, "missing-skill");
    }

    #[test]
    fn resolves_persona_and_reference_paths() {
        let (ok, bad) = scan_body(
            "x",
            "Read `.claude/agents/personas/developer.md` and `.claude/references/context.md`.",
            &symbols(),
        );
        assert_eq!(ok, 2);
        assert!(bad.is_empty());
    }

    #[test]
    fn flags_missing_persona_and_reference() {
        let (_, bad) = scan_body(
            "x",
            "Read .claude/agents/personas/ghost.md and .claude/references/missing.md",
            &symbols(),
        );
        assert_eq!(bad.len(), 2);
    }

    #[test]
    fn ignores_dev_null_and_dir_paths() {
        // /dev/null (device), /documentation/ (dir), bare /preview must not flag,
        // even though dev/documentation/preview are real command namespaces.
        let (ok, bad) = scan_body(
            "x",
            "Run `cmd 2>/dev/null` and `> /dev/null`. See /documentation/ and visit /preview here.",
            &symbols(),
        );
        assert_eq!(ok, 0);
        assert!(bad.is_empty(), "unexpected: {bad:?}");
    }

    #[test]
    fn bold_non_skill_is_ignored() {
        let (ok, bad) = scan_body("x", "This is **important** text, not a skill.", &symbols());
        assert_eq!(ok, 0);
        assert!(bad.is_empty());
    }
}
