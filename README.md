# Programmatic Agent Router

`par` is a small Rust CLI that gives automation one stable prompt interface and routes the call to a local AI agent harness.

It is designed around the useful `claude -p "prompt"` style of workflow:

```sh
par -p "review this repository" --harness codex --model gpt-5.4
par -p "fix the failing tests" --harness opencode --provider anthropic --model claude-sonnet-4-6
git diff | par -p "review this patch" --harness goose --provider openai --model gpt-5.4
```

The router does not call model APIs directly. It starts an already-installed harness CLI, maps shared router flags into that harness's command surface, streams stdout/stderr, and exits with the child process status.

## Why This Exists

Programmatic agent CLIs all solve a similar problem, but their headless interfaces differ:

- Claude Code uses `claude -p`.
- Codex uses `codex exec`.
- Cursor uses `cursor-agent -p`.
- Gemini uses `gemini --prompt`.
- Goose uses `goose run -t`.
- OpenCode uses `opencode run`.
- Aider uses `aider --message`.
- Antigravity uses the `agy` CLI surface.

That makes scripts brittle. Switching harnesses means editing commands, model flags, provider syntax, JSON output flags, and environment variables. `par` keeps scripts focused on intent:

```sh
par --harness <name> --provider <provider> --model <model> -p "<task>"
```

The harness adapter owns the translation.

## Project Status

This is early, working infrastructure for local automation.

Implemented:

- Rust-native CLI with no runtime dependencies.
- Shared `claude -p`-style prompt surface.
- Harness factory pattern with isolated adapters.
- Harness installers via `par install <harness>`.
- Provider/model resolution layer.
- Dry-run mode for deterministic routing tests.
- Setup script that validates Rust, tests, linting, release build, and installed downstream harness CLIs.

Not implemented yet:

- GitHub Actions release builds.
- Homebrew formula.
- End-to-end smoke tests against every vendor CLI.
- Stable semver contract for every harness mapping.

## Install

### Quick Install

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh
```

This installs `par` into `~/.local/bin` and creates an `agent-router` compatibility alias.

### Prerequisites

You need Rust for source installs:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Make sure Cargo is available:

```sh
cargo --version
```

The setup script will also find Cargo at `~/.cargo/bin` even if that directory has not been added to your shell `PATH` yet.

### Validate the System

```sh
scripts/setup.sh
```

This checks:

- Repository layout.
- `cargo` and `rustc`.
- `cargo fmt --check`.
- `cargo test`.
- `cargo clippy --all-targets -- -D warnings`.
- `cargo build --release`.
- Which supported downstream harness CLIs are installed.

Use strict harness validation if this machine should already have at least one supported agent CLI:

```sh
scripts/setup.sh --strict-harnesses
```

## Install Harness CLIs

`par` can install supported downstream harness CLIs:

```sh
par install list
par install claude
par install codex
par install antigravity
par install --dry-run all
```

The installer registry is intentionally transparent: `--dry-run` prints the exact upstream install command without executing it. For harnesses that do not expose a stable terminal one-liner, such as Amazon Q, `par install <name>` prints the official install page and verification command instead of guessing.

Current installer coverage:

| Harness | Installer |
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

### Install from Source

```sh
scripts/setup.sh --install
```

This runs validation, installs the binary with:

```sh
cargo install --path . --force
```

and creates:

```sh
~/.local/bin/par
~/.local/bin/agent-router -> ~/.local/bin/par
```

Make sure both Cargo's bin directory and `~/.local/bin` are on your `PATH`:

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
```

### Install from GitHub

The quick install command is:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh
```

The script tries to install a release binary for your platform first. Until release binaries exist, it falls back to:

```sh
cargo install --git https://github.com/KerryRitter/programmatic-agent-router.git --branch main --force
```

Useful options:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh -s -- --install-dir /usr/local/bin
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh -s -- --from-source
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh -s -- --from-source --git-protocol ssh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/KerryRitter/programmatic-agent-router/main/install.sh | sh -s -- --no-agent-router
```

### Install from a Release Binary

The intended public release flow is to publish platform archives from CI:

- `par-aarch64-apple-darwin.tar.gz`
- `par-x86_64-apple-darwin.tar.gz`
- `par-x86_64-unknown-linux-gnu.tar.gz`
- `par-x86_64-pc-windows-msvc.zip`

