# par — Programmatic Agent Router

**One prompt interface for every AI coding agent.**

`par` is a small, dependency-free Rust CLI. You write `par -p "do the thing"`, and it routes the call to whichever local agent CLI you choose — Claude, Codex, Cursor, Gemini, Goose, OpenCode, Qwen, Aider, Amazon Q, Copilot, Kimi, or Antigravity.

```sh
par -p "review this repository"                              # uses your default agent (claude)
par -h co -m gpt-5.4 -p "fix the failing tests"             # codex
git diff | par -h oc --provider anthropic -p "review this"  # opencode, with piped context
```

It never calls a model API itself. It starts an already-installed agent CLI, translates shared flags into that agent's command surface, streams its output, and exits with its status.

### Why

Every agent CLI has a different headless interface — `claude -p`, `codex exec`, `gemini --prompt`, `goose run -t`, `opencode run`, `aider --message`. Switching agents means rewriting commands, model flags, provider syntax, and env vars. `par` keeps your scripts focused on intent and lets the adapter own the translation. Switch agents by changing one flag.

---

## Install

One command:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh
```

This installs `par` into `~/.local/bin` (plus an `agent-router` alias). Make sure that directory is on your `PATH`:

```sh
export PATH="$HOME/.local/bin:$PATH"
par --version          # verify
```

> `par` routes to agent CLIs — it does not install them for you automatically. Install the agents you want with [`par install`](#install-agent-clis), or bring your own.

### Other install methods

**From source** (needs [Rust](https://sh.rustup.rs)):

```sh
git clone https://github.com/KerryRitter/programmatic-agent-router.git
cd programmatic-agent-router
scripts/setup.sh --install        # validates, then `cargo install --path . --force`
```

This creates `~/.local/bin/par` and `~/.local/bin/agent-router -> par`. Ensure both `~/.cargo/bin` and `~/.local/bin` are on your `PATH`.

**Direct from GitHub:**

```sh
cargo install --git https://github.com/KerryRitter/programmatic-agent-router.git --branch main --force
```

**Install-script options** (append after `sh -s --`):

```sh
... | sh -s -- --install-dir /usr/local/bin   # custom location
... | sh -s -- --from-source                  # force a source build
... | sh -s -- --from-source --git-protocol ssh
... | sh -s -- --no-agent-router              # skip the agent-router alias
```

The script installs a prebuilt release binary for your platform when available, and falls back to a source build otherwise. (Release-binary and Homebrew distribution are planned; see [Release Plan](#release-plan).)

---

## What par can do

| Command | Capability |
| --- | --- |
| [`par -p "..."`](#run-a-prompt) | Route a prompt to any agent, with one shared flag set |
| [`par default <agent>`](#set-a-default-agent) | Pick the default agent (and options) for this machine |
| [`par install <agent>`](#install-agent-clis) | Install a downstream agent CLI |
| [`par shims install`](#shims) | Create `claudey` / `codexy` one-shot shortcuts |
| [`par convert`](#convert--share-one-config-across-agents) | Port your `.claude/` config to every other agent |
| [`par resume`](#resume--continue-a-past-session-from-any-agent) | Browse & resume past sessions across all agents, scoped to the folder |
| [`par ask`](#ask--one-agent-talks-to-another) | Ask another agent headless, optionally seeded with a prior session's context |
| [`par mcp`](#mcp--expose-resume-and-ask-to-other-agents) | Run an MCP server so other agents can resume sessions and ask each other |

---

## Run a prompt

```sh
par                                    # launch the default agent interactively
par -p "summarize this repository"     # one-shot prompt
par -h co -m gpt-5.4 -p "add tests"    # choose an agent + model
git diff | par -p "review this patch"  # pipe context in
par -p "review src" --dry-run          # print the routed command instead of running it
```

**Choosing the agent** — `-h` / `--harness` takes a full name or a short code:

| Code | Agent | Code | Agent | Code | Agent |
| --- | --- | --- | --- | --- | --- |
| `cl` | claude | `g` | gemini | `q` | qwen |
| `co` | codex | `go` | goose | `a` / `ai` | aider |
| `cu` | cursor | `oc` | opencode | `aq` | amazon-q |
| `cp` | copilot | `k` | kimi | `ag` | antigravity |

```sh
par -h cl "review this"
par -h co -m gpt-5.4 "fix tests"
par -h k -p "drain the queue"
```

> Bare `-h` (no value) prints help; `-h <name>` selects an agent.

### Flags

| Flag | Purpose |
| --- | --- |
| `-p`, `--prompt`, `--print` | Prompt text (`--print` is accepted for Claude compatibility). |
| `-h`, `--harness <name>` | Target agent. Defaults to `claude`. Accepts short codes. |
| `--provider <name>` | Provider namespace, when the agent uses provider-qualified models or env config. |
| `-m`, `--model <name>` | Model name. |
| `--agent <name>` | Agent/persona/profile, where supported. |
| `--output-format <fmt>` | Output mode (e.g. `json`), where supported. |
| `--input-format <fmt>` | Claude-compatible input format. |
| `--permission-mode <mode>` | Permission/sandbox mode, where supported. |
| `--max-turns <n>` | Max agent turns, where supported. |
| `--cwd <path>` | Working directory for the child process. |
| `--yolo` / `--no-yolo` | Add / skip the agent's permission-bypass flag. **On by default.** |
| `--dry-run` | Print the routed invocation as JSON; run nothing. |
| `--version`, `-v` | Print version. |
| `--help` | Print help. |
| `--` | Pass everything after it straight to the agent CLI. |

**Pass agent-specific flags** after `--`:

```sh
par -h cl -p "review this" -- --verbose
par -h aider -p "fix lint" -- --yes --no-auto-commits
```

**How prompt input is resolved:**

- No prompt and no stdin → launch the agent's interactive entrypoint.
- stdin piped **and** `-p` given → stdin is placed before the prompt, blank line between.
- stdin piped, no `-p` → stdin becomes the prompt.

### Yolo (permission bypass) is on by default

Every run adds the agent's permission-bypass flag (e.g. `--dangerously-skip-permissions` for Claude) so automation runs hands-off. Opt out per run with `--no-yolo`, or persistently with `AGENT_ROUTER_YOLO=false`. Agents with no known bypass flag (e.g. Amazon Q) simply run without one. **Opt out when running untrusted prompts or in sensitive directories.**

### Set a default agent

So you don't repeat `-h` every time:

```sh
par default codex            # set default agent
par default claude --yolo    # set agent + persist yolo
par default --no-yolo        # keep agent, disable yolo
par default                  # show current defaults
par default --path           # print the config file path
par current                  # alias for showing defaults
par list                     # list supported agent names
```

The default lives in `~/.config/par/default` (or `$XDG_CONFIG_HOME/par/default`; override with `PAR_DEFAULT_FILE`).

### Environment defaults

```sh
export AGENT_ROUTER_HARNESS=codex
export AGENT_ROUTER_PROVIDER=openai
export AGENT_ROUTER_MODEL=gpt-5.4
export AGENT_ROUTER_YOLO=true
```

### Shims

Generate `*y` one-shot shortcuts for yolo-capable agents:

```sh
par shims install            # writes claudey, codexy, ... to ~/.local/bin
claudey -p "work in this sandbox"
codexy "work in this sandbox"
```

Override the location with `par shims install --dir <dir>` or `PAR_SHIM_DIR`. `par shims list` prints the generated names and commands.

---

## Install agent CLIs

`par` can install the downstream agents it routes to:

```sh
par install list             # show installer coverage
par install claude           # install one agent
par install all              # install every supported agent
par install --dry-run all    # print the exact upstream commands, run nothing
```

The registry is transparent — `--dry-run` prints the real upstream install command. Agents without a stable one-liner (e.g. Amazon Q) print the official install page and a verify command instead of guessing.

| Agent | Installer |
| --- | --- |
| `claude` | `curl -fsSL https://claude.ai/install.sh \| bash` |
| `codex` | `npm install -g @openai/codex` |
| `cursor` | `curl https://cursor.com/install -fsS \| bash` |
| `gemini` | `npm install -g @google/gemini-cli` |
| `goose` | `curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh \| bash` |
| `opencode` | `curl -fsSL https://opencode.ai/install \| bash` |
| `qwen` | `curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen.sh \| bash` |
| `aider` | `curl -LsSf https://aider.chat/install.sh \| sh` |
| `amazon-q` | Manual official installer page; verify with `q --version` |
| `copilot` | `curl -fsSL https://gh.io/copilot-install \| bash` |
| `kimi` | `curl -LsSf https://code.kimi.com/install.sh \| bash` |
| `antigravity` | `curl -fsSL https://antigravity.google/cli/install.sh \| bash` |

