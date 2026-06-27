import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Collapsible, CommandPalette, Input, Kbd, Popover, ScrollArea, Spinner, Switch } from "./cruz";
import {
  invoke, IS_TAURI, listDir, listFiles, listSlashCommands, gitHeadFile, readFile, onChatEvent, pickFolder, savePaste,
  type AgentInfo, type AgentList, type AgentUsage, type ChatEvent, type DirListing, type GitDiff, type Panelist, type SendReq,
} from "./bridge";
import { DEFAULT_PANEL, NEEDS_PROVIDER, PRESETS, color, display } from "./agents";
import { monaco, langForPath } from "./monaco";

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
function Typing() {
  return <span className="cz-typing"><i /><i /><i /></span>;
}
function CopyBtn({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button className="cz-copy" title="Copy" onClick={(e) => { e.stopPropagation(); navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1100); }}>
      {done ? "✓ copied" : "copy"}
    </button>
  );
}

// ---- tab shell: many consoles at once --------------------------------------

interface Tab { id: string; title: string; }

export function App() {
  const [tabs, setTabs] = useState<Tab[]>([{ id: "c1", title: "console 1" }]);
  const [active, setActive] = useState("c1");
  const [busyMap, setBusyMap] = useState<Record<string, boolean>>({});
  const next = useRef(2);
  const openers = useRef<Record<string, () => void>>({});

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
  const setTitle = (id: string, title: string) => setTabs((t) => t.map((x) => (x.id === id ? { ...x, title } : x)));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const k = e.key.toLowerCase();
      if (k === "t") { e.preventDefault(); addTab(); }
      else if (k === "w") { e.preventDefault(); closeTab(active); }
      else if (k >= "1" && k <= "9") { const i = +k - 1; if (tabs[i]) { e.preventDefault(); setActive(tabs[i].id); } }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="flex flex-col h-screen">
      <div className="cz-tabstrip" data-tauri-drag-region>
        <div className="cz-tabstrip-pad" />
        <span className="cz-brand"><span style={{ color: "var(--color-primary)" }}>⚖</span> parley</span>
        <button className="cz-pill" onClick={() => openers.current[active]?.()} title="Open folder (active console)">📂 open</button>
        <div className="cz-tab-sep" />
        {tabs.map((t) => (
          <div key={t.id} className={"cz-tab" + (t.id === active ? " cz-tab-active" : "")} onClick={() => setActive(t.id)}>
            {busyMap[t.id] ? <span className="cz-tab-dot cz-dot-warm" style={{ background: "var(--color-warning)" }} /> : <span className="cz-tab-dot" style={{ background: "var(--color-text-tertiary)" }} />}
            <span>{t.title}</span>
            {tabs.length > 1 && <span className="cz-tab-x" onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}>×</span>}
          </div>
        ))}
        <button className="cz-tab-add" onClick={addTab} title="New console (⌘T)">+</button>
      </div>
      <div className="flex-1 min-h-0 relative">
        {tabs.map((t) => (
          <div key={t.id} style={{ display: t.id === active ? "block" : "none", height: "100%" }}>
            <Console chatId={t.id} active={t.id === active} onBusy={(b) => setBusyMap((m) => ({ ...m, [t.id]: b }))} onTitle={(title) => setTitle(t.id, title)} registerOpen={(fn) => { openers.current[t.id] = fn; }} />
          </div>
        ))}
      </div>
    </div>
  );
}