Then installation is:

```sh
tar -xzf par-x86_64-unknown-linux-gnu.tar.gz
install -m 0755 par ~/.local/bin/par
ln -sf ~/.local/bin/par ~/.local/bin/agent-router
```

### Homebrew Later

A public distribution should add a tap:

```sh
brew tap <org>/tap
brew install par
```

That should install the same release binary and provide `par` as the primary CLI name.

## TypeScript SDK

This repository also contains a publishable TypeScript SDK for TypeScript CLIs:

```text
packages/typescript/
```

The SDK exposes the same routing model as the Rust CLI:

```ts
import { buildInvocation, runAgent } from "programmatic-agent-router-sdk";

const invocation = buildInvocation({
  harness: "opencode",
  provider: "anthropic",
  model: "claude-sonnet-4-6",
  prompt: "review this branch",
});

const result = await runAgent({
  harness: "codex",
  model: "gpt-5.4",
  prompt: "summarize this repository",
});
```

Validate the SDK:

```sh
cd packages/typescript
npm install
npm run validate
```

Publish dry run:

```sh
cd packages/typescript
npm pack --dry-run
```

Publish when ready:

```sh
cd packages/typescript
npm publish --access public
```

## Quick Start

Default harness is `claude`:

```sh
par
par -p "summarize this repository"
```

Equivalent routed command:

```sh
claude
claude -p "summarize this repository"
```

Use Codex:

```sh
par --harness codex --model gpt-5.4 -p "add parser tests"
```

Set the default harness on this machine:

```sh
par default codex
par
par -p "add parser tests"
```

Persist a default harness with permission bypass enabled:

```sh
par default claude --yolo
par current
```

The default is stored in `~/.config/par/default`, or `$XDG_CONFIG_HOME/par/default` when `XDG_CONFIG_HOME` is set. Override the path with `PAR_DEFAULT_FILE`.

Create shortcut scripts for yolo-capable harnesses:

```sh
par shims install
claudey -p "work in this sandbox"
codexy "work in this sandbox"
```

By default, shims are written to `~/.local/bin`. Override this with `par shims install --dir <dir>` or `PAR_SHIM_DIR`.

Use OpenCode with provider-qualified model routing:

```sh
par \
  --harness opencode \
  --provider anthropic \
  --model claude-sonnet-4-6 \
  -p "review this branch"
```

Use Goose with environment-backed configuration:

```sh
par \
  --harness goose \
  --provider openai \
  --model gpt-5.4 \
  --permission-mode auto \
  --max-turns 50 \
  -p "fix the failing tests"
```

Pipe context:

```sh
git diff | par --harness aider -p "fix the problems in this patch"
```

Preview without execution:

```sh
par --harness qwen --model qwen3-coder-plus -p "review src" --dry-run
```

Pass harness-specific flags after `--`:

```sh
par --harness claude -p "review this" -- --verbose
par --harness aider -p "fix lint" -- --yes --no-auto-commits
```

## CLI Reference

```text
par [options]
par [options] [-p <prompt>] [positional prompt]
```

| Option | Purpose |
| --- | --- |
| `-p`, `--prompt`, `--print` | Prompt text. `--print` is accepted for Claude compatibility. |
| `--harness`, `-h <name>` | Target CLI adapter. Defaults to `claude`. Accepts short codes (see below). Bare `-h` with no value prints help. |
| `--provider <name>` | Provider namespace when the harness uses provider-qualified model names or env configuration. |
| `--model`, `-m <name>` | Model name. |
| `--agent <name>` | Agent/persona/profile where supported. |
| `--output-format <fmt>` | Output mode where supported by the target harness. |
| `--input-format <fmt>` | Claude-compatible input format. |
| `--permission-mode <mode>` | Permission/sandbox mode where supported. |
| `--max-turns <n>` | Maximum agent turns where supported. |
| `--cwd <path>` | Working directory for the child process. |
| `--yolo` | Explicitly add the harness-specific permission bypass flag. On by default. |
| `--no-yolo` | Opt out of yolo for this run. Also set `AGENT_ROUTER_YOLO=false` to opt out persistently. |
| `--dry-run` | Print the routed invocation as JSON. |
| `--help` | Print help (also `-h` with no value). |
| `--version`, `-v` | Print version. |
| `--` | Pass all remaining args directly to the target CLI. |