> `par` does not manage agent versions; each downstream CLI owns its own upgrade flow.

---

## Convert — share one config across agents

`par convert` ports a Claude command/skill pack to other agents. Your `.claude/` directory (commands, skills, `agents/`, references), `CLAUDE.md`, and `.mcp.json` stay the **single source of truth**; convert generates each agent's native config from them.

```sh
par convert                       # claude -> all targets
par convert --to kimi             # claude -> one target
par convert --from claude --to codex
par convert --dry-run             # show what would be written
par convert --cwd path/to/project
```

**Source:** `claude`. **Targets:** `gemini`, `codex`, `antigravity`, `opencode`, `cursor`, `kimi`.

What it does:

- **Parses frontmatter** — real descriptions, per-command `model`, argument placeholders; strips the block from bodies.
- **Reads** commands, skills, personas (`.claude/agents/`), references, and `.mcp.json`.
- **Emits native artifacts per target** — e.g. `.kimi/skills/<name>/SKILL.md` + `.kimi/mcp.json`, `.codex/config.toml` + `.agents/skills/`, `.gemini/commands/*.toml` + `GEMINI.md`, `.cursor/rules/`, `.opencode/config.json`, plus `AGENTS.md`. MCP servers in `.mcp.json` are translated into each agent's format.
- **Resolves cross-references** — every `/command`, `**skill** skill`, persona path, and reference path is checked against the pack. The run prints a resolution report and **exits non-zero if any reference dead-ends**, so a typo fails the convert instead of shipping a broken pack.

