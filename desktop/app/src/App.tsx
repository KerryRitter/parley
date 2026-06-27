import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AiPromptInput,
  Badge,
  Card,
  CardBody,
  Input,
  Popover,
  ScrollArea,
  Select,
  Spinner,
  Switch,
} from "./cruz";
import {
  invoke,
  onChatEvent,
  type AgentInfo,
  type AgentList,
  type AgentUsage,
  type ChatEvent,
  type GitDiff,
  type SendReq,
} from "./bridge";
import { CODE_MAP, DEFAULT_PANEL, NEEDS_PROVIDER, PRESETS, color, display } from "./agents";

const CHAT_ID = "main";

interface Pane {
  agent: string;
  text: string;
  warm: boolean;
  done: boolean;
  code?: number;
  ms?: number;
}
interface Msg {
  id: string;
  role: "user" | "assistant";
  text?: string;
  target: string;
  judge?: string;
  isFuse: boolean;
  panes: Record<string, Pane>;
  order: string[];
  status: string;
  consensus?: { agree?: string; clash?: string };
  done: boolean;
}

// ---- tiny presentational helpers ------------------------------------------

function Dot({ agent, size = 9 }: { agent: string; size?: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: 999,
        background: color(agent),
        boxShadow: `0 0 10px -1px ${color(agent)}`,
        display: "inline-block",
        flexShrink: 0,
      }}
    />
  );
}

function AgentAvatar({ agent, label }: { agent: string; label: string }) {
  const glyph = agent === "fused" ? "⚖" : label.charAt(0).toUpperCase();
  return (
    <span
      className="grid place-items-center font-extrabold"
      style={{
        width: 26,
        height: 26,
        borderRadius: 8,
        background: color(agent),
        color: "#0c0d11",
        fontSize: 12,
        boxShadow: `0 0 16px -4px ${color(agent)}`,
      }}
    >
      {glyph}
    </span>
  );
}

function fmtMs(ms?: number) {
  if (!ms) return "";
  return ms >= 1000 ? (ms / 1000).toFixed(1) + "s" : Math.round(ms) + "ms";
}

function renderText(raw: string) {
  // light formatting: fenced code + inline code, everything escaped
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const parts = raw.split(/```/);
  let html = "";
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) {
      const body = parts[i].replace(/^[a-zA-Z0-9_+-]*\n/, "");
      html += `<pre class="cz-pre">${esc(body.replace(/\n$/, ""))}</pre>`;
    } else {
      html += esc(parts[i]).replace(/`([^`]+)`/g, '<code class="cz-code">$1</code>');
    }
  }
  return { __html: html };
}

function section(text: string, heading: string) {
  const re = new RegExp(`${heading}[^\\n]*\\n([\\s\\S]*?)(?:\\n[A-Z][A-Z ]{3,}|$)`);
  const m = text.match(re);
  return m
    ? m[1]
        .split("\n")
        .map((l) => l.trim())
        .filter(Boolean)
        .slice(0, 4)
        .join(" · ")
    : "";
}

// ---- the app --------------------------------------------------------------

