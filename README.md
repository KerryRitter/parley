# Programmatic Agent Router

`agent-router` is a small Rust CLI that gives automation one stable prompt interface and routes the call to a local AI agent harness.

It is designed around the useful `claude -p "prompt"` style of workflow:

```sh
agent-router -p "review this repository" --harness codex --model gpt-5.4
agent-router -p "fix the failing tests" --harness opencode --provider anthropic --model claude-sonnet-4-6
git diff | agent-router -p "review this patch" --harness goose --provider openai --model gpt-5.4
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

That makes scripts brittle. Switching harnesses means editing commands, model flags, provider syntax, JSON output flags, and environment variables. `agent-router` keeps scripts focused on intent:

```sh
agent-router --harness <name> --provider <provider> --model <model> -p "<task>"
```

The harness adapter owns the translation.

## Project Status

This is early, working infrastructure for local automation.

Implemented:

- Rust-native CLI with no runtime dependencies.
- Shared `claude -p`-style prompt surface.
- Harness factory pattern with isolated adapters.
- Provider/model resolution layer.
- Dry-run mode for deterministic routing tests.
- Setup script that validates Rust, tests, linting, release build, and installed downstream harness CLIs.

Not implemented yet:

- GitHub Actions release builds.
- Homebrew formula.
- End-to-end smoke tests against every vendor CLI.
- Stable semver contract for every harness mapping.

## Install

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
~/.local/bin/par -> agent-router
```

Make sure both Cargo's bin directory and `~/.local/bin` are on your `PATH`:

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
```

### Install from a Release Binary

The intended public release flow is to publish platform archives from CI:

- `agent-router-aarch64-apple-darwin.tar.gz`
- `agent-router-x86_64-apple-darwin.tar.gz`
- `agent-router-x86_64-unknown-linux-gnu.tar.gz`
- `agent-router-x86_64-pc-windows-msvc.zip`

Then installation is:

```sh
tar -xzf agent-router-x86_64-unknown-linux-gnu.tar.gz
install -m 0755 agent-router ~/.local/bin/agent-router
ln -sf ~/.local/bin/agent-router ~/.local/bin/par
```

### Homebrew Later

A public distribution should add a tap:

```sh
brew tap <org>/tap
brew install agent-router
```

That should install the same release binary and provide both `agent-router` and `par`.

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
agent-router -p "summarize this repository"
```

Equivalent routed command:

```sh
claude -p "summarize this repository"
```

Use Codex:

```sh
agent-router --harness codex --model gpt-5.4 -p "add parser tests"
```

Use OpenCode with provider-qualified model routing:

```sh
agent-router \
  --harness opencode \
  --provider anthropic \
  --model claude-sonnet-4-6 \
  -p "review this branch"
```

Use Goose with environment-backed configuration:

```sh
agent-router \
  --harness goose \
  --provider openai \
  --model gpt-5.4 \
  --permission-mode auto \
  --max-turns 50 \
  -p "fix the failing tests"
```

Pipe context:

```sh
git diff | agent-router --harness aider -p "fix the problems in this patch"
```

Preview without execution:

```sh
agent-router --harness qwen --model qwen3-coder-plus -p "review src" --dry-run
```

Pass harness-specific flags after `--`:

```sh
agent-router --harness claude -p "review this" -- --verbose
agent-router --harness aider -p "fix lint" -- --yes --no-auto-commits
```

## CLI Reference

```text
agent-router [options] [-p <prompt>] [positional prompt]
```

| Option | Purpose |
| --- | --- |
| `-p`, `--prompt`, `--print` | Prompt text. `--print` is accepted for Claude compatibility. |
| `--harness <name>` | Target CLI adapter. Defaults to `claude`. |
| `--provider <name>` | Provider namespace when the harness uses provider-qualified model names or env configuration. |
| `--model`, `-m <name>` | Model name. |
| `--agent <name>` | Agent/persona/profile where supported. |
| `--output-format <fmt>` | Output mode where supported by the target harness. |
| `--input-format <fmt>` | Claude-compatible input format. |
| `--permission-mode <mode>` | Permission/sandbox mode where supported. |
| `--max-turns <n>` | Maximum agent turns where supported. |
| `--cwd <path>` | Working directory for the child process. |
| `--dry-run` | Print the routed invocation as JSON. |
| `--help`, `-h` | Print help. |
| `--version`, `-v` | Print version. |
| `--` | Pass all remaining args directly to the target CLI. |

