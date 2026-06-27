// Parley desktop — unified multi-agent chat. Uses the global Tauri API
// (withGlobalTauri), so no npm/bundler.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const CHAT_ID = "main";

const state = {
  target: "auto",
  agents: [], // [{name, installed}]
  metas: ["auto", "fuse", "solve"],
  busy: false,
  active: new Map(), // msgId -> assistant render record
};

const el = {
  thread: document.getElementById("thread"),
  empty: document.getElementById("empty-state"),
  seg: document.getElementById("target-seg"),
  model: document.getElementById("model"),
  input: document.getElementById("input"),
  send: document.getElementById("send"),
  meta: document.getElementById("composer-meta"),
  settings: document.getElementById("settings"),
  settingsBtn: document.getElementById("settings-btn"),
  cockpit: document.getElementById("cockpit"),
  cockpitBtn: document.getElementById("cockpit-btn"),
  cwd: document.getElementById("cwd"),
  yolo: document.getElementById("yolo"),
  panel: document.getElementById("panel"),
  usage: document.getElementById("usage"),
  suggestions: document.getElementById("suggestions"),
  diffFiles: document.getElementById("diff-files"),
  diffBody: document.getElementById("diff-body"),
  diffRefresh: document.getElementById("diff-refresh"),
  diffDiscard: document.getElementById("diff-discard"),
};

const META_LABELS = { auto: "Auto", fuse: "Fuse", solve: "Solve" };
const SUGGESTIONS = [
  "Design a rate limiter for a multi-tenant API",
  "Explain what this repo does",
  "Find the security holes in this approach",
];

// ---- boot -----------------------------------------------------------------

async function boot() {
  renderSuggestions();
  wireComposer();
  wireChrome();
  try {
    const list = await invoke("list_agents");
    state.agents = list.agents || [];
    state.metas = list.meta || state.metas;
    if (list.defaultPanel?.length) el.panel.placeholder = list.defaultPanel.join(", ");
    buildTargetSeg();
  } catch (e) {
    buildTargetSeg();
    flashMeta("Could not reach `par` — is it on your PATH? " + e);
  }
  await listen("chat-event", (ev) => onChatEvent(ev.payload));
}

function wireChrome() {
  el.settingsBtn.addEventListener("click", () => {
    el.settings.classList.toggle("hidden");
    if (!el.settings.classList.contains("hidden")) refreshUsage();
  });
  el.cockpitBtn.addEventListener("click", () => {
    el.cockpit.classList.toggle("hidden");
    if (!el.cockpit.classList.contains("hidden")) refreshDiff();
  });
  el.diffRefresh.addEventListener("click", refreshDiff);
  el.diffDiscard.addEventListener("click", discardDiff);
}

function buildTargetSeg() {
  el.seg.innerHTML = "";
  for (const m of state.metas) {
    el.seg.appendChild(segButton(m, META_LABELS[m] || m, true, true));
  }
  for (const a of state.agents) {
    el.seg.appendChild(segButton(a.name, a.name, a.installed, false));
  }
  selectTarget(state.target);
}

function segButton(value, label, enabled, isMeta) {
  const b = document.createElement("button");
  b.role = "tab";
  b.dataset.value = value;
  b.className = isMeta ? "meta" : "";
  if (!isMeta && enabled) b.classList.add("installed");
  b.innerHTML = `${escapeHtml(label)}<span class="dot"></span>`;
  b.disabled = !enabled && !isMeta;
  b.title = enabled ? value : `${value} (CLI not installed)`;
  b.addEventListener("click", () => selectTarget(value));
  return b;
}

function selectTarget(value) {
  state.target = value;
  for (const b of el.seg.querySelectorAll("button")) {
    b.setAttribute("aria-selected", b.dataset.value === value ? "true" : "false");
  }
}

function renderSuggestions() {
  for (const s of SUGGESTIONS) {
    const b = document.createElement("button");
    b.textContent = s;
    b.addEventListener("click", () => {
      el.input.value = s;
      autoGrow();
      updateSendState();
      el.input.focus();
    });
    el.suggestions.appendChild(b);
  }
}

// ---- composer -------------------------------------------------------------