export function App() {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [primary, setPrimary] = useState("claude");
  const [fuse, setFuse] = useState(false);
  const [panel, setPanel] = useState<Set<string>>(new Set(DEFAULT_PANEL));
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const [cwd, setCwd] = useState("");
  const [yolo, setYolo] = useState(true);
  const [messages, setMessages] = useState<Msg[]>([]);
  const [busy, setBusy] = useState(false);
  const [input, setInput] = useState("");
  const [cockpit, setCockpit] = useState(false);
  const [diff, setDiff] = useState<GitDiff | null>(null);
  const [usage, setUsage] = useState<AgentUsage[]>([]);
  const threadRef = useRef<HTMLDivElement>(null);
  const seq = useRef(0);

  // load agents
  useEffect(() => {
    invoke<AgentList>("list_agents")
      .then((list) => {
        const a = (list.agents || []).filter((x) => x.name !== "gemini");
        setAgents(a);
        if (!a.find((x) => x.name === "claude" && x.installed)) {
          const first = a.find((x) => x.installed);
          if (first) setPrimary(first.name);
        }
      })
      .catch(() => setAgents([]));
  }, []);

  // stream handler
  useEffect(() => {
    return onChatEvent((e: ChatEvent) => {
      setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== e.msgId) return m;
          const next: Msg = { ...m, panes: { ...m.panes }, order: [...m.order] };
          const ensure = (key: string) => {
            if (!next.panes[key]) {
              next.panes[key] = { agent: e.agent || key, text: "", warm: !!e.warm, done: false };
              next.order.push(key);
            }
            return next.panes[key];
          };
          if (e.kind === "start") {
            const p = ensure(e.pane);
            p.agent = e.agent || p.agent;
            p.warm = !!e.warm;
          } else if (e.kind === "chunk") {
            const p = ensure(e.pane);
            next.panes[e.pane] = { ...p, text: p.text + (e.text || "") };
          } else if (e.kind === "status") {
            next.status = e.text || "";
          } else if (e.kind === "done") {
            const p = ensure(e.pane);
            next.panes[e.pane] = { ...p, done: true, code: e.code, ms: e.ms };
            if (e.pane === "fused" || e.pane === "main") {
              const t = next.panes[e.pane].text;
              const agree = section(t, "CONSENSUS");
              const clash = section(t, "CONTRADICTIONS");
              if (agree || clash) next.consensus = { agree, clash };
              if (e.ms) next.status = `done in ${fmtMs(e.ms)}`;
            }
          } else if (e.kind === "error") {
            const p = ensure(e.pane);
            next.panes[e.pane] = { ...p, text: p.text + "\n⚠ " + (e.text || "error"), done: true };
          }
          return next;
        })
      );
      requestAnimationFrame(() => {
        const el = threadRef.current;
        if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 200) el.scrollTop = el.scrollHeight;
      });
    });
  }, []);

  const judge = primary === "auto" ? "claude" : primary;

  const primaryOptions = useMemo(
    () => [
      { value: "auto", label: "Auto — route each message" },
      ...agents.map((a) => ({
        value: a.name,
        label: display(a.name) + (a.installed ? "" : " (not installed)"),
        disabled: !a.installed,
      })),
    ],
    [agents]
  );

  const panelCandidates = useMemo(
    () => [...DEFAULT_PANEL, ...agents.map((a) => a.name).filter((n) => !DEFAULT_PANEL.includes(n))],
    [agents]
  );

  const refreshDiff = useCallback(() => {
    invoke<GitDiff>("git_diff", { cwd: cwd || null })
      .then(setDiff)
      .catch(() => setDiff(null));
  }, [cwd]);

  const refreshUsage = useCallback(() => {
    invoke<AgentUsage[]>("usage_stats").then(setUsage).catch(() => setUsage([]));
  }, []);

  function parseMention(text: string): { target: string; prompt: string } | null {
    const m = text.match(/^@([a-zA-Z0-9_-]+)\s+([\s\S]+)$/);
    if (!m) return null;
    const tag = m[1].toLowerCase();
    if (tag === "panel" || tag === "fuse") return { target: "fuse", prompt: m[2] };
    if (tag === "auto") return { target: "auto", prompt: m[2] };
    const name = CODE_MAP[tag] || tag;
    if (agents.find((a) => a.name === name)) return { target: name, prompt: m[2] };
    return null;
  }

  async function send(raw: string) {
    const text = raw.trim();
    if (!text || busy) return;
    const mention = parseMention(text);
    const prompt = mention ? mention.prompt : text;
    const fuseOn = mention ? mention.target === "fuse" : fuse;
    const target = fuseOn ? "fuse" : mention ? mention.target : primary;
    const usePanel = fuseOn ? (panel.size >= 2 ? [...panel] : DEFAULT_PANEL.slice()) : [];

    const id = `m${Date.now()}-${seq.current++}`;
    setInput("");
    setMessages((prev) => [
      ...prev,
      { id: id + "-u", role: "user", text: raw, target, isFuse: false, panes: {}, order: [], status: "", done: true },
      {
        id,
        role: "assistant",
        target,
        judge: fuseOn ? judge : undefined,
        isFuse: fuseOn,
        panes: {},
        order: [],
        status: "working…",
        done: false,
      },
    ]);
    setBusy(true);
    requestAnimationFrame(() => {
      const el = threadRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    });

    const req: SendReq = {
      chatId: CHAT_ID,
      msgId: id,
      target,
      model: model || null,
      provider: provider || null,
      prompt,
      cwd: cwd || null,
      panel: usePanel,
      judge: fuseOn ? judge : null,
      yolo,
    };
    try {
      await invoke("send_message", { req });
    } catch (e) {
      setMessages((prev) =>
        prev.map((m) => (m.id === id ? { ...m, status: "error: " + String(e) } : m))
      );
    } finally {
      setBusy(false);
      setMessages((prev) => prev.map((m) => (m.id === id ? { ...m, done: true } : m)));
      if (cockpit) refreshDiff();
    }
  }

  const mentionHint = (() => {
    const m = parseMention(input.trim());
    if (!m) return "";
    return m.target === "fuse" ? "→ this message goes to the panel" : `→ this message goes to ${display(m.target)}`;
  })();

  return (
    <div className="flex flex-col h-screen">
      <Topbar
        primary={primary}
        primaryOptions={primaryOptions}
        onPrimary={setPrimary}
        model={model}
        provider={provider}
        onModel={setModel}
        onProvider={setProvider}
        needsProvider={NEEDS_PROVIDER.has(primary)}
        presets={PRESETS[primary] || []}
        fuse={fuse}
        onFuse={setFuse}
        cockpit={cockpit}
        onCockpit={() => {
          const v = !cockpit;
          setCockpit(v);
          if (v) refreshDiff();
        }}
        onSettings={refreshUsage}
        cwd={cwd}
        onCwd={setCwd}
        yolo={yolo}
        onYolo={setYolo}
        usage={usage}
      />

      {fuse && (
        <PanelBar
          candidates={panelCandidates}
          panel={panel}
          installed={(n) => agents.find((a) => a.name === n)?.installed ?? true}
          onToggle={(n) =>
            setPanel((p) => {
              const next = new Set(p);
              next.has(n) ? next.delete(n) : next.add(n);
              return next;
            })
          }
          judge={judge}
        />
      )}

      <div className="flex-1 min-h-0 flex">
        <div ref={threadRef} className="flex-1 overflow-y-auto">
          {messages.length === 0 ? (
            <Empty onPick={(s) => setInput(s)} />
          ) : (
            <div className="mx-auto max-w-3xl px-6 py-7 flex flex-col gap-6">
              {messages.map((m) =>
                m.role === "user" ? <UserMsg key={m.id} text={m.text || ""} /> : <AssistantMsg key={m.id} m={m} />
              )}
            </div>
          )}
        </div>
        {cockpit && <Cockpit diff={diff} onRefresh={refreshDiff} cwd={cwd} onClose={() => setCockpit(false)} />}
      </div>

      <footer className="px-4 py-3 border-t border-surface-border" style={{ background: "rgba(13,14,19,.7)", backdropFilter: "blur(12px)" }}>
        <div className="mx-auto max-w-3xl">
          <AiPromptInput
            value={input}
            onChange={setInput}
            onSubmit={send}
            loading={busy}
            placeholder="Message your agent…   @agent to direct"
          />
          <div className="text-center mt-2 text-xs" style={{ color: "var(--color-primary)", minHeight: 14 }}>
            {mentionHint}
          </div>
        </div>
      </footer>
    </div>
  );
}