Generated skills carry a `par-convert:generated` marker, so re-running replaces only its own output and never a hand-authored file. Commit `.claude/`; git-ignore the generated output. A typical `npm run sync:instructions` is just `par convert --from claude --to all`.

---

## Resume — continue a past session from any agent

`par resume` browses and resumes sessions from **any** agent, scoped to the current directory — the same scoping every agent's own `--resume` uses. It reads the transcripts each agent already writes to disk; there are no extra files to maintain.

```sh
par resume                      # list this folder's sessions (any agent), pick one
par resume -h cl                # resume a claude session here (picker if several)
par resume -h co --latest       # resume the newest codex session, no prompt
par resume --list               # print the listing, resume nothing
par resume --list --json        # machine-readable listing
par resume -h cl <id> --print   # print the resume command for a session id
par resume --cwd path/to/proj   # scope to another directory
```

A selector is either a list index (`par resume 2`) or a raw session id (`par resume -h cl <id>`). Add `--yolo` to append the agent's permission-bypass flag (off by default here, since resume drops into an interactive session).

**Two tiers of support:**

- **Native listing** — `claude`, `codex`, `opencode`. Read straight from disk (`~/.claude/projects/<slug>/`, `~/.codex/sessions/`, `~/.local/share/opencode/storage/session/`), matched on exact cwd, with title and recency. These show up in the cross-agent listing.
- **Delegate resume** — `cursor`, `gemini`. Their stores are hash-scoped in a way `par` doesn't reproduce, but the binaries self-scope to the cwd. Listing is skipped (marked `~`); resume runs the agent's own cwd-scoped resume (`cursor-agent resume`, `gemini --resume latest`). `par resume -h cu` / `par resume -h g` work directly.

