import { resolveModel } from "./model.js";
import type { Harness, Invocation, NormalizedRequest } from "./types.js";

type HarnessConstructor = new () => Harness;

const harnesses = new Map<string, HarnessConstructor>();

export function registerHarness(name: string, constructor: HarnessConstructor): void {
  harnesses.set(name, constructor);
}

export function createHarness(name: string): Harness {
  const normalized = normalizeHarness(name);
  const constructor = harnesses.get(normalized);
  if (!constructor) {
    throw new Error(`unknown harness "${name}". Known harnesses: ${knownHarnesses().join(", ")}`);
  }
  return new constructor();
}

export function knownHarnesses(): string[] {
  return [...harnesses.keys()].sort();
}

export function normalizeHarness(harness: string | undefined): string {
  switch ((harness ?? "claude").toLowerCase()) {
    case "cursor-agent":
      return "cursor";
    case "open-code":
      return "opencode";
    case "google":
    case "google-gemini":
      return "gemini";
    case "openai":
      return "codex";
    case "github-copilot":
      return "copilot";
    case "amazonq":
    case "aws-q":
    case "amazon":
      return "amazon-q";
    default:
      return harness ?? "claude";
  }
}

function invocation(command: string, args: string[], env: Record<string, string> = {}): Invocation {
  return { command, args, env };
}

function withPassthrough(args: string[], request: NormalizedRequest): string[] {
  return [...args, ...request.passthrough];
}

function isJsonOutput(request: NormalizedRequest): boolean {
  return request.outputFormat === "json" || request.outputFormat === "stream-json";
}

function plainModel(request: NormalizedRequest): string | undefined {
  return resolveModel(request.provider, request.model)?.format("plain");
}

function providerQualifiedModel(request: NormalizedRequest): string | undefined {
  return resolveModel(request.provider, request.model)?.format("provider-qualified");
}

class ClaudeHarness implements Harness {
  readonly name = "claude";

  build(request: NormalizedRequest): Invocation {
    const args = ["-p", request.prompt];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (request.outputFormat) args.push("--output-format", request.outputFormat);
    if (request.inputFormat) args.push("--input-format", request.inputFormat);
    if (request.permissionMode) args.push("--permission-mode", request.permissionMode);
    if (request.maxTurns) args.push("--max-turns", request.maxTurns);
    return invocation("claude", withPassthrough(args, request));
  }
}

class CodexHarness implements Harness {
  readonly name = "codex";

  build(request: NormalizedRequest): Invocation {
    const args = ["exec"];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (isJsonOutput(request)) args.push("--json");
    args.push(request.prompt);
    const env = request.provider ? { AGENT_ROUTER_PROVIDER: request.provider } : {};
    return invocation("codex", withPassthrough(args, request), env);
  }
}

class CursorHarness implements Harness {
  readonly name = "cursor";

  build(request: NormalizedRequest): Invocation {
    const args = ["-p", request.prompt];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (request.outputFormat) args.push("--output-format", request.outputFormat);
    return invocation("cursor-agent", withPassthrough(args, request));
  }
}

class GeminiHarness implements Harness {
  readonly name = "gemini";

  build(request: NormalizedRequest): Invocation {
    const args = ["--prompt", request.prompt];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (request.outputFormat) args.push("--output-format", request.outputFormat);
    return invocation("gemini", withPassthrough(args, request));
  }
}

class GooseHarness implements Harness {
  readonly name = "goose";

  build(request: NormalizedRequest): Invocation {
    const args = ["run"];
    if (request.agent) args.push("--with-builtin", request.agent);
    args.push("-t", request.prompt);

    const env: Record<string, string> = {};
    const model = plainModel(request);
    if (request.provider) env.GOOSE_PROVIDER = request.provider;
    if (model) env.GOOSE_MODEL = model;
    if (request.permissionMode) env.GOOSE_MODE = request.permissionMode;
    if (request.maxTurns) env.GOOSE_MAX_TURNS = request.maxTurns;

    return invocation("goose", withPassthrough(args, request), env);
  }
}

class OpenCodeHarness implements Harness {
  readonly name = "opencode";

  build(request: NormalizedRequest): Invocation {
    const args = ["run"];
    const model = providerQualifiedModel(request);
    if (model) args.push("--model", model);
    if (request.agent) args.push("--agent", request.agent);
    if (isJsonOutput(request)) args.push("--format", "json");
    args.push(request.prompt);
    return invocation("opencode", withPassthrough(args, request));
  }
}

class QwenHarness implements Harness {
  readonly name = "qwen";

  build(request: NormalizedRequest): Invocation {
    const args = ["-p", request.prompt];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (request.outputFormat) args.push("--output-format", request.outputFormat);
    return invocation("qwen", withPassthrough(args, request));
  }
}

class AiderHarness implements Harness {
  readonly name = "aider";

  build(request: NormalizedRequest): Invocation {
    const args = ["--message", request.prompt];
    const model = providerQualifiedModel(request);
    if (model) args.push("--model", model);
    return invocation("aider", withPassthrough(args, request));
  }
}

class AmazonQHarness implements Harness {
  readonly name = "amazon-q";

  build(request: NormalizedRequest): Invocation {
    const args = ["chat"];
    if (request.agent) args.push("--agent", request.agent);
    args.push(request.prompt);
    return invocation("q", withPassthrough(args, request));
  }
}

class CopilotHarness implements Harness {
  readonly name = "copilot";

  build(request: NormalizedRequest): Invocation {
    const args = ["-p", request.prompt];
    const model = plainModel(request);
    if (model) args.push("--model", model);
    if (request.agent) args.push("--agent", request.agent);
    return invocation("copilot", withPassthrough(args, request));
  }
}

registerHarness("claude", ClaudeHarness);
registerHarness("codex", CodexHarness);
registerHarness("cursor", CursorHarness);
registerHarness("gemini", GeminiHarness);
registerHarness("goose", GooseHarness);
registerHarness("opencode", OpenCodeHarness);
registerHarness("qwen", QwenHarness);
registerHarness("aider", AiderHarness);
registerHarness("amazon-q", AmazonQHarness);
registerHarness("copilot", CopilotHarness);
