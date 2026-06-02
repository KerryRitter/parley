import assert from "node:assert/strict";
import test from "node:test";

import {
  buildInvocation,
  knownHarnesses,
  normalizeHarness,
  normalizeRequest,
  registerHarness,
  resolveModel,
  runAgent,
} from "./index.js";
import type { Harness, Invocation, NormalizedRequest } from "./index.js";

test("routes claude by default", () => {
  assert.deepEqual(buildInvocation({ prompt: "explain this", model: "sonnet" }), {
    command: "claude",
    args: ["-p", "explain this", "--model", "sonnet"],
    env: {},
  });
});

test("routes codex exec with json output", () => {
  assert.deepEqual(
    buildInvocation({
      harness: "codex",
      model: "gpt-5.4",
      outputFormat: "json",
      prompt: "review",
    }),
    {
      command: "codex",
      args: ["exec", "--model", "gpt-5.4", "--json", "review"],
      env: {},
    },
  );
});

test("routes opencode with provider-qualified model", () => {
  assert.deepEqual(
    buildInvocation({
      harness: "opencode",
      provider: "anthropic",
      model: "claude-sonnet-4-6",
      agent: "reviewer",
      prompt: "review diff",
    }),
    {
      command: "opencode",
      args: [
        "run",
        "--model",
        "anthropic/claude-sonnet-4-6",
        "--agent",
        "reviewer",
        "review diff",
      ],
      env: {},
    },
  );
});

test("routes goose through env-backed settings", () => {
  assert.deepEqual(
    buildInvocation({
      harness: "goose",
      provider: "openai",
      model: "gpt-5.4",
      permissionMode: "auto",
      maxTurns: 50,
      agent: "developer",
      prompt: "fix tests",
    }),
    {
      command: "goose",
      args: ["run", "--with-builtin", "developer", "-t", "fix tests"],
      env: {
        GOOSE_PROVIDER: "openai",
        GOOSE_MODEL: "gpt-5.4",
        GOOSE_MODE: "auto",
        GOOSE_MAX_TURNS: "50",
      },
    },
  );
});

test("routes aider with passthrough flags", () => {
  assert.deepEqual(
    buildInvocation({
      harness: "aider",
      provider: "anthropic",
      model: "claude-sonnet-4-6",
      prompt: "fix lint",
      passthrough: ["--yes", "--no-auto-commits"],
    }),
    {
      command: "aider",
      args: [
        "--message",
        "fix lint",
        "--model",
        "anthropic/claude-sonnet-4-6",
        "--yes",
        "--no-auto-commits",
      ],
      env: {},
    },
  );
});

test("normalizes aliases", () => {
  assert.equal(normalizeHarness("openai"), "codex");
  assert.equal(normalizeHarness("cursor-agent"), "cursor");
  assert.equal(normalizeHarness("aws-q"), "amazon-q");
  assert.equal(normalizeHarness(undefined), "claude");
});

test("combines stdin and prompt", () => {
  assert.equal(
    normalizeRequest({ harness: "gemini", stdin: "hello\n", prompt: "summarize" }).prompt,
    "hello\n\nsummarize",
  );
});

test("resolves model formatting", () => {
  assert.equal(
    resolveModel("anthropic", "claude-sonnet-4-6")?.format("provider-qualified"),
    "anthropic/claude-sonnet-4-6",
  );
  assert.equal(
    resolveModel("ignored", "openai/gpt-5.4")?.format("provider-qualified"),
    "openai/gpt-5.4",
  );
});

test("lists known harnesses", () => {
  assert.deepEqual(knownHarnesses(), [
    "aider",
    "amazon-q",
    "claude",
    "codex",
    "copilot",
    "cursor",
    "gemini",
    "goose",
    "opencode",
    "qwen",
  ]);
});

test("runAgent captures stdout and invocation", async () => {
  class NodeHarness implements Harness {
    readonly name = "node";

    build(_request: NormalizedRequest): Invocation {
      return {
        command: process.execPath,
        args: ["-e", "require('node:fs').writeFileSync(1, 'ok')"],
        env: {},
      };
    }
  }

  registerHarness("node", NodeHarness);

  const result = await runAgent({
    harness: "node",
    prompt: "unused",
  });

  assert.equal(result.status, 0);
  assert.equal(result.stdout, "ok");
  assert.equal(result.stderr, "");
  assert.equal(result.invocation.command, process.execPath);
});