function Console({ chatId, active, onBusy, onTitle, registerOpen }: { chatId: string; active: boolean; onBusy: (busy: boolean) => void; onTitle: (t: string) => void; registerOpen: (fn: () => void) => void }) {
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
  const [slashCmds, setSlashCmds] = useState<string[]>([]);
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
  // In the real app the OS-native folder dialog is the most reliable real-FS
  // picker, so use it directly. In a plain browser there is no native dialog and
  // no filesystem access, so fall back to the in-app explorer (sample data).
  const doOpen = useCallback(() => {
    if (IS_TAURI) { pickFolder().then((f) => { if (f) { setCwd(f); setTimeout(refreshDiff, 0); } }); }
    else setFolderOpen(true);
  }, [refreshDiff]);

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

  // harness slash commands for the active primary (for `/` autocomplete)
  useEffect(() => {
    const h = primary === "auto" ? "claude" : primary;
    listSlashCommands(cwd || null, h).then(setSlashCmds).catch(() => setSlashCmds([]));
  }, [primary, cwd]);

  // tab title from the working dir basename
  useEffect(() => {
    const base = cwd ? cwd.replace(/\/+$/, "").split("/").pop() : "";
    onTitle(base || "console");
  }, [cwd, onTitle]);

  // expose folder-open to the App tab row
  useEffect(() => { registerOpen(doOpen); }, [registerOpen, doOpen]);

  // console-level keyboard (only the active console responds)
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const k = e.key.toLowerCase();
      if (k === "l") { e.preventDefault(); inputRef.current?.focus(); }
      else if (k === "b") { e.preventDefault(); setRailOpen((v) => !v); }
      else if (k === "j") { e.preventDefault(); setCockpit((v) => { if (!v) refreshDiff(); return !v; }); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, refreshDiff]);

  // panel ops
  const addPanelist = (agent: string) => setPanel((p) => [...p, { id: "p" + ++pid.current, agent, model: "", provider: "" }]);
  const removePanelist = (id: string) => setPanel((p) => p.filter((x) => x.id !== id));
  const patchPanelist = (id: string, patch: Partial<Panelist>) => setPanel((p) => p.map((x) => (x.id === id ? { ...x, ...patch } : x)));

  const commands: Command[] = useMemo(() => [
    { key: "fuse", label: (fuse ? "Disable" : "Enable") + " Fuse", run: () => setFuse((v) => !v) },
    { key: "single", label: "Single agent (Fuse off)", run: () => setFuse(false) },
    { key: "clear", label: "Clear run log", run: () => setMessages([]) },
    { key: "cockpit", label: "Toggle code cockpit", run: () => { setCockpit((v) => !v); refreshDiff(); } },
    { key: "open", label: "Open folder…", run: doOpen },
    { key: "auto", label: "Primary → Auto (route each message)", run: () => setPrimary("auto") },
    ...agents.filter((a) => a.installed).map((a) => ({ key: a.name, label: `Primary → ${display(a.name)}`, run: () => setPrimary(a.name) })),
  ], [agents, fuse, refreshDiff, doOpen]);

  async function run() {
    const raw = input.trim();
    if ((!raw && attachments.length === 0) || busy) return;
    // Routing is via the primary picker + Fuse toggle. `/` and `@` in the text
    // are the harness's own command + file-ref syntax, passed through verbatim.
    let prompt = raw;
    const fuseOn = fuse;
    const target = fuseOn ? "fuse" : primary;
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
      <header className="flex items-center gap-2.5 px-3 h-10 border-b border-surface-border font-mono text-[12.5px]" style={{ background: "rgba(10,11,13,.8)", backdropFilter: "blur(16px)" }}>
        <span className="flex items-center gap-1.5"><Dot agent={primary} size={8} /><span className="text-text-secondary">{cwd ? cwd.replace(/\/+$/, "").split("/").pop() : "no folder"}</span></span>
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
              <div className="flex gap-1.5"><Input size="sm" value={cwd} placeholder="$HOME" onChange={(e: React.ChangeEvent<HTMLInputElement>) => setCwd(e.target.value)} /><button className="cz-mini" onClick={doOpen}>browse</button></div>
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
          <Composer input={input} setInput={setInput} run={run} busy={busy} fuse={fuse} primary={primary} attachments={attachments} setAttachments={setAttachments}
            slashCmds={slashCmds} fileSearch={(q) => listFiles(cwd || null, q)} history={history} histIdx={histIdx} inputRef={inputRef} />
        </main>

        {cockpit && <Cockpit diff={diff} cwd={cwd} onRefresh={refreshDiff} onClose={() => setCockpit(false)} />}
      </div>

      {/* status bar */}
      <footer className="flex items-center gap-3 h-7 px-3 border-t border-surface-border font-mono text-[11px] text-text-muted" style={{ background: "rgba(255,255,255,.02)" }}>
        <button className="hover:text-text" style={{ color: "var(--color-text-secondary)" }} onClick={doOpen} title="Open folder">{cwd || "~"}</button>
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
        {!IS_TAURI && <div className="cz-folder-note font-mono text-[11px]">preview mode — sample folders. run the desktop app (<span style={{ color: "var(--color-primary)" }}>cargo tauri dev</span>) to browse your real files.</div>}
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
  slashCmds: string[]; fileSearch: (q: string) => Promise<string[]>;
  history: string[]; histIdx: React.MutableRefObject<number>; inputRef: React.RefObject<HTMLTextAreaElement>;
}) {
  const { input, setInput, run, busy, fuse, primary, attachments, setAttachments, slashCmds, fileSearch, history, histIdx, inputRef } = props;
  const [sel, setSel] = useState(0);
  const [files, setFiles] = useState<string[]>([]);

  // `/` = harness command (only while typing the command name, no space yet).
  const slashMode = input.startsWith("/") && !input.slice(1).includes(" ");
  const slashQ = slashMode ? input.slice(1).toLowerCase() : "";
  const slashMatches = slashMode ? slashCmds.filter((c) => c.slice(1).toLowerCase().startsWith(slashQ)) : [];
  // `@` = file reference: token immediately before the end of the input.
  const fileMatch = !slashMode ? input.match(/@([^\s@]*)$/) : null;
  const fileMode = !!fileMatch;
  const fileQ = fileMatch ? fileMatch[1] : "";

  useEffect(() => { setSel(0); }, [input]);
  useEffect(() => {
    if (!fileMode) { setFiles([]); return; }
    let live = true;
    fileSearch(fileQ).then((f) => { if (live) setFiles(f); }).catch(() => {});
    return () => { live = false; };
  }, [fileMode, fileQ, fileSearch]);

  const menuItems = slashMode ? slashMatches.slice(0, 8) : fileMode ? files.slice(0, 8) : [];
  const menuKind: "slash" | "file" | null = slashMode && slashMatches.length ? "slash" : fileMode && files.length ? "file" : null;

  function grow(t: HTMLTextAreaElement) { t.style.height = "auto"; t.style.height = Math.min(t.scrollHeight, 180) + "px"; }
  function pick(item: string) {
    if (menuKind === "slash") setInput(item + " ");
    else if (menuKind === "file" && fileMatch) setInput(input.slice(0, fileMatch.index) + "@" + item + " ");
    inputRef.current?.focus();
  }
  async function onPaste(e: React.ClipboardEvent<HTMLTextAreaElement>) {
    const imgs = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith("image/"));
    if (!imgs.length) return;
    e.preventDefault();
    for (const it of imgs) { const file = it.getAsFile(); if (!file) continue; const buf = new Uint8Array(await file.arrayBuffer()); const name = file.name || `paste-${Date.now()}.png`; try { const path = await savePaste(name, buf); setAttachments((a) => [...a, { path, name }]); } catch { /* ignore */ } }
  }
  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (menuKind && menuItems.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => (s + 1) % menuItems.length); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => (s - 1 + menuItems.length) % menuItems.length); return; }
      if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); pick(menuItems[sel]); return; }
      if (e.key === "Escape") { e.preventDefault(); (e.target as HTMLTextAreaElement).blur(); (e.target as HTMLTextAreaElement).focus(); return; }
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); run(); return; }
    if (e.key === "ArrowUp" && history.length && (input === "" || histIdx.current >= 0)) { e.preventDefault(); histIdx.current = Math.min(histIdx.current + 1, history.length - 1); setInput(history[history.length - 1 - histIdx.current]); return; }
    if (e.key === "ArrowDown" && histIdx.current >= 0) { e.preventDefault(); histIdx.current -= 1; setInput(histIdx.current < 0 ? "" : history[history.length - 1 - histIdx.current]); return; }
  }
  const caret = busy ? "var(--color-warning)" : slashMode ? "var(--color-info)" : fileMode ? "var(--color-success)" : "var(--color-primary)";
  return (
    <div className="border-t border-surface-border px-4 py-3 relative" style={{ background: "rgba(10,11,13,.6)" }}>
      {menuKind && menuItems.length > 0 && (
        <div className="cz-slash">
          <div className="cz-slash-cap">{menuKind === "slash" ? `${display(primary)} commands` : "files"}</div>
          {menuItems.map((it, i) => (
            <button key={it} className={"cz-slash-item" + (i === sel ? " cz-slash-sel" : "")} onMouseEnter={() => setSel(i)} onClick={() => pick(it)}>
              {menuKind === "slash" ? <span className="text-text">{it}</span> : <><span style={{ color: "var(--color-success)" }}>@</span><span className="text-text">{it}</span></>}
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
        <span className="cz-caret" style={{ color: caret }}>{slashMode ? "/" : fileMode ? "@" : "❯"}</span>
        <textarea ref={inputRef} rows={1} value={input} placeholder={fuse ? "message the panel…   / command · @ file · paste image" : `message ${display(primary)}…   / command · @ file · paste image`} onChange={(e) => { setInput(e.target.value); grow(e.target); }} onKeyDown={onKeyDown} onPaste={onPaste} />
        <button className="cz-run" disabled={busy || (!input.trim() && attachments.length === 0)} onClick={run}>{busy ? <Spinner size="xs" /> : <><span>run</span><Kbd>⏎</Kbd></>}</button>
      </div>
      <div className="text-[11px] text-text-tertiary mt-1.5 h-3 font-mono">{menuKind === "slash" ? "harness command — ↑↓ select · ⏎ insert" : menuKind === "file" ? "file ref — ↑↓ select · ⏎ insert" : ""}</div>
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
  const fusedTitle = `fused${panelPanes.length ? ` · ${panelPanes.length} agents → 1` : ""}`;
  return (
    <div className="pl-4 flex flex-col gap-2">
      {fused ? <RunBlock p={fused} title={fusedTitle} fused />
        : <div className="cz-run-block cz-rise" style={{ borderLeftColor: "var(--color-primary)" }}><div className="cz-run-head"><Dot agent="fused" size={8} /><span className="font-semibold" style={{ color: "var(--color-text)" }}>fused</span><span className="text-text-tertiary">· {panelPanes.length} agents → 1</span><div className="flex-1" /><Typing /><span className="text-text-tertiary">synthesizing</span></div></div>}
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
    <div className="cz-run-block cz-run-hover cz-rise" style={{ borderLeftColor: accent, ...(fused ? { background: "linear-gradient(180deg, rgba(240,163,94,.06), transparent 60%)" } : {}) }}>
      <div className="cz-run-head">
        <Dot agent={fused ? "fused" : p.agent} size={8} warm={p.warm} />
        <span className="font-semibold" style={{ color: "var(--color-text)" }}>{auto ? `auto → ${display(p.agent)}` : title}</span>
        {p.warm ? <span className="cz-warm-tag">warm · resumed</span> : (!fused && <span className="cz-cold-tag">new session</span>)}
        <div className="flex-1" />
        {p.text && <CopyBtn text={p.text} />}
        {p.done ? <><span className="text-text-tertiary tabular-nums">{fmtMs(p.ms)}</span><span style={{ color: p.code === 0 ? "var(--color-success)" : "var(--color-danger)" }}>{p.code === 0 ? "✓ exit 0" : `✕ exit ${p.code ?? 1}`}</span></> : <Spinner size="xs" />}
      </div>
      <div className={"cz-run-body" + (fused ? " cz-run-fused" : "")}>
        {p.text ? <span dangerouslySetInnerHTML={renderText(p.text)} /> : (!p.done && <Typing />)}
        {!p.done && p.text && <span className="cz-cursor">▋</span>}
      </div>
      {p.cmd && <div className="cz-run-cmd"><span className="flex-1">$ {p.cmd}</span><CopyBtn text={p.cmd} /></div>}
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
      <div>› <span className="text-text-secondary">/</span> {display(primary)} command · <span className="text-text-secondary">@</span> file ref · paste images · <Kbd>⌘</Kbd><Kbd>K</Kbd> for parley actions · <Kbd>↑</Kbd> history</div>
    </div>
  );
}

function parsePorcelain(line: string): { status: string; path: string } {
  const m = line.match(/^(..)\s+(.*)$/);
  return m ? { status: m[1].trim() || "M", path: m[2] } : { status: "?", path: line.trim() };
}
function statusColor(s: string) {
  if (s.includes("?")) return "var(--color-success)";
  if (s.includes("M")) return "var(--color-warning)";
  if (s.includes("A")) return "var(--color-success)";
  if (s.includes("D")) return "var(--color-danger)";
  return "var(--color-text-muted)";
}

function DiffView({ cwd, path }: { cwd: string; path: string }) {
  const elRef = useRef<HTMLDivElement>(null);
  const edRef = useRef<ReturnType<typeof monaco.editor.createDiffEditor> | null>(null);
  useEffect(() => {
    if (!elRef.current) return;
    const ed = monaco.editor.createDiffEditor(elRef.current, {
      readOnly: true, theme: "parley-dark", automaticLayout: true, renderSideBySide: false,
      fontSize: 12, fontFamily: "JetBrains Mono, ui-monospace, monospace", minimap: { enabled: false },
      scrollBeyondLastLine: false, lineNumbersMinChars: 3, renderOverviewRuler: false,
    });
    edRef.current = ed;
    return () => { const m = ed.getModel(); ed.dispose(); m?.original?.dispose(); m?.modified?.dispose(); };
  }, []);
  useEffect(() => {
    let live = true;
    Promise.all([gitHeadFile(cwd || null, path).catch(() => ""), readFile(cwd || null, path).catch(() => "")]).then(([head, cur]) => {
      if (!live || !edRef.current) return;
      const lang = langForPath(path);
      const old = edRef.current.getModel();
      edRef.current.setModel({ original: monaco.editor.createModel(head, lang), modified: monaco.editor.createModel(cur, lang) });
      old?.original?.dispose();
      old?.modified?.dispose();
    });
    return () => { live = false; };
  }, [cwd, path]);
  return <div ref={elRef} style={{ height: "100%", width: "100%" }} />;
}

function Cockpit({ diff, cwd, onRefresh, onClose }: { diff: GitDiff | null; cwd: string; onRefresh: () => void; onClose: () => void }) {
  const files = useMemo(() => (diff?.files || []).map(parsePorcelain), [diff]);
  const [selected, setSelected] = useState<string | null>(null);
  useEffect(() => {
    if (files.length && (!selected || !files.find((f) => f.path === selected))) setSelected(files[0].path);
    if (!files.length) setSelected(null);
  }, [files, selected]);

  return (
    <aside className="flex flex-col border-l border-surface-border" style={{ width: 520, flexShrink: 0, background: "var(--color-surface)" }}>
      <div className="flex items-center justify-between px-3 h-9 border-b border-surface-border text-[11px] font-mono uppercase tracking-wider text-text-muted">
        <span>changes {diff?.isRepo ? `· ⎇ ${diff.branch}` : ""}</span>
        <div className="flex gap-1.5"><button className="cz-mini" onClick={onRefresh}>refresh</button><button className="cz-mini" onClick={onClose}>close ⌘J</button></div>
      </div>
      <div className="border-b border-surface-border" style={{ maxHeight: 150, overflowY: "auto" }}>
        {!diff || !diff.isRepo ? <div className="px-3 py-2 italic text-text-tertiary font-mono text-[11.5px]">not a git repo</div>
          : files.length === 0 ? <div className="px-3 py-2 italic text-text-tertiary font-mono text-[11.5px]">working tree clean</div>
          : files.map((f) => (
            <button key={f.path} className={"cz-diff-file" + (selected === f.path ? " cz-diff-file-sel" : "")} onClick={() => setSelected(f.path)}>
              <span className="font-bold" style={{ color: statusColor(f.status), width: 16, display: "inline-block" }}>{f.status}</span>
              <span className="truncate">{f.path}</span>
            </button>
          ))}
      </div>
      <div className="flex-1 min-h-0">
        {selected ? <DiffView cwd={cwd} path={selected} />
          : <div className="grid place-items-center h-full text-text-tertiary font-mono text-[12px]">select a changed file</div>}
      </div>
    </aside>
  );
}
