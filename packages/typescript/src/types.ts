import type { SpawnOptionsWithoutStdio, StdioOptions } from "node:child_process";

export type HarnessName =
  | "claude"
  | "codex"
  | "cursor"
  | "gemini"
  | "goose"
  | "opencode"
  | "qwen"
  | "aider"
  | "amazon-q"
  | "copilot"
  | "openai"
  | "cursor-agent"
  | "google"
  | "google-gemini"
  | "open-code"
  | "amazonq"
  | "aws-q"
  | "amazon"
  | "github-copilot";

export type OutputFormat = "text" | "json" | "stream-json" | string;

export interface AgentRequest {
  harness?: HarnessName | string;
  provider?: string;
  model?: string;
  outputFormat?: OutputFormat;
  inputFormat?: string;
  permissionMode?: string;
  maxTurns?: string | number;
  agent?: string;
  cwd?: string;
  prompt: string;
  stdin?: string;
  passthrough?: readonly string[];
}

export interface Invocation {
  command: string;
  args: string[];
  env: Record<string, string>;
}

export interface Harness {
  readonly name: string;
  build(request: NormalizedRequest): Invocation;
}

export interface NormalizedRequest {
  harness: string;
  provider: string | undefined;
  model: string | undefined;
  outputFormat: OutputFormat | undefined;
  inputFormat: string | undefined;
  permissionMode: string | undefined;
  maxTurns: string | undefined;
  agent: string | undefined;
  cwd: string | undefined;
  prompt: string;
  passthrough: string[];
}

export interface RunResult {
  status: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
  invocation: Invocation;
}

export interface SpawnAgentOptions extends Omit<SpawnOptionsWithoutStdio, "stdio"> {
  stdio?: StdioOptions;
}

export interface RunAgentOptions extends SpawnOptionsWithoutStdio {
  encoding?: BufferEncoding;
}
