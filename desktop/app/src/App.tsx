import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Collapsible, CommandPalette, Input, Kbd, Popover, ScrollArea, Spinner, Switch } from "./cruz";
import {
  invoke, listDir, onChatEvent, pickFolder, savePaste,
  type AgentInfo, type AgentList, type AgentUsage, type ChatEvent, type DirListing, type GitDiff, type Panelist, type SendReq,
} from "./bridge";
import { CODE_MAP, DEFAULT_PANEL, NEEDS_PROVIDER, PRESETS, color, display } from "./agents";

interface Pane { agent: string; text: string; warm: boolean; cmd?: string; done: boolean; code?: number; ms?: number; }
interface Msg {
  id: string; role: "user" | "assistant"; text?: string;
  target: string; judge?: string; isFuse: boolean;
  panes: Record<string, Pane>; order: string[]; labels: Record<string, string>;
  status: string; consensus?: { agree?: string; clash?: string }; done: boolean;
}
interface Attachment { path: string; name: string; }
interface Command { key: string; label: string; run: () => void; }

function fmtMs(ms?: number) { if (ms == null) return ""; return ms >= 1000 ? (ms / 1000).toFixed(1) + "s" : Math.round(ms) + "ms"; }
function renderText(raw: string) {
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const parts = raw.split(/```/);
  let html = "";
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) html += `<pre class="cz-pre">${esc(parts[i].replace(/^[a-zA-Z0-9_+-]*\n/, "").replace(/\n$/, ""))}</pre>`;
    else html += esc(parts[i]).replace(/`([^`]+)`/g, '<code class="cz-code">$1</code>');
  }
  return { __html: html };
}
function section(text: string, heading: string) {
  const re = new RegExp(`${heading}[^\\n]*\\n([\\s\\S]*?)(?:\\n[A-Z][A-Z ]{3,}|$)`);
  const m = text.match(re);
  return m ? m[1].split("\n").map((l) => l.trim()).filter(Boolean).slice(0, 4).join(" · ") : "";
}
function plLabel(agent: string, model?: string | null) { return model && model.trim() ? `${display(agent)} · ${model}` : display(agent); }
function Dot({ agent, size = 8, warm = false }: { agent: string; size?: number; warm?: boolean }) {
  return <span className={warm ? "cz-dot-warm" : ""} style={{ width: size, height: size, borderRadius: 999, background: color(agent), boxShadow: `0 0 8px -1px ${color(agent)}`, display: "inline-block", flexShrink: 0 }} />;
}

// ---- tab shell: many consoles at once --------------------------------------

interface Tab { id: string; title: string; }

export function App() {
  const [tabs, setTabs] = useState<Tab[]>([{ id: "c1", title: "console 1" }]);
  const [active, setActive] = useState("c1");
  const [busyMap, setBusyMap] = useState<Record<string, boolean>>({});
  const next = useRef(2);

  const addTab = () => {
    const n = next.current++;
    const id = "c" + n;
    setTabs((t) => [...t, { id, title: "console " + n }]);
    setActive(id);
  };
  const closeTab = (id: string) => {
    setTabs((t) => {
      if (t.length <= 1) return t;
      const idx = t.findIndex((x) => x.id === id);
      const rest = t.filter((x) => x.id !== id);
      if (active === id) setActive(rest[Math.max(0, idx - 1)].id);
      return rest;
    });
  };

  return (
    <div className="flex flex-col h-screen">
      <div className="cz-tabstrip" data-tauri-drag-region>
        <div className="cz-tabstrip-pad" />
        {tabs.map((t) => (
          <div key={t.id} className={"cz-tab" + (t.id === active ? " cz-tab-active" : "")} onClick={() => setActive(t.id)}>
            {busyMap[t.id] ? <span className="cz-tab-dot cz-dot-warm" style={{ background: "var(--color-warning)" }} /> : <span className="cz-tab-dot" style={{ background: "var(--color-text-tertiary)" }} />}
            <span>{t.title}</span>
            {tabs.length > 1 && <span className="cz-tab-x" onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}>×</span>}
          </div>
        ))}
        <button className="cz-tab-add" onClick={addTab} title="New console">+</button>
      </div>
      <div className="flex-1 min-h-0 relative">
        {tabs.map((t) => (
          <div key={t.id} style={{ display: t.id === active ? "block" : "none", height: "100%" }}>
            <Console chatId={t.id} onBusy={(b) => setBusyMap((m) => ({ ...m, [t.id]: b }))} />
          </div>
        ))}
      </div>
    </div>
  );
}