function wireComposer() {
  el.input.addEventListener("input", () => {
    autoGrow();
    updateSendState();
    hintMention();
  });
  el.input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  });
  el.send.addEventListener("click", send);
}

function autoGrow() {
  el.input.style.height = "auto";
  el.input.style.height = Math.min(el.input.scrollHeight, 200) + "px";
}

function updateSendState() {
  el.send.disabled = state.busy || el.input.value.trim() === "";
}

// Parse a leading @mention to override the target for this one message.
// @panel / @fuse → fuse; @auto, @solve, or any known agent name/code.
function parseMention(text) {
  const m = text.match(/^@([a-zA-Z0-9_-]+)\s+([\s\S]+)$/);
  if (!m) return null;
  const tag = m[1].toLowerCase();
  const rest = m[2];
  if (tag === "panel" || tag === "fuse") return { target: "fuse", prompt: rest };
  if (tag === "auto" || tag === "solve") return { target: tag, prompt: rest };
  const known = state.agents.find((a) => a.name === tag) || isCode(tag);
  if (known) return { target: tag, prompt: rest };
  return null;
}

const CODE_MAP = {
  cl: "claude", co: "codex", g: "gemini", cu: "cursor", oc: "opencode",
  q: "qwen", k: "kimi", go: "goose", cp: "copilot", aq: "amazon-q", ag: "antigravity",
};
function isCode(tag) {
  return CODE_MAP[tag] ? { name: CODE_MAP[tag] } : null;
}

function hintMention() {
  const parsed = parseMention(el.input.value.trim());
  flashMeta(parsed ? `→ this message goes to ${labelFor(parsed.target)}` : "");
}

function labelFor(t) {
  return META_LABELS[t] || (CODE_MAP[t] || t);
}

function flashMeta(text) {
  el.meta.textContent = text;
}

// ---- send -----------------------------------------------------------------

let seq = 0;

async function send() {
  const raw = el.input.value.trim();
  if (!raw || state.busy) return;

  const mention = parseMention(raw);
  const target = mention ? mention.target : state.target;
  const prompt = mention ? mention.prompt : raw;

  el.empty?.remove();
  el.input.value = "";
  autoGrow();
  flashMeta("");

  addUserMessage(raw);

  const msgId = `m${Date.now()}-${seq++}`;
  const isFuse = target === "fuse";
  const assistant = addAssistantMessage(msgId, target, isFuse);
  state.active.set(msgId, assistant);
  setBusy(true);

  const req = {
    chatId: CHAT_ID,
    msgId,
    target,
    model: el.model.value.trim() || null,
    prompt,
    cwd: el.cwd.value.trim() || null,
    panel: isFuse ? parsePanel(el.panel.value) : [],
    judge: null,
    yolo: el.yolo.checked,
  };

  try {
    await invoke("send_message", { req });
  } catch (e) {
    appendError(assistant, "main", String(e));
  } finally {
    finishMessage(msgId);
    if (!el.cockpit.classList.contains("hidden")) refreshDiff();
  }
}

function parsePanel(raw) {
  return raw.split(",").map((s) => s.trim()).filter(Boolean);
}

function setBusy(b) {
  state.busy = b;
  el.send.classList.toggle("busy", b);
  el.send.querySelector(".send-glyph").textContent = b ? "■" : "↑";
  updateSendState();
}

// ---- message rendering ----------------------------------------------------

function addUserMessage(text) {
  const msg = document.createElement("div");
  msg.className = "msg user";
  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.textContent = text;
  msg.appendChild(bubble);
  el.thread.appendChild(msg);
  scrollDown();
}

