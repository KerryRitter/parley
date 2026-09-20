import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";

export const PARLEY_PROTOCOL_VERSION = 1 as const;

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
  | "kimi"
  | "antigravity"
  | "muse"
  | "pi"
  | "fuse"
  | (string & {});

export interface RunTimeouts {
  /** Total wall-clock budget. Zero disables this timeout. */
  overallMs?: number;
  /** Maximum gap between stdout/stderr bytes. Zero disables this timeout. */
  idleMs?: number;
}

export interface AgentRequest {
  harness: HarnessName;
  prompt: string;
  provider?: string;
  model?: string;
  agent?: string;
  cwd?: string;
  outputFormat?: "text" | "json" | "stream-json" | (string & {});
  inputFormat?: string;
  permissionMode?: string;
  maxTurns?: string | number;
  sessionId?: string;
  resumeId?: string;
  passthrough?: string[];
  /** Permission bypass is opt-in for SDK calls. Defaults to false. */
  yolo?: boolean;
  /** Replace the adapter's normal binary with a wrapper or compatible CLI. */
  executable?: string;
  /** Applied to the child only. Values travel over stdin, never argv. */
  env?: Record<string, string>;
  /** Remove variables after adapter defaults. Explicit `env` values win. */
  unsetEnv?: string[];
  /** Set false to clear the child environment before applying `env`. */
  inheritEnv?: boolean;
  timeout?: RunTimeouts;
  /** Bytes retained in each final output tail. Stream events are never truncated. */
  maxCaptureChars?: number;
  signal?: AbortSignal;
}

interface WireAgentRequest extends Omit<AgentRequest, "signal"> {
  protocolVersion: typeof PARLEY_PROTOCOL_VERSION;
}

export type RunStatus = "completed" | "failed" | "cancelled" | "timed_out";
export type TerminationReason = "cancelled" | "overall_timeout" | "idle_timeout";

export interface RunResult {
  status: RunStatus;
  terminationReason: TerminationReason | null;
  exitCode: number | null;
  output: string;
  stderr: string;
  durationMs: number;
  sessionId: string | null;
}

export interface StartedEvent {
  protocolVersion: number;
  type: "started";
  harness: string;
  pid: number;
}

export interface OutputEvent {
  protocolVersion: number;
  type: "stdout" | "stderr";
  sequence: number;
  data: string;
}

export interface CompletedEvent {
  protocolVersion: number;
  type: "completed";
  result: RunResult;
}

export type RunEvent = StartedEvent | OutputEvent | CompletedEvent;

export interface HarnessCapabilities {
  protocolVersion: number;
  harness: string;
  headless: boolean;
  model: boolean;
  provider: boolean;
  agent: boolean;
  structuredOutput: boolean;
  inputFormat: boolean;
  permissionMode: boolean;
  maxTurns: boolean;
  sessionId: boolean;
  resume: boolean;
  yolo: boolean;
  passthrough: boolean;
  executableOverride: boolean;
  environmentOverride: boolean;
  cancellation: boolean;
  liveSteering: boolean;
}

export interface ParleyClientOptions {
  /** Path or command name for the `par` binary. Defaults to PARLEY_BIN or `par`. */
  binary?: string;
  /** Extra environment for the Parley transport process itself. */
  processEnv?: NodeJS.ProcessEnv;
}

export interface OpenAICompatibleEnvironmentOptions {
  baseUrl: string;
  apiKey?: string | undefined;
  /** Defaults to OPENAI_BASE_URL. */
  baseUrlEnv?: string;
  /** Defaults to OPENAI_API_KEY. */
  apiKeyEnv?: string;
  extra?: Record<string, string>;
}

/** Build environment overrides for OpenAI-compatible endpoints such as exo. */
export function openAICompatibleEnv(
  options: OpenAICompatibleEnvironmentOptions,
): Record<string, string> {
  const env: Record<string, string> = { ...options.extra };
  env[options.baseUrlEnv ?? "OPENAI_BASE_URL"] = options.baseUrl;
  if (options.apiKey !== undefined) {
    env[options.apiKeyEnv ?? "OPENAI_API_KEY"] = options.apiKey;
  }
  return env;
}