function Console({ chatId, onBusy }: { chatId: string; onBusy: (busy: boolean) => void }) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [primary, setPrimary] = useState("claude");
  const [fuse, setFuse] = useState(false);
  const [panel, setPanel] = useState<Panelist[]>(DEFAULT_PANEL.map((a) => ({ id: a, agent: a })));
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const [cwd, setCwd] = useState("");
  const [yolo, setYolo] = useState(true);
  const [messages, setMessages] = useState<Msg[]>([]);
  const [busy, setBusy] = useState(false);
  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [history, setHistory] = useState<string[]>([]);
  const [cockpit, setCockpit] = useState(false);
  const [diff, setDiff] = useState<GitDiff | null>(null);
  const [usage, setUsage] = useState<AgentUsage[]>([]);
  const [palette, setPalette] = useState(false);
  const [railOpen, setRailOpen] = useState(true);
  const [folderOpen, setFolderOpen] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const seq = useRef(0);
  const pid = useRef(0);
  const histIdx = useRef(-1);

  const usageByAgent = useMemo(() => { const m: Record<string, AgentUsage> = {}; for (const u of usage) m[u.agent] = u; return m; }, [usage]);
  const judge = primary === "auto" ? "claude" : primary;
  const installedCount = agents.filter((a) => a.installed).length;
  const totals = usage.reduce((a, u) => ({ calls: a.calls + u.calls, ms: a.ms + u.totalMs }), { calls: 0, ms: 0 });

  const refreshDiff = useCallback(() => { invoke<GitDiff>("git_diff", { cwd: cwd || null }).then(setDiff).catch(() => setDiff(null)); }, [cwd]);
  const refreshUsage = useCallback(() => { invoke<AgentUsage[]>("usage_stats").then(setUsage).catch(() => setUsage([])); }, []);

  useEffect(() => {
    invoke<AgentList>("list_agents").then((list) => {
      const a = (list.agents || []).filter((x) => x.name !== "gemini");
      setAgents(a);
      if (!a.find((x) => x.name === "claude" && x.installed)) { const f = a.find((x) => x.installed); if (f) setPrimary(f.name); }
    }).catch(() => setAgents([]));
    refreshDiff();
  }, [refreshDiff]);

  useEffect(() => {
    return onChatEvent((e: ChatEvent) => {
      if (e.chatId !== chatId) return; // each console only handles its own chat
      setMessages((prev) => prev.map((m) => {
        if (m.id !== e.msgId) return m;
        const next: Msg = { ...m, panes: { ...m.panes }, order: [...m.order] };
        const ensure = (key: string) => { if (!next.panes[key]) { next.panes[key] = { agent: e.agent || key, text: "", warm: !!e.warm, done: false }; next.order.push(key); } return next.panes[key]; };
        if (e.kind === "start") { const p = ensure(e.pane); next.panes[e.pane] = { ...p, agent: e.agent || p.agent, warm: !!e.warm, cmd: e.cmd }; }
        else if (e.kind === "chunk") { const p = ensure(e.pane); next.panes[e.pane] = { ...p, text: p.text + (e.text || "") }; }
        else if (e.kind === "status") next.status = e.text || "";
        else if (e.kind === "done") {
          const p = ensure(e.pane); next.panes[e.pane] = { ...p, done: true, code: e.code, ms: e.ms };
          if (e.pane === "fused" || e.pane === "main") { const t = next.panes[e.pane].text; const ag = section(t, "CONSENSUS"); const cl = section(t, "CONTRADICTIONS"); if (ag || cl) next.consensus = { agree: ag, clash: cl }; }
        } else if (e.kind === "error") { const p = ensure(e.pane); next.panes[e.pane] = { ...p, text: p.text + "\n⚠ " + (e.text || "error"), done: true, code: 1 }; }
        return next;
      }));
      requestAnimationFrame(() => { const el = logRef.current; if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 240) el.scrollTop = el.scrollHeight; });
    });
  }, [chatId]);

  useEffect(() => { onBusy(busy); }, [busy, onBusy]);

  // panel ops
  const addPanelist = (agent: string) => setPanel((p) => [...p, { id: "p" + ++pid.current, agent, model: "", provider: "" }]);
  const removePanelist = (id: string) => setPanel((p) => p.filter((x) => x.id !== id));
  const patchPanelist = (id: string, patch: Partial<Panelist>) => setPanel((p) => p.map((x) => (x.id === id ? { ...x, ...patch } : x)));

  const commands: Command[] = useMemo(() => [
    { key: "fuse", label: (fuse ? "Disable" : "Enable") + " Fuse", run: () => setFuse((v) => !v) },
    { key: "single", label: "Single agent (Fuse off)", run: () => setFuse(false) },
    { key: "clear", label: "Clear run log", run: () => setMessages([]) },
    { key: "cockpit", label: "Toggle code cockpit", run: () => { setCockpit((v) => !v); refreshDiff(); } },
    { key: "open", label: "Open folder…", run: () => setFolderOpen(true) },
    { key: "auto", label: "Primary → Auto (route each message)", run: () => setPrimary("auto") },
    ...agents.filter((a) => a.installed).map((a) => ({ key: a.name, label: `Primary → ${display(a.name)}`, run: () => setPrimary(a.name) })),
  ], [agents, fuse, refreshDiff]);

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

  async function run() {
    const raw = input.trim();
    if ((!raw && attachments.length === 0) || busy) return;
    const mention = parseMention(raw);
    let prompt = mention ? mention.prompt : raw;
    const fuseOn = mention ? mention.target === "fuse" : fuse;
    const target = fuseOn ? "fuse" : mention ? mention.target : primary;
    const usePanel: Panelist[] = fuseOn ? (panel.length >= 2 ? panel : DEFAULT_PANEL.map((a) => ({ id: a, agent: a }))) : [];
    if (attachments.length) prompt = attachments.map((a) => `[Attached image: ${a.path}]`).join("\n") + "\n" + prompt;

    const labels: Record<string, string> = {};
    if (fuseOn) usePanel.forEach((p) => (labels[p.id] = plLabel(p.agent, p.model)));
    else labels["main"] = target === "auto" ? "" : plLabel(target, model);

    if (raw) setHistory((h) => [...h, raw]);
    histIdx.current = -1;
    const id = `m${Date.now()}-${seq.current++}`;
    setInput(""); setAttachments([]);
    setMessages((prev) => [
      ...prev,
      { id: id + "-u", role: "user", text: raw || "(image)", target, isFuse: false, panes: {}, order: [], labels: {}, status: "", done: true },
      { id, role: "assistant", target, judge: fuseOn ? judge : undefined, isFuse: fuseOn, panes: {}, order: [], labels, status: "running…", done: false },
    ]);
    setBusy(true);
    requestAnimationFrame(() => { const el = logRef.current; if (el) el.scrollTop = el.scrollHeight; });

    const req: SendReq = { chatId, msgId: id, target, model: model || null, provider: provider || null, prompt, cwd: cwd || null, panel: usePanel, judge: fuseOn ? judge : null, yolo };
    try { await invoke("send_message", { req }); }
    catch (e) { setMessages((prev) => prev.map((m) => (m.id === id ? { ...m, status: "error: " + String(e) } : m))); }
    finally { setBusy(false); setMessages((prev) => prev.map((m) => (m.id === id ? { ...m, done: true } : m))); refreshUsage(); refreshDiff(); }
  }

  const paletteItems = commands.map((c) => ({ id: c.key, group: ["fuse", "single", "clear", "cockpit", "open"].includes(c.key) ? "Actions" : "Primary agent", label: c.label, onSelect: c.run }));

  return (
    <div className="flex flex-col h-full">
      {/* topbar */}
      <header className="flex items-center gap-2.5 px-3 h-11 border-b border-surface-border font-mono text-[12.5px]" style={{ background: "rgba(10,11,13,.8)", backdropFilter: "blur(16px)" }}>
        <span className="flex items-center gap-1.5 font-semibold" style={{ fontFamily: "var(--font-sans)" }}><span style={{ color: "var(--color-primary)" }}>⚖</span> parley</span>
        <button className="cz-pill" onClick={() => setFolderOpen(true)} title="Open folder (set working dir)">📂 open</button>
        <div className="flex-1" />
        <button className="cz-kbd-btn" onClick={() => setPalette(true)} title="Command palette"><Kbd>⌘</Kbd><Kbd>K</Kbd></button>
        <div className="flex items-center gap-1.5"><Switch checked={fuse} onChange={setFuse} size="sm" /><span style={{ color: fuse ? "var(--color-primary)" : "var(--color-text-muted)" }}>fuse</span></div>
        <Popover placement="bottom-end" trigger={<button className="cz-pill" data-set={model ? "1" : "0"}>{model ? (provider ? `${provider}/${model}` : model) : "model"}</button>}>
          <div className="p-3 grid gap-3" style={{ minWidth: 250 }}>
            <div className="text-[11px] text-text-tertiary font-mono">primary {display(primary)}</div>
            {NEEDS_PROVIDER.has(primary) && <Input label="Provider" size="sm" value={provider} placeholder="anthropic" onChange={(e: React.ChangeEvent<HTMLInputElement>) => setProvider(e.target.value)} />}
            <Input label="Model" size="sm" value={model} placeholder="default" onChange={(e: React.ChangeEvent<HTMLInputElement>) => setModel(e.target.value)} />
            {(PRESETS[primary] || []).length > 0 && <div className="flex flex-wrap gap-1.5">{(PRESETS[primary] || []).map((mm) => <button key={mm} className="cz-preset" onClick={() => setModel(mm)}>{mm}</button>)}</div>}
          </div>
        </Popover>
        <Popover placement="bottom-end" trigger={<button className="cz-icon" title="Settings">⚙</button>}>
          <div className="p-3 grid gap-3" style={{ minWidth: 280 }}>
            <div className="grid gap-1"><span className="text-[11px] uppercase tracking-wide text-text-tertiary">Working directory</span>
              <div className="flex gap-1.5"><Input size="sm" value={cwd} placeholder="$HOME" onChange={(e: React.ChangeEvent<HTMLInputElement>) => setCwd(e.target.value)} /><button className="cz-mini" onClick={() => setFolderOpen(true)}>browse</button></div>
            </div>
            <div className="flex items-center gap-2"><Switch checked={yolo} onChange={setYolo} size="sm" /><span className="text-xs text-text-secondary">yolo — act without prompting</span></div>
          </div>
        </Popover>
        <button className={"cz-icon" + (cockpit ? " cz-icon-on" : "")} title="Code cockpit" onClick={() => { setCockpit((v) => !v); if (!cockpit) refreshDiff(); }}>{"</>"}</button>
      </header>

      {/* fuse panel builder bar */}
      {fuse && <FusePanelBar panel={panel} agents={agents} judge={judge} onAdd={addPanelist} onRemove={removePanelist} onPatch={patchPanelist} />}

      <div className="flex flex-1 min-h-0">
        {/* slim agents rail (primary picker) */}
        <aside className="flex flex-col border-r border-surface-border" style={{ width: railOpen ? 158 : 46, flexShrink: 0, background: "rgba(255,255,255,.012)", transition: "width .12s" }}>
          <div className="flex items-center justify-between h-8 px-2 text-[10px] uppercase tracking-[0.12em] text-text-tertiary font-mono">
            {railOpen && <span>Primary</span>}
            <button className="cz-mini" style={{ padding: "1px 6px" }} onClick={() => setRailOpen((v) => !v)} title="Collapse">{railOpen ? "‹" : "›"}</button>
          </div>
          <div className="overflow-y-auto flex-1 px-1.5">
            <RailRow name="auto" active={primary === "auto"} installed open={railOpen} onClick={() => setPrimary("auto")} usage={undefined} />
            {agents.map((a) => <RailRow key={a.name} name={a.name} active={primary === a.name} installed={a.installed} open={railOpen} onClick={() => a.installed && setPrimary(a.name)} usage={usageByAgent[a.name]} />)}
          </div>
        </aside>

        {/* run log + composer */}
        <main className="flex-1 flex flex-col min-h-0">
          <div ref={logRef} className="flex-1 overflow-y-auto px-4 py-3 font-mono text-[13px]">
            {messages.length === 0 ? <Empty primary={primary} fuse={fuse} /> : (
              <div className="flex flex-col gap-3 max-w-[1100px]">
                {messages.map((m) => (m.role === "user" ? <PromptLine key={m.id} text={m.text || ""} /> : <RunGroup key={m.id} m={m} />))}
              </div>
            )}
          </div>
          <Composer input={input} setInput={setInput} run={run} busy={busy} fuse={fuse} primary={primary} attachments={attachments} setAttachments={setAttachments} commands={commands} history={history} histIdx={histIdx} inputRef={inputRef}
            mentionHint={(() => { const m = parseMention(input.trim()); return m ? (m.target === "fuse" ? "panel" : display(m.target)) : ""; })()} />
        </main>

        {cockpit && <Cockpit diff={diff} onRefresh={refreshDiff} onClose={() => setCockpit(false)} />}
      </div>

      {/* status bar */}
      <footer className="flex items-center gap-3 h-7 px-3 border-t border-surface-border font-mono text-[11px] text-text-muted" style={{ background: "rgba(255,255,255,.02)" }}>
        <button className="hover:text-text" style={{ color: "var(--color-text-secondary)" }} onClick={() => setFolderOpen(true)} title="Open folder">{cwd || "~"}</button>
        {diff?.isRepo && <span>⎇ {diff.branch || "—"}{diff.files.length ? <span style={{ color: "var(--color-warning)" }}>*{diff.files.length}</span> : ""}</span>}
        <div className="flex-1" />
        <span className="flex items-center gap-1"><Dot agent={primary} size={7} />{display(primary)}</span>
        <span style={{ color: fuse ? "var(--color-primary)" : undefined }}>fuse:{fuse ? `on·${panel.length}` : "off"}</span>
        {model && <span>{provider ? provider + "/" : ""}{model}</span>}
        <span>{installedCount} ready</span>
        <span className="tabular-nums">{totals.calls} runs · {fmtMs(totals.ms)}</span>
      </footer>

      <CommandPalette open={palette} onOpenChange={setPalette} items={paletteItems} placeholder="Run a command…" />
      {folderOpen && <FolderModal initial={cwd} onClose={() => setFolderOpen(false)} onPick={(p) => { setCwd(p); setFolderOpen(false); setTimeout(refreshDiff, 0); }} onNative={async () => { const f = await pickFolder(); if (f) { setCwd(f); setFolderOpen(false); setTimeout(refreshDiff, 0); } }} />}
    </div>
  );
}