// ---- topbar ---------------------------------------------------------------

function Topbar(props: {
  primary: string;
  primaryOptions: { value: string; label: string; disabled?: boolean }[];
  onPrimary: (v: string) => void;
  model: string;
  provider: string;
  onModel: (v: string) => void;
  onProvider: (v: string) => void;
  needsProvider: boolean;
  presets: string[];
  fuse: boolean;
  onFuse: (v: boolean) => void;
  cockpit: boolean;
  onCockpit: () => void;
  onSettings: () => void;
  cwd: string;
  onCwd: (v: string) => void;
  yolo: boolean;
  onYolo: (v: boolean) => void;
  usage: AgentUsage[];
}) {
  const modelLabel = props.model ? (props.provider ? `${props.provider}/${props.model}` : props.model) : "model";
  return (
    <header
      className="flex items-center gap-3 px-4 py-2.5 border-b border-surface-border"
      style={{ paddingLeft: 84, background: "rgba(13,14,19,.72)", backdropFilter: "blur(18px) saturate(140%)" }}
      data-tauri-drag-region
    >
      <div className="flex items-center gap-2 font-semibold" style={{ fontSize: 15 }}>
        <span style={{ color: "var(--color-primary)", fontSize: 18, filter: "drop-shadow(0 0 10px rgba(240,163,94,.3))" }}>⚖</span>
        Parley
      </div>

      <div className="flex items-center gap-2 flex-1">
        <div style={{ minWidth: 210 }}>
          <Select
            options={props.primaryOptions}
            value={props.primary}
            onChange={(v: string | undefined) => props.onPrimary(v || "claude")}
            size="sm"
          />
        </div>

        <Popover
          placement="bottom-start"
          trigger={
            <button className="cz-pill" data-set={props.model ? "1" : "0"}>
              <span className="font-mono text-xs">{modelLabel}</span>
            </button>
          }
        >
          <div className="p-3 grid gap-3" style={{ minWidth: 250 }}>
            {props.needsProvider && (
              <Input
                label="Provider"
                value={props.provider}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) => props.onProvider(e.target.value)}
                placeholder="anthropic"
                size="sm"
              />
            )}
            <Input
              label="Model"
              value={props.model}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => props.onModel(e.target.value)}
              placeholder="default"
              size="sm"
            />
            {props.presets.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {props.presets.map((m) => (
                  <button key={m} className="cz-preset" onClick={() => props.onModel(m)}>
                    {m}
                  </button>
                ))}
              </div>
            )}
          </div>
        </Popover>

        <div className="flex items-center gap-2 pl-1">
          <Switch checked={props.fuse} onChange={props.onFuse} />
          <span className="text-sm font-medium" style={{ color: props.fuse ? "var(--color-primary)" : "var(--color-text-muted)" }}>
            Fuse
          </span>
        </div>
      </div>

      <Popover
        placement="bottom-end"
        trigger={
          <button className="cz-icon" onClick={props.onSettings} title="Settings">
            ⚙
          </button>
        }
      >
        <div className="p-3 grid gap-3" style={{ minWidth: 280 }}>
          <Input
            label="Working directory"
            value={props.cwd}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => props.onCwd(e.target.value)}
            placeholder="$HOME"
            size="sm"
          />
          <div className="flex items-center gap-2">
            <Switch checked={props.yolo} onChange={props.onYolo} size="sm" />
            <span className="text-xs text-text-secondary">Allow agents to act without prompting (yolo)</span>
          </div>
          <div>
            <div className="text-[10px] uppercase tracking-wide text-text-tertiary mb-1">This session</div>
            {props.usage.length === 0 ? (
              <div className="text-xs text-text-muted">No agent calls yet.</div>
            ) : (
              props.usage.map((u) => (
                <div key={u.agent} className="flex items-center gap-2 text-xs py-1 border-b border-surface-border">
                  <Dot agent={u.agent} size={8} />
                  <span className="flex-1 text-text">{display(u.agent)}</span>
                  {u.warm && <Badge color="success" variant="subtle" size="sm">warm</Badge>}
                  <span className="text-text-muted tabular-nums">{u.calls}×</span>
                  <span className="text-text-muted tabular-nums">{fmtMs(u.totalMs)}</span>
                </div>
              ))
            )}
          </div>
        </div>
      </Popover>

      <button className={"cz-icon" + (props.cockpit ? " cz-icon-on" : "")} onClick={props.onCockpit} title="Code cockpit">
        {"</>"}
      </button>
    </header>
  );
}