/** One running agent process. It is both an event stream and a result handle. */
export class ParleyRun implements AsyncIterable<RunEvent> {
  readonly transportPid: number | undefined;
  agentPid: number | undefined;
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly events = new AsyncQueue<RunEvent>();
  private readonly resultPromise: Promise<RunResult>;
  private resolveResult!: (result: RunResult) => void;
  private rejectResult!: (error: Error) => void;
  private transportStderr = "";
  private sawCompletion = false;
  private abortCleanup?: () => void;

  constructor(binary: string, request: AgentRequest, processEnv: NodeJS.ProcessEnv) {
    this.resultPromise = new Promise<RunResult>((resolve, reject) => {
      this.resolveResult = resolve;
      this.rejectResult = reject;
    });
    this.child = spawn(binary, ["sdk", "run"], {
      env: processEnv,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    // EPIPE is expected when the transport fails before consuming its request;
    // the child error/close handlers below produce the useful diagnostic.
    this.child.stdin.on("error", () => undefined);
    this.transportPid = this.child.pid;
    this.consumeProtocol();

    const { signal, ...wireFields } = request;
    const wireRequest: WireAgentRequest = {
      protocolVersion: PARLEY_PROTOCOL_VERSION,
      ...wireFields,
    };
    this.child.stdin.write(`${JSON.stringify(wireRequest)}\n`);

    if (signal) {
      const abort = (): void => this.cancel();
      if (signal.aborted) {
        abort();
      } else {
        signal.addEventListener("abort", abort, { once: true });
        this.abortCleanup = () => signal.removeEventListener("abort", abort);
      }
    }
  }

  /** Ask Parley to terminate this run and its descendant processes. */
  cancel(): void {
    if (!this.sawCompletion && this.child.stdin.writable) {
      this.child.stdin.write('{"type":"cancel"}\n');
    }
  }

  /** Resolve after the terminal completed event, regardless of agent exit status. */
  result(): Promise<RunResult> {
    return this.resultPromise;
  }

  [Symbol.asyncIterator](): AsyncIterator<RunEvent> {
    return this.events[Symbol.asyncIterator]();
  }

  private consumeProtocol(): void {
    let pending = "";
    this.child.stdout.setEncoding("utf8");
    this.child.stdout.on("data", (chunk: string) => {
      pending += chunk;
      while (true) {
        const newline = pending.indexOf("\n");
        if (newline < 0) break;
        const line = pending.slice(0, newline);
        pending = pending.slice(newline + 1);
        if (line.trim()) this.acceptLine(line);
      }
    });
    this.child.stdout.on("end", () => {
      if (pending.trim()) this.acceptLine(pending);
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk: string) => {
      this.transportStderr += chunk;
    });
    this.child.on("error", (error) => this.fail(error));
    this.child.on("close", (code, signal) => {
      this.abortCleanup?.();
      if (!this.sawCompletion) {
        const detail = this.transportStderr.trim();
        const suffix = detail ? `: ${detail}` : "";
        this.fail(
          new Error(
            `Parley transport exited before a completed event (code=${String(code)}, signal=${String(signal)})${suffix}`,
          ),
        );
      }
    });
  }

  private acceptLine(line: string): void {
    let value: unknown;
    try {
      value = JSON.parse(line);
    } catch (cause) {
      this.fail(new Error(`Invalid JSONL event from Parley: ${line}`, { cause }));
      return;
    }
    if (!isRunEvent(value)) {
      this.fail(new Error(`Unknown event from Parley: ${line}`));
      return;
    }
    this.events.push(value);
    if (value.type === "started") this.agentPid = value.pid;
    if (value.type === "completed") {
      this.sawCompletion = true;
      this.abortCleanup?.();
      this.child.stdin.end();
      this.resolveResult(value.result);
      this.events.close();
    }
  }

  private fail(error: Error): void {
    if (this.sawCompletion) return;
    this.sawCompletion = true;
    this.abortCleanup?.();
    if (this.child.stdin.writable) {
      this.child.stdin.write('{"type":"cancel"}\n');
      this.child.stdin.end();
    }
    this.rejectResult(error);
    this.events.close(error);
  }
}

export class ParleyClient {
  private readonly binary: string;
  private readonly processEnv: NodeJS.ProcessEnv;

  constructor(options: ParleyClientOptions = {}) {
    this.binary = options.binary ?? process.env.PARLEY_BIN ?? "par";
    this.processEnv = { ...process.env, ...options.processEnv };
  }

  start(request: AgentRequest): ParleyRun {
    return new ParleyRun(this.binary, request, this.processEnv);
  }

  async run(request: AgentRequest): Promise<RunResult> {
    return this.start(request).result();
  }

  capabilities(harness: HarnessName): Promise<HarnessCapabilities>;
  capabilities(): Promise<HarnessCapabilities[]>;
  async capabilities(harness?: HarnessName): Promise<HarnessCapabilities | HarnessCapabilities[]> {
    const args = ["sdk", "capabilities"];
    if (harness !== undefined) args.push("--harness", harness);
    const value = await collectJson(this.binary, args, this.processEnv);
    if (harness !== undefined) return value as HarnessCapabilities;
    const response = value as { harnesses?: HarnessCapabilities[] };
    if (!Array.isArray(response.harnesses)) {
      throw new Error("Parley returned an invalid capabilities response");
    }
    return response.harnesses;
  }
}

function isRunEvent(value: unknown): value is RunEvent {
  if (!value || typeof value !== "object" || !("type" in value)) return false;
  const event = value as Record<string, unknown>;
  if (event.protocolVersion !== PARLEY_PROTOCOL_VERSION) return false;
  switch (event.type) {
    case "started":
      return typeof event.harness === "string" && typeof event.pid === "number";
    case "stdout":
    case "stderr":
      return typeof event.sequence === "number" && typeof event.data === "string";
    case "completed":
      return isRunResult(event.result);
    default:
      return false;
  }
}

function isRunResult(value: unknown): value is RunResult {
  if (!value || typeof value !== "object") return false;
  const result = value as Record<string, unknown>;
  return (
    ["completed", "failed", "cancelled", "timed_out"].includes(String(result.status)) &&
    (["cancelled", "overall_timeout", "idle_timeout"].includes(
      String(result.terminationReason),
    ) || result.terminationReason === null) &&
    (typeof result.exitCode === "number" || result.exitCode === null) &&
    typeof result.output === "string" &&
    typeof result.stderr === "string" &&
    typeof result.durationMs === "number" &&
    (typeof result.sessionId === "string" || result.sessionId === null)
  );
}

function collectJson(
  binary: string,
  args: string[],
  env: NodeJS.ProcessEnv,
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      env,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => (stdout += chunk));
    child.stderr.on("data", (chunk: string) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(stderr.trim() || `Parley exited with code ${String(code)}`));
        return;
      }
      try {
        const value: unknown = JSON.parse(stdout);
        if (
          !value ||
          typeof value !== "object" ||
          (value as Record<string, unknown>).protocolVersion !== PARLEY_PROTOCOL_VERSION
        ) {
          reject(new Error("Parley returned an unsupported capability protocol version"));
          return;
        }
        resolve(value);
      } catch (cause) {
        reject(new Error("Parley returned invalid capability JSON", { cause }));
      }
    });
  });
}

class AsyncQueue<T> implements AsyncIterable<T> {
  private readonly values: T[] = [];
  private readonly waiters: Array<{
    resolve: (result: IteratorResult<T>) => void;
    reject: (error: Error) => void;
  }> = [];
  private done = false;
  private error: Error | undefined;

  push(value: T): void {
    if (this.done) return;
    const waiter = this.waiters.shift();
    if (waiter) waiter.resolve({ value, done: false });
    else this.values.push(value);
  }

  close(error?: Error): void {
    if (this.done) return;
    this.done = true;
    this.error = error;
    for (const waiter of this.waiters.splice(0)) {
      if (error) waiter.reject(error);
      else waiter.resolve({ value: undefined, done: true });
    }
  }

  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        const value = this.values.shift();
        if (value !== undefined) return Promise.resolve({ value, done: false });
        if (this.error) return Promise.reject(this.error);
        if (this.done) return Promise.resolve({ value: undefined, done: true });
        return new Promise<IteratorResult<T>>((resolve, reject) => {
          this.waiters.push({ resolve, reject });
        });
      },
    };
  }
}