function addAssistantMessage(msgId, target, isFuse) {
  const msg = document.createElement("div");
  msg.className = "msg assistant";
  msg.dataset.msgId = msgId;

  const who = document.createElement("div");
  who.className = "who";
  who.innerHTML = `<span class="badge">⚖ ${escapeHtml(META_LABELS[target] || target)}</span>`;
  msg.appendChild(who);

  let panesEl = null;
  if (isFuse) {
    panesEl = document.createElement("div");
    panesEl.className = "panes";
    msg.appendChild(panesEl);
  }

  const consensus = document.createElement("div");
  consensus.className = "consensus hidden";
  msg.appendChild(consensus);

  const answer = document.createElement("div");
  answer.className = isFuse ? "answer fused" : "answer";
  answer.style.display = "none";
  msg.appendChild(answer);

  const status = document.createElement("div");
  status.className = "status-line";
  status.innerHTML = `<span class="spinner"></span><span class="status-text">working…</span>`;
  msg.appendChild(status);

  el.thread.appendChild(msg);
  scrollDown();

  return {
    el: msg, whoEl: who, panesEl, consensusEl: consensus, answerEl: answer,
    statusEl: status, isFuse, target, panes: new Map(), finalText: "",
  };
}

function getPane(a, pane, agent, warm) {
  if (a.panes.has(pane)) return a.panes.get(pane);

  if (pane === "main" || pane === "fused") {
    a.answerEl.style.display = "block";
    const badge = a.whoEl.querySelector(".badge");
    if (pane === "fused") {
      badge.innerHTML = `⚖ Fused${agent ? " · judge " + escapeHtml(agent) : ""}`;
    } else if (agent) {
      const warmTag = warm ? ` <span class="warm">warm</span>` : "";
      const lead = a.target === "auto" ? "Auto → " : "";
      badge.innerHTML = `⚖ ${lead}${escapeHtml(agent)}${warmTag}`;
    }
    const rec = { bodyEl: a.answerEl, acc: "", isFused: pane === "fused" };
    a.panes.set(pane, rec);
    return rec;
  }

  // fuse panelist → its own collapsible pane
  const details = document.createElement("details");
  details.className = "pane";
  details.open = true;
  const warmTag = warm ? `<span class="warm">warm</span>` : "";
  details.innerHTML =
    `<summary><span class="pane-name">${escapeHtml(agent || pane)}</span>${warmTag}` +
    `<span class="pane-spin spinner"></span></summary>`;
  const body = document.createElement("div");
  body.className = "pane-body";
  details.appendChild(body);
  a.panesEl.appendChild(details);
  const rec = { bodyEl: body, summaryEl: details.querySelector("summary"), acc: "", isFused: false };
  a.panes.set(pane, rec);
  return rec;
}

function onChatEvent(p) {
  const a = state.active.get(p.msgId);
  if (!a) return;
  switch (p.kind) {
    case "start":
      getPane(a, p.pane, p.agent, p.warm);
      break;
    case "chunk": {
      const rec = getPane(a, p.pane, p.agent, p.warm);
      rec.acc += p.text || "";
      rec.bodyEl.innerHTML = renderText(rec.acc);
      scrollDownIfNear();
      break;
    }
    case "status":
      setStatus(a, p.text || "");
      break;
    case "done": {
      const rec = a.panes.get(p.pane);
      if (rec?.summaryEl) {
        const spin = rec.summaryEl.querySelector(".pane-spin");
        if (spin)
          spin.outerHTML =
            (p.code === 0 ? `<span class="tick">✓</span>` : `<span class="cross">✕</span>`) +
            (p.ms ? `<span class="ms">${fmtMs(p.ms)}</span>` : "");
      }
      if (rec && (p.pane === "main" || p.pane === "fused")) {
        a.finalText = rec.acc;
        if (p.pane === "fused") renderConsensus(a, rec.acc);
        if (p.ms) appendTiming(a, p.ms);
      }
      break;
    }
    case "error":
      appendError(a, p.pane, p.text || "error");
      break;
  }
}

function setStatus(a, text) {
  const t = a.statusEl.querySelector(".status-text");
  if (t) t.textContent = text;
}

function appendTiming(a, ms) {
  const t = a.statusEl.querySelector(".status-text");
  if (t) t.textContent = `done in ${fmtMs(ms)}`;
}

function appendError(a, pane, text) {
  const rec = getPane(a, pane, pane, false);
  rec.acc += (rec.acc ? "\n" : "") + "⚠ " + text;
  rec.bodyEl.innerHTML = renderText(rec.acc);
  rec.bodyEl.style.display = "block";
}

