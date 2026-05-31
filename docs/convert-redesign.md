# Convert Redesign — Porting Claude commands/skills to other harnesses

Status: in progress. Tracks the rework of `par convert` so complex Claude
command/skill packs (e.g. `../zipper/.claude`) port to Kimi / Codex / OpenCode /
Gemini / Cursor / Antigravity **without breaking their internal references**.

## Problem

Claude command packs are not independent prompt blobs. They form a graph:

- command → command  (`/autonomous/auto-pm {JIRA_KEY}`)
- command → skill     ("Run **checkpoint** skill")
- command → persona   (`.claude/agents/personas/developer.md`)
- command → reference (`.claude/references/context.md`)
- command → MCP tool  (`jira_get`, `browser_navigate`)

The old converter read each `.md` as an opaque `body`, renamed the unit per
target, and dumped the body verbatim. It never rewrote the references, never
read `.claude/agents/`, never parsed frontmatter, and inlined skills as
always-on appendix text. Result: ported workflows dead-end at the first hop.

See the findings table in the PR / analysis for severity detail. Headline bugs:

1. Cross-references never rewritten (renamed target ≠ name in body).
2. `.claude/agents/` (personas) dropped entirely.
3. Skills emitted only as appendix text, not as discoverable skill units.
4. Skill reader is shallow (no recursion into skill dirs + assets).
5. Frontmatter unparsed → wrong descriptions + leaked `model:`/`color:` into body.
6. Per-command `model:` hint dropped.
7. Argument placeholders (`{JIRA_KEY}`) never mapped to target arg syntax.
8. `truncate()` slices on byte offset → UTF-8 panic on emoji-bearing commands.
9. No broken-link validation; blunt directory deletes; one-directional only.

## Root cause

The data model (`ProjectConfig` = flat `Vec<body string>`) has no symbol table,
no reference graph, no parsed frontmatter, and no first-class skill/persona
units. Writers can only rename-and-dump.

## Target model

```
Unit {
  id:        canonical id, e.g. "command:autonomous/auto-pm", "skill:checkpoint"
  kind:      Command | Skill | Persona | Reference
  source:    rel path under .claude
  name:      from frontmatter or path
  description, model, tools, arg_hint   (parsed frontmatter)
  body:      frontmatter-stripped markdown
  args:      detected placeholders ({JIRA_KEY}, $1, $ARGUMENTS)
}

Project {
  name, instructions (CLAUDE.md), mcp_servers,
  commands: Vec<Unit>, skills: Vec<Unit>, personas: Vec<Unit>, references: Vec<Unit>,
  symbols:  map<reference-form, unit-id>   // built once
}
```

A `TargetScheme` per harness answers: given a unit id, what is its rendered name
and how does the model invoke it. The link rewriter walks each unit body,
resolves every reference against the symbol table, and rewrites it into the
target's invocation form — emitting an `unresolved` list for the convert report.

## Phases

- **P1 (this change set):** frontmatter parser; UTF-8-safe truncate; extend model
  with parsed fields + personas; recurse skill dirs. Keep writers compiling.
  Tests stay green.
- **P2:** symbol table + link rewriter; `TargetScheme` per harness; writers emit
  rewritten bodies. Convert report lists unresolved references.
- **P3:** skills as first-class units in every target; argument normalization;
  references kept as on-demand (not always-on) where the harness supports it.
- **P4:** safety — scoped marker-based deletes, `--dry-run` broken-link + clobber
  report, per-harness exclude policy.

## Non-goals (for now)

- Reverse conversion (other → claude).
- Honoring per-command `model:` at runtime (captured, surfaced, not enforced).