// ---- fuse panel bar -------------------------------------------------------

function PanelBar(props: {
  candidates: string[];
  panel: Set<string>;
  installed: (n: string) => boolean;
  onToggle: (n: string) => void;
  judge: string;
}) {
  return (
    <div
      className="flex items-center gap-3 px-5 py-2 border-b border-surface-border"
      style={{ background: "linear-gradient(180deg, rgba(240,163,94,.10), transparent)" }}
    >
      <span className="text-[10px] uppercase tracking-wider font-bold" style={{ color: "var(--color-primary)" }}>
        Panel
      </span>
      <div className="flex gap-1.5 flex-1">
        {props.candidates.map((n) => {
          const on = props.panel.has(n);
          return (
            <button
              key={n}
              className="cz-chip"
              data-on={on ? "1" : "0"}
              disabled={!props.installed(n)}
              onClick={() => props.onToggle(n)}
            >
              <Dot agent={n} size={7} />
              {display(n)}
            </button>
          );
        })}
      </div>
      <span className="text-xs text-text-muted">
        judged by <b className="text-text">{display(props.judge)}</b>
      </span>
    </div>
  );
}

// ---- messages -------------------------------------------------------------

function UserMsg({ text }: { text: string }) {
  return (
    <div className="flex">
      <div
        className="ml-auto max-w-[80%] px-4 py-2.5 whitespace-pre-wrap"
        style={{
          background: "linear-gradient(180deg, var(--color-surface-light), var(--color-surface))",
          border: "1px solid var(--color-surface-border)",
          borderRadius: "16px 16px 4px 16px",
        }}
      >
        {text}
      </div>
    </div>
  );
}

