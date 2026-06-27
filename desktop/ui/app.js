// Parley desktop — chat logic. Uses the global Tauri API (withGlobalTauri), so
// no npm/bundler: window.__TAURI__.core.invoke + window.__TAURI__.event.listen.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---- state ----------------------------------------------------------------

const state = {
  target: "auto",
  history: [], // [{role, text}] of completed turns, for context
  busy: false,
  // msgId -> { el, panes: Map<pane,{bodyEl,statusEl,acc}>, isFuse }
  active: new Map(),
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
  cwd: document.getElementById("cwd"),
  yolo: document.getElementById("yolo"),
  panel: document.getElementById("panel"),
  suggestions: document.getElementById("suggestions"),
};

const META_LABELS = { auto: "Auto", fuse: "Fuse", solve: "Solve" };
const SUGGESTIONS = [
  "Design a rate limiter for a multi-tenant API",
  "Explain what this repo does",
  "Review this approach for security holes",
];

// ---- boot -----------------------------------------------------------------

async function boot() {
  renderSuggestions();
  wireComposer();
  el.settingsBtn.addEventListener("click", () => {
    el.settings.classList.toggle("hidden");
  });

  try {
    const list = await invoke("list_agents");
    buildTargetSeg(list);
    if (list.defaultPanel?.length) {
      el.panel.placeholder = list.defaultPanel.join(", ");
    }
  } catch (e) {
    buildTargetSeg({ meta: ["auto", "fuse", "solve"], agents: [] });
    flashMeta("Could not reach `par` — is it on your PATH? " + e);
  }

  await listen("chat-event", (event) => onChatEvent(event.payload));
}

function buildTargetSeg(list) {
  el.seg.innerHTML = "";
  for (const m of list.meta || []) {
    el.seg.appendChild(makeSegButton(m, META_LABELS[m] || m, true, true));
  }
  for (const a of list.agents || []) {
    el.seg.appendChild(makeSegButton(a.name, a.name, a.installed, false));
  }
  selectTarget(state.target);
}

function makeSegButton(value, label, enabled, isMeta) {
  const b = document.createElement("button");
  b.role = "tab";
  b.dataset.value = value;
  b.className = isMeta ? "meta" : "";
  if (!isMeta && enabled) b.classList.add("installed");
  b.innerHTML = `${label}<span class="dot"></span>`;
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
  el.panel.parentElement.style.opacity = value === "fuse" ? "1" : "0.5";
}

function renderSuggestions() {
  for (const s of SUGGESTIONS) {
    const b = document.createElement("button");
    b.textContent = s;
    b.addEventListener("click", () => {
      el.input.value = s;
      autoGrow();
      el.input.focus();
      updateSendState();
    });
    el.suggestions.appendChild(b);
  }
}

// ---- composer -------------------------------------------------------------