---

## Ask — one agent talks to another

`par ask` runs another agent **headless** and returns its reply as text. Because `par` already routes a prompt to any agent, "Claude asks Gemini" is just routing the prompt to Gemini and capturing its output. With `--context-from`, `par` first reads a prior session's transcript and prepends it — so the answer is informed by that history. This is the cross-agent **context bridge**.

```sh
par ask -h g -p "critique this approach in 3 bullets"        # ask gemini, print its reply
par ask -h g -p "what did we decide?" --context-from cl      # seed with your latest claude session here
par ask -h cl -p "continue this" --context-from co:<id>      # use a specific source session id
par ask -h g -p "..." --max-context 8000                     # cap injected context (default 12000 chars)
par ask -h g -p "..." --dry-run                              # show the routed command + final prompt, run nothing
```

`--context-from` takes `harness[:session]`; omit the session (or use `latest`) for the newest in the directory. **Context sources:** `claude`, `codex`, `opencode` (full transcripts). `cursor` / `gemini` can't export transcripts, so they can't be context *sources* — they can still be asked.

Notes: each call is one-shot (the target keeps no memory between asks); yolo is on by default so the headless agent can't block on a permission prompt; long transcripts are truncated to the most recent turns within the budget.

## MCP — expose resume and ask to other agents