// ---- fuse panel builder ----------------------------------------------------

function FusePanelBar({ panel, agents, judge, onAdd, onRemove, onPatch }: {
  panel: Panelist[]; agents: AgentInfo[]; judge: string;
  onAdd: (a: string) => void; onRemove: (id: string) => void; onPatch: (id: string, p: Partial<Panelist>) => void;
}) {
  return (
    <div className="flex items-center gap-2 px-4 py-2 border-b border-surface-border font-mono text-[12px]" style={{ background: "linear-gradient(180deg, rgba(240,163,94,.08), transparent)" }}>
      <span className="text-[10px] uppercase tracking-wider font-bold" style={{ color: "var(--color-primary)" }}>Panel</span>
      <div className="flex items-center gap-1.5 flex-wrap flex-1">
        {panel.map((p) => (
          <Popover key={p.id} placement="bottom-start" trigger={
            <button className="cz-panelist"><Dot agent={p.agent} size={7} /><span className="text-text">{display(p.agent)}</span>{p.model ? <span className="text-text-tertiary">· {p.model}</span> : <span className="text-text-tertiary">· default</span>}<span className="cz-px" onClick={(e) => { e.stopPropagation(); onRemove(p.id); }}>×</span></button>
          }>
            <div className="p-3 grid gap-2.5" style={{ minWidth: 240 }}>
              <div className="text-[11px] text-text-tertiary font-mono">{display(p.agent)} instance</div>
              {NEEDS_PROVIDER.has(p.agent) && <Input label="Provider" size="sm" value={p.provider || ""} placeholder="anthropic" onChange={(e: React.ChangeEvent<HTMLInputElement>) => onPatch(p.id, { provider: e.target.value })} />}
              <Input label="Model" size="sm" value={p.model || ""} placeholder="default" onChange={(e: React.ChangeEvent<HTMLInputElement>) => onPatch(p.id, { model: e.target.value })} />
              {(PRESETS[p.agent] || []).length > 0 && <div className="flex flex-wrap gap-1.5">{(PRESETS[p.agent] || []).map((mm) => <button key={mm} className="cz-preset" onClick={() => onPatch(p.id, { model: mm })}>{mm}</button>)}</div>}
            </div>
          </Popover>
        ))}
        <Popover placement="bottom-start" trigger={<button className="cz-add">+ add</button>}>
          <div className="p-1.5 grid gap-0.5" style={{ minWidth: 170 }}>
            {agents.filter((a) => a.installed).map((a) => (
              <button key={a.name} className="cz-menu-row" onClick={() => onAdd(a.name)}><Dot agent={a.name} size={7} />{display(a.name)}</button>
            ))}
          </div>
        </Popover>
      </div>
      <span className="text-text-muted">judged by <b className="text-text">{display(judge)}</b></span>
    </div>
  );
}