Environment defaults:

```sh
export AGENT_ROUTER_HARNESS=codex
export AGENT_ROUTER_PROVIDER=openai
export AGENT_ROUTER_MODEL=gpt-5.4
```

Prompt input rules:

- If stdin is piped and `-p` is provided, stdin is placed before the prompt with a blank line between them.
- If stdin is piped and no prompt is provided, stdin becomes the prompt.
- If no prompt and no stdin are provided, the command fails before launching a harness.

## Supported Harnesses

| Harness | Aliases | Routed command |
| --- | --- | --- |
| `claude` | none | `claude -p "<prompt>"` |
| `codex` | `openai` | `codex exec "<prompt>"` |
| `cursor` | `cursor-agent` | `cursor-agent -p "<prompt>"` |
| `gemini` | `google`, `google-gemini` | `gemini --prompt "<prompt>"` |
| `goose` | none | `goose run -t "<prompt>"` |
| `opencode` | `open-code` | `opencode run "<prompt>"` |
| `qwen` | none | `qwen -p "<prompt>"` |
| `aider` | none | `aider --message "<prompt>"` |
| `amazon-q` | `amazonq`, `aws-q`, `amazon` | `q chat "<prompt>"` |
| `copilot` | `github-copilot` | `copilot -p "<prompt>"` |

### Harness Details

Claude:

- Uses `claude -p`.
- Supports `--model`, `--output-format`, `--input-format`, `--permission-mode`, and `--max-turns`.

Codex:

- Uses `codex exec`.
- `--output-format json` and `--output-format stream-json` map to `--json`.
- Provider is currently preserved in `AGENT_ROUTER_PROVIDER` for future policy, but Codex receives the plain model name.

Cursor:

- Uses `cursor-agent -p`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Cursor agent.

Gemini:

- Uses `gemini --prompt`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Gemini CLI.

Goose:

- Uses `goose run -t`.
- `--provider` maps to `GOOSE_PROVIDER`.
- `--model` maps to `GOOSE_MODEL`.
- `--permission-mode` maps to `GOOSE_MODE`.
- `--max-turns` maps to `GOOSE_MAX_TURNS`.
- `--agent <name>` maps to `goose run --with-builtin <name>`.

OpenCode:

- Uses `opencode run`.
- `--provider anthropic --model claude-sonnet-4-6` becomes `--model anthropic/claude-sonnet-4-6`.
- `--output-format json` and `--output-format stream-json` map to `--format json`.
- `--agent` maps to `--agent`.

Qwen:

- Uses `qwen -p`.
- Supports plain `--model`.
- Supports `--output-format` when accepted by the installed Qwen CLI.

Aider:

- Uses `aider --message`.
- Provider and model are joined for `--model`, such as `anthropic/claude-sonnet-4-6`.
- Use `--` for Aider-specific automation flags, for example `--yes`.

Amazon Q:

- Uses `q chat`.
- `--agent` maps to `--agent`.
- Model selection is owned by Amazon Q CLI configuration.

Copilot:

- Provisional adapter.
- Uses `copilot -p`.
- Validate against the installed CLI before relying on it in automation.

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
    copilot.rs         provisional Copilot adapter
```

Core types:

- `Request`: normalized router input.
- `Invocation`: concrete command, args, and environment.
- `Harness`: adapter trait implemented by each harness.
- `HarnessFactory`: registry that maps harness names to adapter constructors.
- `ModelFactory`: central place for provider/model normalization and formatting.

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

`agent-router` does not inspect or redact prompt content. Anything passed to stdin or `-p` is forwarded to the selected harness. The selected harness may send that content to its configured provider.

The router also does not weaken or bypass harness sandboxing. Permission behavior is delegated to the downstream CLI. When a shared option like `--permission-mode` is mapped, it uses the target harness's documented control surface.

Use `--dry-run` when validating automation that may include secrets or proprietary context.

## Recommended Next Harnesses

High priority:

- `crush`: add once its one-shot/headless command surface is pinned.
- `amp`: useful commercial harness, but verify whether it has a true non-interactive one-shot mode before adding.
- `windsurf`: support only if/when the agent CLI exposes a stable non-interactive prompt command.

Medium priority:

- `qoder`, `trae`, `kimi`, `openclaw`, `droid`: support if they expose either ACP or a stable direct CLI.
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

Re-check these command surfaces before a public release. Agent CLIs change fast.