`par mcp` runs a small [MCP](https://modelcontextprotocol.io) server over stdio (newline-delimited JSON-RPC 2.0), so any MCP-capable agent can resume sessions in the current directory **and** ask other agents questions. This is how you say *"pick up my last conversation from Claude"* — or *"ask Gemini to review this, with my Claude context"* — from inside another agent.

**Tools:**

- `list_sessions {cwd?, harness?}` — resumable sessions for a directory, newest first.
- `get_last_session {cwd?, harness?}` — the most recent session plus a ready-to-run resume command.
- `resume_command {harness, id, cwd?, yolo?}` — build the native resume command for a session id (text; never spawns an interactive agent).
- `ask_agent {harness, prompt, model?, provider?, cwd?, context_from?: {harness, session?}}` — run another agent headless and return its reply, optionally seeded with a session transcript. The agent-to-agent / context-bridge primitive.

**Register it into an agent** — `par mcp connect` runs whatever that agent needs:

```sh
par mcp connect -h cl           # claude   -> runs `claude mcp add -s user par -- <par> mcp`
par mcp connect -h co           # codex    -> runs `codex mcp add par -- <par> mcp`
par mcp connect -h g            # gemini   -> runs `gemini mcp add par <par> mcp`
par mcp connect -h oc           # opencode -> opens its own interactive `opencode mcp add`
par mcp connect -h cu           # cursor   -> merges ~/.cursor/mcp.json (no add command exists)
par mcp connect -h cl --dry-run # show the exact command / file change, do nothing
```

`connect` registers the absolute path of the running `par`, so it works regardless of the caller's `PATH`. Agents with a native `mcp add` are invoked directly (some, like opencode, prompt in their own TUI); cursor has no add subcommand, so its config file is merged in place, preserving existing servers.

Then, from any registered agent: *"use par to pick up my last claude session here"* → the agent calls `get_last_session` and runs the returned `claude --resume <id>`.

---

## Supported agents

| Agent | Aliases | Routed command |
| --- | --- | --- |
| `claude` | `cl` | `claude -p "<prompt>"` |
| `codex` | `co`, `openai` | `codex exec "<prompt>"` |
| `cursor` | `cu`, `cursor-agent` | `cursor-agent -p "<prompt>"` |
| `gemini` | `g`, `google`, `google-gemini` | `gemini --prompt "<prompt>"` |
| `goose` | `go` | `goose run -t "<prompt>"` |
| `opencode` | `oc`, `open-code` | `opencode run "<prompt>"` |
| `qwen` | `q` | `qwen -p "<prompt>"` |
| `aider` | `a`, `ai` | `aider --message "<prompt>"` |
| `amazon-q` | `aq`, `amazonq`, `aws-q`, `amazon` | `q chat "<prompt>"` |
| `copilot` | `cp`, `github-copilot` | `copilot -p "<prompt>"` |
| `kimi` | `k`, `moonshot`, `kimi-code` | `kimi -p "<prompt>"` |
| `antigravity` | `ag`, `agy`, `google-antigravity` | `agy "<prompt>"` |

<details>
<summary><b>Per-agent flag mappings</b></summary>

**Claude** — `claude -p`. Supports `--model`, `--output-format`, `--input-format`, `--permission-mode`, `--max-turns`. Yolo → `--dangerously-skip-permissions`.

**Codex** — `codex exec`. `--output-format json|stream-json` → `--json`. Provider is preserved in `AGENT_ROUTER_PROVIDER` (Codex receives the plain model name). Yolo → `--dangerously-bypass-approvals-and-sandbox` for routed runs; the `codexy` shim uses `codex --yolo`.

**Cursor** — `cursor-agent -p`. Plain `--model`; `--output-format` when accepted. Yolo → `--force` (required for print-mode file writes).

**Gemini** — `gemini --prompt`. Plain `--model`; `--output-format` when accepted. Yolo → `--yolo`.

**Goose** — `goose run -t`. `--provider`→`GOOSE_PROVIDER`, `--model`→`GOOSE_MODEL`, `--permission-mode`→`GOOSE_MODE`, `--max-turns`→`GOOSE_MAX_TURNS`, `--agent`→`--with-builtin`. Yolo sets `GOOSE_MODE=auto` unless `--permission-mode` is given.

**OpenCode** — `opencode run`. `--provider anthropic --model claude-sonnet-4-6` → `--model anthropic/claude-sonnet-4-6`. `--output-format json|stream-json` → `--format json`. `--agent`→`--agent`. Yolo → `--dangerously-skip-permissions`.

**Qwen** — `qwen -p`. Plain `--model`; `--output-format` when accepted. Yolo → `--yolo`.

**Aider** — `aider --message`. Provider+model joined for `--model` (e.g. `anthropic/claude-sonnet-4-6`). Use `--` for Aider flags like `--yes`. Yolo → `--yes-always`.

**Amazon Q** — `q chat`. `--agent`→`--agent`. Model selection owned by Amazon Q config.

**Copilot** — provisional. `copilot -p`. Yolo → `--yolo`. Validate against the installed CLI before relying on it.

**Kimi** — `kimi -p`. Plain `--model`, `--output-format`. Yolo → `--yolo`. Auto-loads project MCP: when `./.kimi/mcp.json` exists (relative to `--cwd` or the process cwd), the adapter adds `--mcp-config-file <cwd>/.kimi/mcp.json`, so the project config generated by `par convert` loads automatically.

**Antigravity** — experimental. Uses the `agy` command. Docs currently describe the interactive AGY CLI rather than a headless mode; the adapter passes the prompt and leaves version-specific flags to `--`. Yolo → `--dangerously-skip-permissions`.

</details>

---

## Development

```sh
scripts/setup.sh                 # full local validation (fmt, test, clippy, release build, agent check)
scripts/setup.sh --strict-harnesses   # also require at least one agent CLI installed
scripts/setup.sh --install       # validate, then install the binary
```

Direct commands:

```sh
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo run -- -h oc --provider anthropic --model claude-sonnet-4-6 -p "review" --dry-run
```

Expected `--dry-run` shape:

```json
{
  "command": "opencode",
  "args": ["run", "--model", "anthropic/claude-sonnet-4-6", "review"],
  "env": {}
}
```

### Architecture

```text
src/
  main.rs              entrypoint, stdin handling, dry-run, dispatch
  cli.rs               command-line parser
  model.rs             provider/model resolution
  process.rs           child process execution (inherit-stdio run + captured run)
  json.rs              zero-dep JSON parser/serializer (used by convert, session, ask, mcp)
  ask.rs               agent-to-agent calls (headless run + transcript context injection)
  mcp.rs               stdio MCP server (resume tools + ask_agent) + `mcp connect`
  harness/             per-agent adapters (claude, codex, cursor, gemini, goose,
                       opencode, qwen, aider, amazon_q, copilot, kimi, antigravity)
    mod.rs             Harness trait, Request, HarnessFactory, normalize_harness
    invocation.rs      command/args/env representation
  convert/             .claude/ reader + per-target writers + cross-reference resolver
  session/             cross-agent session discovery, resume, and transcript export
    mod.rs             SessionStore trait, SessionRef, Turn, listing + resume + context
    claude.rs codex.rs opencode.rs   native parsers (cwd-scoped listing + transcripts)
    cursor.rs gemini.rs              delegate adapters (resume via native CLI)
  installer.rs         agent installer registry
```

**Design constraints:** no `sh -c` (adapters build argv directly); no hidden API calls (only starts local CLIs); no login handling (authenticate each agent separately); agent-specific behavior stays in its module; provider/model transforms stay centralized in `model.rs`; `--dry-run` output stays stable enough for tests.

### Adding an agent

1. Create `src/harness/<name>.rs` and implement `Harness` for a small adapter struct.
2. Register it in `HarnessFactory::default()`.
3. Add aliases in `normalize_harness()` if useful.
4. Add dry-run tests (command, args, provider/model, env, passthrough).
5. Document it in this README.

```rust
use super::{add_passthrough, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(ExampleHarness)
}

struct ExampleHarness;

impl Harness for ExampleHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["run".to_string(), request.prompt.clone()];
        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        Ok(Invocation::new("example", add_passthrough(args, request)))
    }
}
```

---

## Project status

Working infrastructure for local automation.

**Done:** dependency-free Rust CLI · shared `claude -p`-style prompt surface · isolated per-agent adapters · agent installers · provider/model resolution · dry-run routing · cross-agent session resume · agent-to-agent calls with context bridging · stdio MCP server · validating setup script.

**Not yet:** GitHub Actions release builds · Homebrew formula · end-to-end smoke tests against every vendor CLI · a stable semver contract per agent mapping.

### Release plan

1. `scripts/setup.sh`.
2. Smoke-test locally available CLIs with `--dry-run`.
3. CI: `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, and release binaries for Linux/macOS/Windows (`par-<target>.tar.gz` / `.zip`).
4. Add archive checksums, publish a GitHub release.
5. Add a Homebrew tap once artifact names are stable.

---

## Security & privacy

`par` does not inspect or redact prompt content. Anything passed via stdin or `-p` is forwarded to the selected agent, which may send it to its configured provider.

**Yolo (permission bypass) is on by default** — each run adds the agent's bypass flag unless you pass `--no-yolo` or set `AGENT_ROUTER_YOLO=false`. This favors hands-off automation over sandboxing; opt out for untrusted prompts or sensitive directories. Use `--dry-run` to validate automation that may include secrets before running it.

---

## Source notes

Written against public docs (checked May 19, 2026): [Claude](https://code.claude.com/docs/en/cli-reference), [Codex](https://www.mintlify.com/openai/codex/advanced/exec-mode), [Cursor](https://docs.cursor.com/en/cli/using), [Gemini](https://google-gemini.github.io/gemini-cli/docs/cli/), [OpenCode](https://dev.opencode.ai/docs/cli/), [Qwen](https://qwenlm.github.io/qwen-code-docs/en/cli/index), [Aider](https://aider.chat/docs/scripting.html), [Amazon Q](https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/command-line-reference.html), [Goose](https://block.github.io/goose/docs/tutorials/headless-goose/), [Antigravity](https://antigravity.google/docs/cli-using). Installer commands live in `src/installer.rs`. Agent CLIs change fast — re-check these surfaces before a public release.