// ---- folder explorer -------------------------------------------------------

function FolderModal({ initial, onClose, onPick, onNative }: { initial: string; onClose: () => void; onPick: (p: string) => void; onNative: () => void }) {
  const [listing, setListing] = useState<DirListing | null>(null);
  const [loading, setLoading] = useState(true);
  const go = useCallback((path: string | null) => { setLoading(true); listDir(path).then((l) => { setListing(l); setLoading(false); }).catch(() => setLoading(false)); }, []);
  useEffect(() => { go(initial || null); }, [go, initial]);

  return (
    <div className="cz-overlay" onClick={onClose}>
      <div className="cz-folder" onClick={(e) => e.stopPropagation()}>
        <div className="cz-folder-head">
          <span className="font-mono text-[12px]">📂 open folder</span>
          <button className="cz-mini" onClick={onNative}>native picker</button>
        </div>
        <div className="cz-folder-path font-mono text-[12px]">{listing?.path || "…"}</div>
        <div className="cz-folder-list">
          {loading ? <div className="p-3 text-text-muted"><Spinner size="xs" /> loading…</div> : (
            <>
              {listing?.parent != null && <button className="cz-dir" onClick={() => go(listing.parent)}><span className="text-text-tertiary">↰</span> ..</button>}
              {(listing?.dirs || []).map((d) => <button key={d.path} className="cz-dir" onClick={() => go(d.path)}><span style={{ color: "var(--color-primary)" }}></span> {d.name}</button>)}
              {listing && listing.dirs.length === 0 && <div className="p-3 text-text-tertiary italic">no sub-folders</div>}
            </>
          )}
        </div>
        <div className="cz-folder-foot">
          <button className="cz-mini" onClick={onClose}>cancel</button>
          <button className="cz-run" disabled={!listing} onClick={() => listing && onPick(listing.path)}>use this folder</button>
        </div>
      </div>
    </div>
  );
}