function wireComposer() {
  el.input.addEventListener("input", () => {
    autoGrow();
    updateSendState();
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

function flashMeta(text) {
  el.meta.textContent = text;
}

// ---- send -----------------------------------------------------------------

let seq = 0;

async function send() {
  const prompt = el.input.value.trim();
  if (!prompt || state.busy) return;

  el.empty?.remove();
  el.input.value = "";
  autoGrow();

  addUserMessage(prompt);

  const msgId = `m${Date.now()}-${seq++}`;
  const isFuse = state.target === "fuse";
  const assistant = addAssistantMessage(msgId, state.target, isFuse);
  state.active.set(msgId, assistant);

  setBusy(true);

  const req = {
    chatId: "main",
    msgId,
    target: state.target,
    model: el.model.value.trim() || null,
    prompt,
    history: state.history.slice(),
    cwd: el.cwd.value.trim() || null,
    panel: isFuse ? parsePanel(el.panel.value) : [],
    judge: null,
    yolo: el.yolo.checked,
  };

  // Record the user's turn now; the assistant turn is recorded on completion.
  state.history.push({ role: "you", text: prompt });

  try {
    await invoke("send_message", { req });
  } catch (e) {
    appendError(assistant, "main", String(e));
  } finally {
    finishMessage(msgId);
  }
}

function parsePanel(raw) {
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

function setBusy(b) {
  state.busy = b;
  el.send.classList.toggle("busy", b);
  el.send.querySelector(".send-glyph").textContent = b ? "■" : "↑";
  updateSendState();
  flashMeta(b ? "Running…" : "");
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
  const label = META_LABELS[target] || target;
  who.innerHTML = `<span class="badge">⚖ ${escapeHtml(label)}</span>`;
  msg.appendChild(who);

  let panesEl = null;
  if (isFuse) {
    panesEl = document.createElement("div");
    panesEl.className = "panes";
    msg.appendChild(panesEl);
  }

  // The primary answer body (single agent → here; fuse → the fused pane).
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
    el: msg,
    whoEl: who,
    panesEl,
    answerEl: answer,
    statusEl: status,
    isFuse,
    target,
    panes: new Map(), // pane -> { bodyEl, acc, isFused }
    finalText: "",
  };
}

function getPane(a, pane, agent) {
  if (a.panes.has(pane)) return a.panes.get(pane);

  // The single-agent stream and the fused result both write to the main answer.
  if (pane === "main" || pane === "fused") {
    a.answerEl.style.display = "block";
    if (pane === "fused") {
      a.whoEl.querySelector(".badge").innerHTML = `⚖ Fused${
        agent ? " · judge " + escapeHtml(agent) : ""
      }`;
    } else if (agent && a.target === "auto") {
      a.whoEl.querySelector(".badge").innerHTML = `⚖ Auto → ${escapeHtml(agent)}`;
    }
    const rec = { bodyEl: a.answerEl, acc: "", isFused: pane === "fused" };
    a.panes.set(pane, rec);
    return rec;
  }

  // A fuse panelist → its own collapsible pane.
  const details = document.createElement("details");
  details.className = "pane";
  details.open = true;
  const summary = document.createElement("summary");
  summary.innerHTML = `<span class="pane-name">${escapeHtml(
    agent || pane
  )}</span><span class="pane-spin spinner"></span>`;
  const body = document.createElement("div");
  body.className = "pane-body";
  details.appendChild(summary);
  details.appendChild(body);
  a.panesEl.appendChild(details);

  const rec = { bodyEl: body, summaryEl: summary, acc: "", isFused: false };
  a.panes.set(pane, rec);
  return rec;
}

function onChatEvent(p) {
  const a = state.active.get(p.msgId);
  if (!a) return;

  switch (p.kind) {
    case "start": {
      getPane(a, p.pane, p.agent);
      break;
    }
    case "chunk": {
      const rec = getPane(a, p.pane, p.agent);
      rec.acc += p.text || "";
      rec.bodyEl.innerHTML = renderText(rec.acc);
      scrollDownIfNear();
      break;
    }
    case "status": {
      setStatus(a, p.text || "");
      break;
    }
    case "done": {
      const rec = a.panes.get(p.pane);
      if (rec?.summaryEl) {
        const spin = rec.summaryEl.querySelector(".pane-spin");
        if (spin) {
          spin.outerHTML =
            p.code === 0 ? `<span class="tick">✓</span>` : `<span class="cross">✕</span>`;
        }
      }
      if (rec && (p.pane === "main" || p.pane === "fused")) {
        a.finalText = rec.acc;
      }
      break;
    }
    case "error": {
      appendError(a, p.pane, p.text || "error");
      break;
    }
  }
}

function setStatus(a, text) {
  const t = a.statusEl.querySelector(".status-text");
  if (t) t.textContent = text;
}

function appendError(a, pane, text) {
  const rec = getPane(a, pane, pane);
  rec.acc += (rec.acc ? "\n" : "") + "⚠ " + text;
  rec.bodyEl.innerHTML = renderText(rec.acc);
  rec.bodyEl.style.display = "block";
}

function finishMessage(msgId) {
  const a = state.active.get(msgId);
  if (a) {
    a.statusEl.remove();
    // Record the assistant turn for context continuity.
    const text = a.finalText || collectAnyText(a);
    if (text.trim()) {
      state.history.push({ role: a.target, text: text.trim() });
    }
    state.active.delete(msgId);
  }
  setBusy(false);
  el.input.focus();
}

function collectAnyText(a) {
  for (const rec of a.panes.values()) if (rec.isFused) return rec.acc;
  const main = a.panes.get("main");
  return main ? main.acc : "";
}

// ---- text rendering (light, dependency-free) ------------------------------

function escapeHtml(s) {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// Minimal, safe formatting: fenced code blocks + inline code. Everything else
// is escaped and rendered as preformatted-ish wrapped text.
function renderText(raw) {
  const parts = raw.split(/```/);
  let html = "";
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) {
      // code block; drop a leading language tag line
      const body = parts[i].replace(/^[^\n]*\n/, (m) =>
        /^[a-zA-Z0-9_+-]*\n$/.test(m) ? "" : m
      );
      html += `<pre>${escapeHtml(body.replace(/\n$/, ""))}</pre>`;
    } else {
      html += escapeHtml(parts[i]).replace(
        /`([^`]+)`/g,
        '<code class="inline">$1</code>'
      );
    }
  }
  return html;
}

// ---- scrolling ------------------------------------------------------------

function scrollDown() {
  el.thread.scrollTop = el.thread.scrollHeight;
}
function scrollDownIfNear() {
  const nearBottom =
    el.thread.scrollHeight - el.thread.scrollTop - el.thread.clientHeight < 140;
  if (nearBottom) scrollDown();
}

boot();