**Yolo is on by default.** Every run adds the target harness's permission-bypass flag (e.g. `--dangerously-skip-permissions` for Claude). Use `--no-yolo` per run, or `AGENT_ROUTER_YOLO=false` to disable persistently. Harnesses without a known bypass flag (e.g. Amazon Q) simply run without one.

**Harness short codes** (`-h <code>`):

| Code | Harness | Code | Harness | Code | Harness |
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

Environment defaults:

```sh
export AGENT_ROUTER_HARNESS=codex
export AGENT_ROUTER_PROVIDER=openai
export AGENT_ROUTER_MODEL=gpt-5.4
export AGENT_ROUTER_YOLO=true
```

Persisted default commands:

```sh
par default                  # show persisted defaults
par default codex --yolo     # set harness and yolo default
par default --no-yolo        # keep harness, disable yolo
par default --path           # print the default file path
par current                  # alias for showing defaults
par list                     # list supported harness names
```

This follows the useful part of `nvm`'s command shape: a default alias, `use`-style setter, `current`, and `list`. `par` does not manage installed versions; downstream CLIs still own their own install and upgrade flows.

Prompt input rules:

- If no prompt and no stdin are provided, `par` launches the default harness's interactive entrypoint.
- If stdin is piped and `-p` is provided, stdin is placed before the prompt with a blank line between them.
- If stdin is piped and no prompt is provided, stdin becomes the prompt.

## Supported Harnesses

| Harness | Aliases | Routed command |
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

### Harness Details

Claude:

- Uses `claude -p`.
- Supports `--model`, `--output-format`, `--input-format`, `--permission-mode`, and `--max-turns`.
- `--yolo` maps to `--dangerously-skip-permissions`.

Codex:

- Uses `codex exec`.
- `--output-format json` and `--output-format stream-json` map to `--json`.
- Provider is currently preserved in `AGENT_ROUTER_PROVIDER` for future policy, but Codex receives the plain model name.
- `--yolo` maps to `--dangerously-bypass-approvals-and-sandbox` for routed noninteractive runs. The `codexy` shim uses the shorter `codex --yolo` entrypoint.

Cursor:

- Uses `cursor-agent -p`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Cursor agent.
- `--yolo` maps to `--force`, which Cursor requires for print-mode file writes.

Gemini:

- Uses `gemini --prompt`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Gemini CLI.
- `--yolo` maps to `--yolo`.

Goose:

- Uses `goose run -t`.
- `--provider` maps to `GOOSE_PROVIDER`.
- `--model` maps to `GOOSE_MODEL`.
- `--permission-mode` maps to `GOOSE_MODE`.
- `--max-turns` maps to `GOOSE_MAX_TURNS`.
- `--agent <name>` maps to `goose run --with-builtin <name>`.
- `--yolo` sets `GOOSE_MODE=auto` unless `--permission-mode` is provided.

OpenCode:

- Uses `opencode run`.
- `--provider anthropic --model claude-sonnet-4-6` becomes `--model anthropic/claude-sonnet-4-6`.
- `--output-format json` and `--output-format stream-json` map to `--format json`.
- `--agent` maps to `--agent`.
- `--yolo` maps to `--dangerously-skip-permissions`.

Qwen:

- Uses `qwen -p`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Qwen CLI.
- `--yolo` maps to `--yolo`.

Aider:

- Uses `aider --message`.
- Provider and model are joined for `--model`, such as `anthropic/claude-sonnet-4-6`.
- Use `--` for Aider-specific automation flags, for example `--yes`.
- `--yolo` maps to `--yes-always`.

Amazon Q:

- Uses `q chat`.
- `--agent` maps to `--agent`.
- Model selection is owned by Amazon Q CLI configuration.

Copilot:

- Provisional adapter.
- Uses `copilot -p`.
- `--yolo` maps to `--yolo`.
- Validate against the installed CLI before relying on it in automation.

Kimi:

- Uses `kimi -p`.
- Supports plain `--model` and `--output-format`.
- `--yolo` maps to `--yolo`.
- Auto-loads project MCP: when `./.kimi/mcp.json` exists (relative to `--cwd` or the process cwd), the adapter adds `--mcp-config-file <cwd>/.kimi/mcp.json`. Kimi otherwise only auto-discovers MCP from the global `~/.kimi/mcp.json`, so this makes the project config (generated by `par convert`) load automatically.