// ---- composer --------------------------------------------------------------

function Composer(props: {
  input: string; setInput: (v: string) => void; run: () => void; busy: boolean; fuse: boolean; primary: string;
  attachments: Attachment[]; setAttachments: React.Dispatch<React.SetStateAction<Attachment[]>>;
  commands: Command[]; history: string[]; histIdx: React.MutableRefObject<number>; inputRef: React.RefObject<HTMLTextAreaElement>; mentionHint: string;
}) {
  const { input, setInput, run, busy, fuse, primary, attachments, setAttachments, commands, history, histIdx, inputRef } = props;
  const [sel, setSel] = useState(0);
  const slash = input.startsWith("/");
  const query = slash ? input.slice(1).toLowerCase().trim() : "";
  const matches = slash ? commands.filter((c) => c.key.startsWith(query) || c.label.toLowerCase().includes(query)) : [];
  useEffect(() => { setSel(0); }, [input]);
  function grow(t: HTMLTextAreaElement) { t.style.height = "auto"; t.style.height = Math.min(t.scrollHeight, 180) + "px"; }
  async function onPaste(e: React.ClipboardEvent<HTMLTextAreaElement>) {
    const imgs = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith("image/"));
    if (!imgs.length) return;
    e.preventDefault();
    for (const it of imgs) { const file = it.getAsFile(); if (!file) continue; const buf = new Uint8Array(await file.arrayBuffer()); const name = file.name || `paste-${Date.now()}.png`; try { const path = await savePaste(name, buf); setAttachments((a) => [...a, { path, name }]); } catch { /* ignore */ } }
  }
  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (slash && matches.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => (s + 1) % matches.length); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => (s - 1 + matches.length) % matches.length); return; }
      if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); matches[sel]?.run(); setInput(""); return; }
      if (e.key === "Escape") { e.preventDefault(); setInput(""); return; }
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); run(); return; }
    if (e.key === "ArrowUp" && history.length && (input === "" || histIdx.current >= 0)) { e.preventDefault(); histIdx.current = Math.min(histIdx.current + 1, history.length - 1); setInput(history[history.length - 1 - histIdx.current]); return; }
    if (e.key === "ArrowDown" && histIdx.current >= 0) { e.preventDefault(); histIdx.current -= 1; setInput(histIdx.current < 0 ? "" : history[history.length - 1 - histIdx.current]); return; }
  }
  return (
    <div className="border-t border-surface-border px-4 py-3 relative" style={{ background: "rgba(10,11,13,.6)" }}>
      {slash && matches.length > 0 && (
        <div className="cz-slash">
          {matches.slice(0, 8).map((c, i) => (
            <button key={c.key} className={"cz-slash-item" + (i === sel ? " cz-slash-sel" : "")} onMouseEnter={() => setSel(i)} onClick={() => { c.run(); setInput(""); inputRef.current?.focus(); }}>
              <span className="text-text-tertiary">/{c.key}</span><span className="text-text-secondary">{c.label}</span>
            </button>
          ))}
        </div>
      )}
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-1.5 mb-2 max-w-[1100px]">
          {attachments.map((a, i) => <span key={i} className="cz-attach">🖼 {a.name}<button onClick={() => setAttachments((x) => x.filter((_, j) => j !== i))}>×</button></span>)}
        </div>
      )}
      <div className="cz-term">
        <span className="cz-caret" style={{ color: busy ? "var(--color-warning)" : slash ? "var(--color-info)" : "var(--color-primary)" }}>{slash ? "/" : "❯"}</span>
        <textarea ref={inputRef} rows={1} value={input} placeholder={fuse ? "message the panel…   @agent · /command · paste an image" : `message ${display(primary)}…   @agent · /command · paste an image`} onChange={(e) => { setInput(e.target.value); grow(e.target); }} onKeyDown={onKeyDown} onPaste={onPaste} />
        <button className="cz-run" disabled={busy || (!input.trim() && attachments.length === 0)} onClick={run}>{busy ? <Spinner size="xs" /> : <><span>run</span><Kbd>⏎</Kbd></>}</button>
      </div>
      <div className="text-[11px] text-text-tertiary mt-1.5 h-3 font-mono">{props.mentionHint ? <>→ <span style={{ color: "var(--color-primary)" }}>{props.mentionHint}</span></> : slash ? "command — ↑↓ select · ⏎ run" : ""}</div>
    </div>
  );
}

