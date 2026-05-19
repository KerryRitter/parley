import { spawn, type ChildProcess } from "node:child_process";

import { createHarness, knownHarnesses, normalizeHarness, registerHarness } from "./harnesses.js";
import { resolveModel } from "./model.js";
import type {
  AgentRequest,
  Harness,
  HarnessName,
  Invocation,
  NormalizedRequest,
  RunAgentOptions,
  RunResult,
  SpawnAgentOptions,
} from "./types.js";

export {
  createHarness,
  knownHarnesses,
  normalizeHarness,
  registerHarness,
  resolveModel,
};

export type {
  AgentRequest,
  Harness,
  HarnessName,
  Invocation,
  NormalizedRequest,
  RunAgentOptions,
  RunResult,
  SpawnAgentOptions,
};

export function normalizeRequest(request: AgentRequest): NormalizedRequest {
  const prompt = mergePrompt(request.stdin, request.prompt);
  if (!prompt) {
    throw new Error("missing prompt; pass prompt or stdin");
  }

  return {
    harness: normalizeHarness(request.harness),
    provider: request.provider,
    model: request.model,
    outputFormat: request.outputFormat,
    inputFormat: request.inputFormat,
    permissionMode: request.permissionMode,
    maxTurns: request.maxTurns === undefined ? undefined : String(request.maxTurns),
    agent: request.agent,
    cwd: request.cwd,
    prompt,
    passthrough: [...(request.passthrough ?? [])],
  };
}

export function buildInvocation(request: AgentRequest): Invocation {
  const normalized = normalizeRequest(request);
  return createHarness(normalized.harness).build(normalized);
}

export function spawnAgent(request: AgentRequest, options: SpawnAgentOptions = {}): ChildProcess {
  const invocation = buildInvocation(request);
  const env = { ...process.env, ...invocation.env };

  return spawn(invocation.command, invocation.args, {
    ...options,
    cwd: request.cwd ?? options.cwd,
    env: { ...env, ...options.env },
    stdio: options.stdio ?? "inherit",
  });
}

export function runAgent(request: AgentRequest, options: RunAgentOptions = {}): Promise<RunResult> {
  const invocation = buildInvocation(request);
  const encoding = options.encoding ?? "utf8";
  const child = spawn(invocation.command, invocation.args, {
    ...options,
    cwd: request.cwd ?? options.cwd,
    env: { ...process.env, ...invocation.env, ...options.env },
    stdio: ["ignore", "pipe", "pipe"],
  });

  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];

  child.stdout?.on("data", (chunk: Buffer) => stdout.push(chunk));
  child.stderr?.on("data", (chunk: Buffer) => stderr.push(chunk));

  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (status, signal) => {
      resolve({
        status,
        signal,
        stdout: Buffer.concat(stdout).toString(encoding),
        stderr: Buffer.concat(stderr).toString(encoding),
        invocation,
      });
    });
  });
}

function mergePrompt(stdin: string | undefined, prompt: string): string {
  const trimmedStdin = stdin?.trimEnd();
  if (trimmedStdin && prompt) return `${trimmedStdin}\n\n${prompt}`;
  return prompt || trimmedStdin || "";
}