Antigravity:

- Experimental adapter.
- Uses the `agy` command installed by Google Antigravity.
- Official docs currently describe the interactive AGY CLI rather than a stable print/headless mode. The adapter passes the prompt to `agy` and leaves version-specific flags to `--`.
- `--yolo` maps to `--dangerously-skip-permissions`.

## Convert

`par convert` ports a Claude command/skill pack to other harnesses. `.claude/` (commands, skills, agents, references), `CLAUDE.md`, and `.mcp.json` stay the **single source of truth**; convert generates each harness's native config from them.

```sh
par convert                       # claude -> all targets
par convert --to kimi             # claude -> one target
par convert --from claude --to codex
par convert --dry-run             # show what would be written
par convert --cwd path/to/project
```

Supported source: `claude`. Targets: `gemini`, `codex`, `antigravity`, `opencode`, `cursor`, `kimi`.

What it does:

- **Parses frontmatter** — real descriptions, per-command `model`, argument placeholders; strips the block from bodies.
- **Reads commands, skills, personas (`.claude/agents/`), references, and `.mcp.json`.**
- **Emits native artifacts per target** — e.g. `.kimi/skills/<name>/SKILL.md` + `.kimi/mcp.json`, `.codex/config.toml` + `.agents/skills/`, `.gemini/commands/*.toml` + `GEMINI.md`, `.cursor/rules/`, `.opencode/config.json`, plus `AGENTS.md`. MCP servers from `.mcp.json` are translated into each harness's format.
- **Resolves cross-references** — every `/command`, `**skill** skill`, persona path, and reference path is checked against the pack. The run prints a resolution report and **exits non-zero if any reference dead-ends**, so a typo'd command or missing skill fails the convert instead of shipping a broken pack.

Generated skills carry a `par-convert:generated` marker so re-running convert replaces only its own output and never a hand-authored skill. Generated files are meant to be git-ignored — `.claude/` is what you commit. A typical `npm run sync:instructions` is just `par convert --from claude --to all`.

Kimi invokes the generated skills as slash commands, e.g. `/skill:agent-queue dev`. See the Kimi harness notes above for MCP auto-loading.

## Architecture

The code is intentionally small and explicit.

```text
src/
  main.rs              entrypoint, stdin handling, dry-run, process dispatch
  cli.rs               shared command-line parser
  model.rs             provider/model resolution
  process.rs           child process execution
  harness/
    mod.rs             Harness trait, Request, HarnessFactory
    invocation.rs      command/args/env representation
    claude.rs          Claude adapter
    codex.rs           Codex adapter
    cursor.rs          Cursor adapter
    gemini.rs          Gemini adapter
    goose.rs           Goose adapter
    opencode.rs        OpenCode adapter
    qwen.rs            Qwen adapter
    aider.rs           Aider adapter
    amazon_q.rs        Amazon Q adapter
    antigravity.rs     Antigravity adapter
    copilot.rs         provisional Copilot adapter
    kimi.rs            Kimi adapter (auto-loads .kimi/mcp.json)
  convert/
    mod.rs             convert orchestration + report
    claude.rs          reader for .claude/ packs
    project.rs         parsed command/skill/persona model
    frontmatter.rs     YAML frontmatter parser
    links.rs           cross-reference resolver
    util.rs            slug/truncate/skill helpers
    gemini.rs codex.rs kimi.rs cursor.rs opencode.rs antigravity.rs   target writers
  installer.rs         harness installer registry
```

Core types:

- `Request`: normalized router input.
- `Invocation`: concrete command, args, and environment.
- `Harness`: adapter trait implemented by each harness.
- `HarnessFactory`: registry that maps harness names to adapter constructors.
- `ModelFactory`: central place for provider/model normalization and formatting.
- `installer.rs`: explicit upstream install commands for downstream harness CLIs.

Design constraints:

- No `sh -c`; adapters build argv directly.
- No hidden API calls; the router only starts local CLIs.
- No login handling; authenticate each downstream harness separately.
- Harness-specific behavior stays in that harness module.
- Provider/model transformation stays centralized in `model.rs`.
- `--dry-run` output should be stable enough for tests and shell debugging.