function RailRow({ name, active, installed, open, onClick, usage }: { name: string; active: boolean; installed: boolean; open: boolean; onClick: () => void; usage?: AgentUsage }) {
  return (
    <button className={"cz-rail-row" + (active ? " cz-rail-active" : "")} onClick={onClick} disabled={!installed} title={installed ? name : name + " (not installed)"} style={{ justifyContent: open ? "flex-start" : "center" }}>
      <Dot agent={name} size={8} warm={!!usage?.warm} />
      {open && <><span className="flex-1 text-left truncate" style={{ opacity: installed ? 1 : 0.4 }}>{display(name)}</span>{usage?.warm && <span className="cz-warm-tag">warm</span>}{usage && usage.calls > 0 && <span className="text-text-tertiary tabular-nums">{usage.calls}</span>}</>}
    </button>
  );
}

function PromptLine({ text }: { text: string }) {
  return <div className="flex gap-2 items-start pt-1"><span style={{ color: "var(--color-primary)" }}>❯</span><span className="whitespace-pre-wrap text-text">{text}</span></div>;
}

function RunGroup({ m }: { m: Msg }) {
  const panelPanes = m.order.filter((k) => k !== "fused" && k !== "main").map((k) => k);
  const fused = m.panes["fused"];
  const main = m.panes["main"];
  const label = (key: string, agent: string) => m.labels[key] || display(agent);

  if (!m.isFuse) {
    return <div className="pl-4">{main ? <RunBlock p={main} title={label("main", main.agent)} auto={m.target === "auto"} /> : !m.done && <div className="flex items-center gap-2 text-text-muted"><Spinner size="xs" /> {m.status}</div>}</div>;
  }
  return (
    <div className="pl-4 flex flex-col gap-2">
      {fused ? <RunBlock p={fused} title="fused" fused />
        : <div className="cz-run-block" style={{ borderLeftColor: "var(--color-primary)" }}><div className="cz-run-head"><Dot agent="fused" size={8} /><span className="font-semibold" style={{ color: "var(--color-text)" }}>fused</span><div className="flex-1" /><Spinner size="xs" /><span className="text-text-tertiary">synthesizing…</span></div></div>}
      {m.consensus && (m.consensus.agree || m.consensus.clash) && (
        <div className="flex flex-col gap-1">{m.consensus.agree && <ConsensusRow kind="agree" text={m.consensus.agree} />}{m.consensus.clash && <ConsensusRow kind="clash" text={m.consensus.clash} />}</div>
      )}
      {panelPanes.length > 0 && (
        <div className="cz-panel-wrap">
          <div className="cz-panel-head">▾ panel · {panelPanes.length} agents</div>
          {panelPanes.map((key) => { const p = m.panes[key]; return (
            <Collapsible key={key} defaultOpen={false} trigger={
              <div className="cz-agent-sum"><Dot agent={p.agent} size={7} warm={p.warm} /><span className="font-semibold" style={{ color: "var(--color-text)" }}>{label(key, p.agent)}</span>{p.warm && <span className="cz-warm-tag">warm</span>}<div className="flex-1" />{p.done ? <><span className="text-text-tertiary tabular-nums">{fmtMs(p.ms)}</span><span style={{ color: p.code === 0 ? "var(--color-success)" : "var(--color-danger)" }}>{p.code === 0 ? "✓" : "✕"}</span></> : <Spinner size="xs" />}</div>
            }>
              <div className="cz-run-body" dangerouslySetInnerHTML={renderText(p.text || "…")} />
              {p.cmd && <div className="cz-run-cmd">$ {p.cmd}</div>}
            </Collapsible>
          ); })}
        </div>
      )}
    </div>
  );
}

