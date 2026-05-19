# programmatic-agent-router-sdk

TypeScript SDK for routing programmatic prompts to local AI agent CLIs.

This package mirrors the Rust `agent-router` routing model for TypeScript CLIs. It does not call model APIs directly; it builds and optionally executes local harness commands such as `claude -p`, `codex exec`, `goose run -t`, and `opencode run`.

## Install

```sh
npm install programmatic-agent-router-sdk
```

## Build an Invocation

```ts
import { buildInvocation } from "programmatic-agent-router-sdk";

const invocation = buildInvocation({
  harness: "opencode",
  provider: "anthropic",
  model: "claude-sonnet-4-6",
  prompt: "review this branch",
});

console.log(invocation);
```

Output:

```json
{
  "command": "opencode",
  "args": ["run", "--model", "anthropic/claude-sonnet-4-6", "review this branch"],
  "env": {}
}
```

## Execute a Harness

```ts
import { runAgent } from "programmatic-agent-router-sdk";

const result = await runAgent({
  harness: "codex",
  model: "gpt-5.4",
  prompt: "summarize this repository",
});

if (result.status !== 0) {
  throw new Error(result.stderr);
}

console.log(result.stdout);
```

## Stream Output

```ts
import { spawnAgent } from "programmatic-agent-router-sdk";

const child = spawnAgent(
  {
    harness: "goose",
    provider: "openai",
    model: "gpt-5.4",
    permissionMode: "auto",
    maxTurns: "50",
    prompt: "fix the failing tests",
  },
  { stdio: "inherit" },
);

child.on("exit", (code) => {
  process.exitCode = code ?? 1;
});
```

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

## API

Primary exports:

- `buildInvocation(request)`: returns `{ command, args, env }`.
- `spawnAgent(request, options)`: starts the routed child process.
- `runAgent(request, options)`: executes and captures stdout/stderr.
- `createHarness(name)`: returns a concrete harness adapter.
- `normalizeHarness(name)`: normalizes aliases.
- `resolveModel(provider, model)`: central provider/model helper.

Use `passthrough` for harness-specific flags:

```ts
buildInvocation({
  harness: "aider",
  prompt: "fix lint",
  passthrough: ["--yes", "--no-auto-commits"],
});
```