function finishMessage(msgId) {
  const a = state.active.get(msgId);
  if (a) {
    a.statusEl.remove();
    state.active.delete(msgId);
  }
  setBusy(false);
  el.input.focus();
}

// Pull the judge's CONSENSUS / CONTRADICTIONS headings into a compact strip.
function renderConsensus(a, text) {
  const consensus = section(text, "CONSENSUS");
  const contra = section(text, "CONTRADICTIONS");
  if (!consensus && !contra) return;
  let html = "";
  if (consensus) html += `<div class="c-row c-agree"><span>agree</span><div>${escapeHtml(trimList(consensus))}</div></div>`;
  if (contra) html += `<div class="c-row c-clash"><span>clash</span><div>${escapeHtml(trimList(contra))}</div></div>`;
  a.consensusEl.innerHTML = html;
  a.consensusEl.classList.remove("hidden");
}

function section(text, heading) {
  const re = new RegExp(`${heading}[^\\n]*\\n([\\s\\S]*?)(?:\\n[A-Z][A-Z ]{3,}|$)`);
  const m = text.match(re);
  return m ? m[1].trim() : "";
}
function trimList(s) {
  return s.split("\n").map((l) => l.trim()).filter(Boolean).slice(0, 4).join(" · ");
}

// ---- cockpit (git diff) ---------------------------------------------------

async function refreshDiff() {
  try {
    const d = await invoke("git_diff", { cwd: el.cwd.value.trim() || null });
    if (!d.isRepo) {
      el.diffFiles.innerHTML = `<div class="diff-empty">not a git repo</div>`;
      el.diffBody.textContent = "";
      return;
    }
    el.diffFiles.innerHTML =
      d.files.length === 0
        ? `<div class="diff-empty">working tree clean</div>`
        : d.files.map((f) => `<div class="diff-file">${escapeHtml(f)}</div>`).join("");
    el.diffBody.textContent = d.diff || "";
  } catch (e) {
    el.diffFiles.innerHTML = `<div class="diff-empty">${escapeHtml(String(e))}</div>`;
  }
}

async function discardDiff() {
  if (!confirm("Discard all uncommitted changes in the working tree? This runs `git checkout -- .`"))
    return;
  try {
    await invoke("git_discard", { cwd: el.cwd.value.trim() || null });
    refreshDiff();
  } catch (e) {
    alert("discard failed: " + e);
  }
}

// ---- usage ----------------------------------------------------------------

async function refreshUsage() {
  try {
    const rows = await invoke("usage_stats");
    if (!rows.length) {
      el.usage.innerHTML = `<div class="hint">No agent calls yet this session.</div>`;
      return;
    }
    el.usage.innerHTML =
      `<div class="usage-head">This session</div>` +
      rows
        .map(
          (r) =>
            `<div class="usage-row"><span class="u-agent">${escapeHtml(r.agent)}${
              r.warm ? ' <span class="warm">warm</span>' : ""
            }</span><span class="u-calls">${r.calls} call${r.calls === 1 ? "" : "s"}</span><span class="u-ms">${fmtMs(
              r.totalMs
            )}</span></div>`
        )
        .join("");
  } catch (e) {
    el.usage.innerHTML = `<div class="hint">${escapeHtml(String(e))}</div>`;
  }
}

// ---- text rendering -------------------------------------------------------

function escapeHtml(s) {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function renderText(raw) {
  const parts = raw.split(/```/);
  let html = "";
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) {
      const body = parts[i].replace(/^[a-zA-Z0-9_+-]*\n/, "");
      html += `<pre>${escapeHtml(body.replace(/\n$/, ""))}</pre>`;
    } else {
      html += escapeHtml(parts[i]).replace(/`([^`]+)`/g, '<code class="inline">$1</code>');
    }
  }
  return html;
}

function fmtMs(ms) {
  return ms >= 1000 ? (ms / 1000).toFixed(1) + "s" : Math.round(ms) + "ms";
}

function scrollDown() {
  el.thread.scrollTop = el.thread.scrollHeight;
}
function scrollDownIfNear() {
  if (el.thread.scrollHeight - el.thread.scrollTop - el.thread.clientHeight < 160) scrollDown();
}

boot();