function RunBlock({ p, title, fused = false, auto = false }: { p: Pane; title: string; fused?: boolean; auto?: boolean }) {
  const accent = fused ? "var(--color-primary)" : color(p.agent);
  return (
    <div className="cz-run-block" style={{ borderLeftColor: accent, ...(fused ? { background: "linear-gradient(180deg, rgba(240,163,94,.06), transparent 60%)" } : {}) }}>
      <div className="cz-run-head">
        <Dot agent={fused ? "fused" : p.agent} size={8} warm={p.warm} />
        <span className="font-semibold" style={{ color: "var(--color-text)" }}>{auto ? `auto → ${display(p.agent)}` : title}</span>
        {p.warm ? <span className="cz-warm-tag">warm · resumed</span> : (!fused && <span className="cz-cold-tag">new session</span>)}
        <div className="flex-1" />
        {p.done ? <><span className="text-text-tertiary tabular-nums">{fmtMs(p.ms)}</span><span style={{ color: p.code === 0 ? "var(--color-success)" : "var(--color-danger)" }}>{p.code === 0 ? "✓ exit 0" : `✕ exit ${p.code ?? 1}`}</span></> : <Spinner size="xs" />}
      </div>
      <div className={"cz-run-body" + (fused ? " cz-run-fused" : "")} dangerouslySetInnerHTML={renderText(p.text || (p.done ? "" : "…"))} />
      {p.cmd && <div className="cz-run-cmd">$ {p.cmd}</div>}
    </div>
  );
}