function AssistantMsg({ m }: { m: Msg }) {
  const fusedPane = m.panes["fused"];
  const mainPane = m.panes["main"];
  const panelPanes = m.order.filter((k) => k !== "fused" && k !== "main").map((k) => m.panes[k]);
  const headAgent = m.isFuse ? "fused" : mainPane?.agent || m.target;
  const headLabel = m.isFuse ? "Fused" : display(mainPane?.agent || m.target);
  const warm = !m.isFuse && mainPane?.warm;

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-center gap-2.5">
        <AgentAvatar agent={headAgent} label={headLabel} />
        <span className="font-semibold" style={{ fontSize: 13.5 }}>
          {m.isFuse ? "Fused" : m.target === "auto" ? `Auto → ${headLabel}` : headLabel}
        </span>
        {warm && <Badge color="success" variant="subtle" size="sm">warm</Badge>}
        {m.isFuse && m.judge && <span className="text-xs text-text-muted">judge {display(m.judge)}</span>}
      </div>

      {m.isFuse && panelPanes.length > 0 && (
        <div className="grid gap-2.5" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(230px, 1fr))" }}>
          {panelPanes.map((p) => (
            <Card key={p.agent} variant="outlined" padding="none">
              <div
                className="flex items-center gap-2 px-3 py-2 text-xs border-b border-surface-border"
                style={{ borderLeft: `2px solid ${color(p.agent)}` }}
              >
                <span className="font-semibold text-text">{display(p.agent)}</span>
                {p.warm && <Badge color="success" variant="subtle" size="sm">warm</Badge>}
                <span className="ml-auto">
                  {p.done ? (
                    <span style={{ color: p.code === 0 ? "var(--color-success)" : "var(--color-danger)" }}>
                      {p.code === 0 ? "✓" : "✕"} {fmtMs(p.ms)}
                    </span>
                  ) : (
                    <Spinner size="xs" />
                  )}
                </span>
              </div>
              <div
                className="px-3 py-2.5 text-[13px] whitespace-pre-wrap overflow-y-auto"
                style={{ maxHeight: 320, color: "#d3d7df" }}
                dangerouslySetInnerHTML={renderText(p.text)}
              />
            </Card>
          ))}
        </div>
      )}

      {m.consensus && (m.consensus.agree || m.consensus.clash) && (
        <div className="grid gap-1.5">
          {m.consensus.agree && (
            <div className="flex gap-2.5 items-start text-[13px] px-3 py-2 rounded-lg" style={{ background: "var(--color-surface)", border: "1px solid var(--color-surface-border)" }}>
              <Badge color="success" variant="subtle" size="sm">agree</Badge>
              <span style={{ color: "#d3d7df" }}>{m.consensus.agree}</span>
            </div>
          )}
          {m.consensus.clash && (
            <div className="flex gap-2.5 items-start text-[13px] px-3 py-2 rounded-lg" style={{ background: "var(--color-surface)", border: "1px solid var(--color-surface-border)" }}>
              <Badge color="warning" variant="subtle" size="sm">clash</Badge>
              <span style={{ color: "#d3d7df" }}>{m.consensus.clash}</span>
            </div>
          )}
        </div>
      )}

      {(() => {
        const ans = m.isFuse ? fusedPane : mainPane;
        if (!ans) return m.done ? null : <div className="flex items-center gap-2 text-xs text-text-muted"><Spinner size="xs" /> {m.status}</div>;
        return (
          <div
            className="whitespace-pre-wrap text-[14px]"
            style={
              m.isFuse
                ? {
                    border: "1px solid rgba(240,163,94,.4)",
                    borderRadius: 14,
                    padding: "14px 16px",
                    background: "linear-gradient(180deg, rgba(240,163,94,.07), transparent 55%), var(--color-surface)",
                  }
                : { color: "#e3e6ec" }
            }
            dangerouslySetInnerHTML={renderText(ans.text)}
          />
        );
      })()}

      {!m.done && (
        <div className="flex items-center gap-2 text-xs text-text-muted">
          <Spinner size="xs" /> {m.status}
        </div>
      )}
    </div>
  );
}

