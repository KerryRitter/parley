// The bridge to the Rust Tauri backend. In the real app it uses @tauri-apps/api;
// in a plain browser (Vite dev / Playwright) it falls back to a mock that streams
// realistic chat-events, so the UI can be developed and verified without Tauri.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export interface AgentInfo {
  name: string;
  installed: boolean;
}
export interface AgentList {
  meta: string[];
  agents: AgentInfo[];
  defaultPanel: string[];
}
export interface ChatEvent {
  chatId: string;
  msgId: string;
  pane: string;
  kind: "start" | "chunk" | "status" | "done" | "error";
  text?: string;
  agent?: string;
  warm?: boolean;
  code?: number;
  ms?: number;
  cmd?: string;
}
export interface SendReq {
  chatId: string;
  msgId: string;
  target: string;
  model?: string | null;
  provider?: string | null;
  prompt: string;
  cwd?: string | null;
  panel: string[];
  judge?: string | null;
  yolo: boolean;
}
export interface GitDiff {
  isRepo: boolean;
  branch: string;
  files: string[];
  diff: string;
}
export interface AgentUsage {
  agent: string;
  calls: number;
  totalMs: number;
  warm: boolean;
}

export const IS_TAURI =
  typeof window !== "undefined" &&
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);

type Handler = (e: ChatEvent) => void;
const handlers = new Set<Handler>();
function dispatch(e: ChatEvent) {
  handlers.forEach((h) => h(e));
}
export function onChatEvent(cb: Handler): () => void {
  handlers.add(cb);
  return () => handlers.delete(cb);
}

if (IS_TAURI) {
  void tauriListen<ChatEvent>("chat-event", (ev) => dispatch(ev.payload));
}

export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (IS_TAURI) return tauriInvoke<T>(cmd, args);
  return mockInvoke(cmd, args) as Promise<T>;
}

/// Open a native folder picker (Tauri) or fall back to a prompt in the browser.
export async function pickFolder(): Promise<string | null> {
  if (IS_TAURI) {
    const r = await openDialog({ directory: true, multiple: false });
    return typeof r === "string" ? r : null;
  }
  const v = window.prompt("Working directory path:");
  return v && v.trim() ? v.trim() : null;
}

/// Save a pasted image; returns the saved file path.
export async function savePaste(name: string, bytes: Uint8Array): Promise<string> {
  return invoke<string>("save_paste", { name, data: Array.from(bytes) });
}

// ---- mock backend (browser preview only) ----------------------------------

const seen = new Set<string>();
const usage: Record<string, AgentUsage> = {};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const SAMPLE: Record<string, string[]> = {
  claude: [
    "A token bucket per tenant is the cleanest fit here.",
    "Keep the counter in Redis with a per-tenant key",
    "and refill lazily on read to avoid a sweeper job.",
  ],
  codex: [
    "Use a sliding-window log if you need exactness,",
    "otherwise a token bucket is simpler and cheaper.",
  ],
  antigravity: [
    "Token bucket. Store `{tokens, last_refill}` per tenant",
    "and compute the refill on each request.",
  ],
  cursor: ["I'd put the limiter in middleware,", "keyed by the tenant id from the auth context."],
  opencode: ["A leaky bucket smooths bursts better for shared infra."],
};

const BIN: Record<string, string> = {
  claude: "claude -p «prompt»",
  codex: "codex exec «prompt»",
  antigravity: "agy «prompt»",
  cursor: "cursor-agent -p «prompt»",
  opencode: "opencode run «prompt»",
};

async function streamPane(req: SendReq, pane: string, agent: string, lines: string[], warm: boolean) {
  const base = BIN[agent] ?? `${agent} «prompt»`;
  const cmd = `${base}${warm ? " --resume 1a2b3c" : ""} --dangerously-skip-permissions`;
  dispatch({ chatId: req.chatId, msgId: req.msgId, pane, kind: "start", agent, warm, cmd });
  for (const ln of lines) {
    await sleep(85);
    dispatch({ chatId: req.chatId, msgId: req.msgId, pane, kind: "chunk", text: ln + "\n" });
  }
  await sleep(70);
  const ms = 600 + lines.length * 110;
  const u = (usage[agent] ??= { agent, calls: 0, totalMs: 0, warm: false });
  u.calls += 1;
  u.totalMs += ms;
  u.warm = u.warm || warm;
  dispatch({ chatId: req.chatId, msgId: req.msgId, pane, kind: "done", code: 0, ms });
}

async function mockSend(req: SendReq) {
  if (req.target === "fuse") {
    const panel = req.panel.length ? req.panel : ["claude", "codex", "antigravity"];
    await Promise.all(panel.map((a) => streamPane(req, a, a, SAMPLE[a] ?? [`(answer from ${a})`], seen.has(a))));
    panel.forEach((a) => seen.add(a));
    const judge = req.judge || "claude";
    const fused = [
      "CONSENSUS",
      "All favor a per-tenant token bucket; refill is computed lazily.",
      "",
      "CONTRADICTIONS",
      "Claude/Antigravity store state in Redis; Codex suggests in-memory for single-node.",
      "",
      "GAPS",
      "Only Codex raised a sliding-window log for strict exactness.",
      "",
      "FINAL ANSWER",
      "Use a per-tenant token bucket: keep {tokens, last_refill} in Redis,",
      "refill lazily on read, and fall back to in-memory only for single-node.",
    ];
    await streamPane(req, "fused", judge, fused, false);
    return;
  }
  const agent = req.target === "auto" ? "antigravity" : req.target;
  await streamPane(req, "main", agent, SAMPLE[agent] ?? [`(answer from ${agent})`], seen.has(agent));
  seen.add(agent);
}

async function mockInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "list_agents":
      return {
        meta: ["auto", "fuse", "solve"],
        agents: [
          { name: "claude", installed: true },
          { name: "codex", installed: true },
          { name: "antigravity", installed: true },
          { name: "cursor", installed: true },
          { name: "opencode", installed: true },
          { name: "qwen", installed: false },
          { name: "kimi", installed: false },
        ],
        defaultPanel: ["claude", "codex", "antigravity"],
      } satisfies AgentList;
    case "send_message":
      await mockSend((args as { req: SendReq }).req);
      return null;
    case "usage_stats":
      return Object.values(usage).sort((a, b) => b.calls - a.calls);
    case "save_paste":
      return `/tmp/parley-attachments/${(args as { name?: string })?.name || "paste.png"}`;
    case "git_diff":
      return {
        isRepo: true,
        branch: "feat/router-steals",
        files: [" M src/main.rs", " M README.md", "?? notes.txt"],
        diff:
          "diff --git a/src/main.rs b/src/main.rs\n@@ -1,3 +1,5 @@\n-fn main() {}\n+fn main() {\n+    println!(\"hello\");\n+}\n",
      } satisfies GitDiff;
    default:
      return null;
  }
}