function ConsensusRow({ kind, text }: { kind: "agree" | "clash"; text: string }) {
  return <div className="flex gap-2.5 items-start text-[13px] px-2.5 py-1.5 rounded" style={{ background: "var(--color-surface)", border: "1px solid var(--color-surface-border)", fontFamily: "var(--font-mono)" }}><Badge color={kind === "agree" ? "success" : "warning"} variant="subtle" size="sm">{kind}</Badge><span style={{ color: "#d3d7df" }}>{text}</span></div>;
}

function Empty({ primary, fuse }: { primary: string; fuse: boolean }) {
  return (
    <div className="text-text-tertiary text-[12.5px] leading-relaxed pt-2">
      <div style={{ color: "var(--color-primary)" }}># parley — multi-agent dev console</div>
      <div className="mt-1">› primary: <span className="text-text-secondary">{display(primary)}</span>   fuse: <span className="text-text-secondary">{fuse ? "on" : "off"}</span>   (sessions resume warm — prompt cache reused)</div>
      <div>› <span className="text-text-secondary">📂 open</span> a folder · pick a primary in the rail · toggle <span className="text-text-secondary">fuse</span> &amp; build a panel (add the same agent twice at different models)</div>
      <div>› <span className="text-text-secondary">@agent</span> to direct · <span className="text-text-secondary">/</span> commands · paste images · <Kbd>⌘</Kbd><Kbd>K</Kbd> palette · <Kbd>↑</Kbd> history</div>
    </div>
  );
}

function Cockpit({ diff, onRefresh, onClose }: { diff: GitDiff | null; onRefresh: () => void; onClose: () => void }) {
  return (
    <aside className="flex flex-col border-l border-surface-border" style={{ width: 400, flexShrink: 0, background: "var(--color-surface)" }}>
      <div className="flex items-center justify-between px-3 h-9 border-b border-surface-border text-[11px] font-mono uppercase tracking-wider text-text-muted"><span>working tree {diff?.isRepo ? `· ⎇ ${diff.branch}` : ""}</span><div className="flex gap-1.5"><button className="cz-mini" onClick={onRefresh}>refresh</button><button className="cz-mini" onClick={onClose}>close</button></div></div>
      <div className="px-3 py-2 border-b border-surface-border font-mono text-[11.5px]" style={{ maxHeight: 150, overflowY: "auto" }}>
        {!diff || !diff.isRepo ? <span className="italic text-text-tertiary">not a git repo</span> : diff.files.length === 0 ? <span className="italic text-text-tertiary">working tree clean</span> : diff.files.map((f, i) => <div key={i} className="whitespace-pre" style={{ color: "#d3d7df" }}>{f}</div>)}
      </div>
      <ScrollArea><pre className="m-0 px-3 py-2.5 font-mono whitespace-pre" style={{ fontSize: 11.5, lineHeight: 1.5, color: "#aeb6c4" }}>{diff?.diff || ""}</pre></ScrollArea>
    </aside>
  );
}