// ---- empty state + cockpit ------------------------------------------------

function Empty({ onPick }: { onPick: (s: string) => void }) {
  const suggestions = [
    "Design a rate limiter for a multi-tenant API",
    "Explain what this repo does",
    "Find the security holes in this approach",
  ];
  return (
    <div className="mx-auto text-center px-6" style={{ maxWidth: 560, marginTop: "11vh", color: "var(--color-text-muted)" }}>
      <div style={{ fontSize: 46, color: "var(--color-primary)", filter: "drop-shadow(0 0 26px rgba(240,163,94,.25))" }}>⚖</div>
      <h1 className="font-bold" style={{ fontSize: 25, color: "var(--color-text)", margin: "16px 0 10px", letterSpacing: "-0.3px" }}>
        One chat. Every agent.
      </h1>
      <p style={{ fontSize: 14.5, lineHeight: 1.65 }}>
        Work with your <b style={{ color: "var(--color-text)" }}>primary</b> agent. Flip on{" "}
        <b style={{ color: "var(--color-text)" }}>Fuse</b> and the same message fans out to a panel — your primary
        becomes the judge that synthesizes one answer. Type <code className="cz-code">@agent</code> to direct a single
        message.
      </p>
      <div className="flex flex-wrap gap-2 justify-center mt-6">
        {suggestions.map((s) => (
          <button key={s} className="cz-suggestion" onClick={() => onPick(s)}>
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

function Cockpit(props: { diff: GitDiff | null; onRefresh: () => void; cwd: string; onClose: () => void }) {
  const d = props.diff;
  return (
    <aside className="flex flex-col border-l border-surface-border" style={{ width: 400, flexShrink: 0, background: "var(--color-surface)" }}>
      <div className="flex items-center justify-between px-3.5 py-2.5 border-b border-surface-border text-xs font-semibold text-text-muted">
        <span>Working tree</span>
        <div className="flex gap-1.5">
          <button className="cz-mini" onClick={props.onRefresh}>refresh</button>
          <button className="cz-mini" onClick={props.onClose}>close</button>
        </div>
      </div>
      <div className="px-3.5 py-2 border-b border-surface-border font-mono text-xs" style={{ maxHeight: 150, overflowY: "auto" }}>
        {!d || !d.isRepo ? (
          <span className="italic text-text-tertiary">not a git repo</span>
        ) : d.files.length === 0 ? (
          <span className="italic text-text-tertiary">working tree clean</span>
        ) : (
          d.files.map((f, i) => (
            <div key={i} className="whitespace-pre" style={{ color: "#d3d7df" }}>
              {f}
            </div>
          ))
        )}
      </div>
      <ScrollArea>
        <pre className="m-0 px-3.5 py-3 font-mono whitespace-pre" style={{ fontSize: 11.8, lineHeight: 1.5, color: "#aeb6c4" }}>
          {d?.diff || ""}
        </pre>
      </ScrollArea>
    </aside>
  );
}