## Adding a Harness

1. Create `src/harness/<name>.rs`.
2. Implement `Harness` for a small adapter struct.
3. Register the adapter in `HarnessFactory::default()`.
4. Add aliases in `normalize_harness()` if useful.
5. Add dry-run tests for command, args, provider/model behavior, env vars, and passthrough.
6. Document the harness and source docs in this README.

Adapter skeleton:

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

## Development

Run the full local validation:

```sh
scripts/setup.sh
```

Install after validation:

```sh
scripts/setup.sh --install
```

Common direct commands:

```sh
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Try a dry run:

```sh
cargo run -- --harness opencode --provider anthropic --model claude-sonnet-4-6 -p "review" --dry-run
```

Expected dry-run shape:

```json
{
  "command": "opencode",
  "args": ["run", "--model", "anthropic/claude-sonnet-4-6", "review"],
  "env": {}
}
```

## Release Plan

Recommended first public release checklist:

1. Run `scripts/setup.sh`.
2. Smoke-test installed CLIs that are available locally with `--dry-run`.
3. Add GitHub Actions for:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo clippy --all-targets -- -D warnings`
   - release binaries for Linux, macOS, and Windows
4. Add archive checksums.
5. Publish a GitHub release.
6. Add a Homebrew tap once the release artifact names are stable.

## Security and Privacy

`par` does not inspect or redact prompt content. Anything passed to stdin or `-p` is forwarded to the selected harness. The selected harness may send that content to its configured provider.

**Yolo (permission bypass) is on by default.** Each run adds the target harness's bypass flag unless you pass `--no-yolo` or set `AGENT_ROUTER_YOLO=false`. This favors hands-off automation over sandboxing — opt out when running untrusted prompts or in sensitive directories. Permission behavior is otherwise delegated to the downstream CLI; when a shared option like `--permission-mode` or `--yolo`/`--no-yolo` is mapped, it uses the target harness's command surface.

Use `--dry-run` when validating automation that may include secrets or proprietary context.

## Recommended Next Harnesses

High priority:

- `crush`: add once its one-shot/headless command surface is pinned.
- `amp`: useful commercial harness, but verify whether it has a true non-interactive one-shot mode before adding.
- `windsurf`: support only if/when the agent CLI exposes a stable non-interactive prompt command.

Medium priority:

- `qoder`, `trae`, `openclaw`, `droid`: support if they expose either ACP or a stable direct CLI.
- `factory`: worth tracking if the local CLI is scriptable outside its hosted workflow.
- `continue`, `cline`, `roo`, `kilo`: these are often extension-first rather than CLI-first; only add if there is a real headless command.

Out of scope for now:

- Web-only agents with no local CLI.
- Interactive-only CLIs that block on a TTY.
- Raw model providers where there is no agent harness. This project routes harnesses, not chat completions.

## Source Notes

This README was written against public docs checked on May 19, 2026:

- [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference) documents `claude -p` / `--print`.
- [Codex exec mode docs](https://www.mintlify.com/openai/codex/advanced/exec-mode) document `codex exec`.
- [Cursor CLI docs](https://docs.cursor.com/en/cli/using) document `cursor-agent -p` / `--print`.
- [Gemini CLI docs](https://google-gemini.github.io/gemini-cli/docs/cli/) document `--prompt` / `-p`.
- [OpenCode CLI docs](https://dev.opencode.ai/docs/cli/) document `opencode run`.
- [Qwen Code CLI docs](https://qwenlm.github.io/qwen-code-docs/en/cli/index) document `qwen -p` / `--prompt`.
- [Aider scripting docs](https://aider.chat/docs/scripting.html) document `aider --message`.
- [Amazon Q Developer CLI reference](https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/command-line-reference.html) documents `q chat --agent <name> "<prompt>"`.
- [Goose headless docs](https://block.github.io/goose/docs/tutorials/headless-goose/) document `goose run -t`.
- [Antigravity CLI docs](https://antigravity.google/docs/cli-using) document the `agy` CLI surface and configuration model.

Installer commands are kept in `src/installer.rs` and checked against the official install docs where a stable one-liner exists. Amazon Q remains manual because the official AWS CLI docs point users through platform-specific installer flows rather than a single shell installer command.

Re-check these command surfaces before a public release. Agent CLIs change fast.
