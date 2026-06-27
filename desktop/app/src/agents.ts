// Per-agent presentation: signature colors, display names, model presets, and
// which agents need a provider alongside the model.

export const AGENT_COLOR: Record<string, string> = {
  claude: "#e0825a",
  codex: "#34c08b",
  antigravity: "#7c8cff",
  cursor: "#d7b450",
  opencode: "#54b6c9",
  qwen: "#b074f0",
  kimi: "#ef8a5b",
  copilot: "#8aa0ff",
  goose: "#c98f63",
  "amazon-q": "#e8a13c",
  auto: "#f0a35e",
  fused: "#f0a35e",
};

export function color(agent: string): string {
  return AGENT_COLOR[agent] ?? "#8a90a0";
}

export function display(name: string): string {
  if (name === "auto") return "Auto";
  if (name === "amazon-q") return "Amazon Q";
  return name.charAt(0).toUpperCase() + name.slice(1);
}

export const NEEDS_PROVIDER = new Set(["cursor", "opencode", "aider", "goose"]);

export const PRESETS: Record<string, string[]> = {
  claude: ["sonnet", "opus", "haiku"],
  codex: ["gpt-5.2", "gpt-5.2-codex"],
  antigravity: ["gemini-3-pro", "gemini-3-flash"],
  cursor: ["sonnet-4.5", "gpt-5"],
  opencode: ["claude-sonnet-4-6", "gpt-5"],
};

export const CODE_MAP: Record<string, string> = {
  cl: "claude",
  co: "codex",
  ag: "antigravity",
  cu: "cursor",
  oc: "opencode",
  q: "qwen",
  k: "kimi",
  go: "goose",
  cp: "copilot",
  aq: "amazon-q",
};

export const DEFAULT_PANEL = ["claude", "codex", "antigravity"];
