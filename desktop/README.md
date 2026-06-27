<div align="center">

# Parley Desktop

**One chat. Every agent. Fused.**

A small, fast desktop chat app over the coding-agent CLIs already on your
machine — Claude, Codex, Gemini, and more. Pick **Auto** to route to the best
agent for a prompt, **Fuse** to ask a panel and synthesize one answer, **Solve**
to escalate when one gets stuck, or send straight to a single agent. One
conversation, switchable per message.

Built with **Tauri** (Rust + native webview) — a ~8 MB app, not a 150 MB one.

</div>

---

## Why it's just a thin shell

The app has no model logic of its own. It drives the [`par`](../README.md)
binary — Parley's CLI — which owns all the harness/routing/fusion logic. For any
message it asks `par … --dry-run` for the exact command to run, then spawns that
and streams the output live into the UI.

That means the desktop app inherits, for free, everything `par` already does:
**each agent's own auth, subscription, and prompt caching** — no API keys, your
code never leaves your machine. Fusion runs every panelist concurrently (each in
its own pane) and a judge synthesizes the result, exactly like `par fuse`, but
streamed instead of all-at-once.

```
┌─ ⚖ Parley ────────────── Auto · Fuse · Solve · claude · codex … ─┐
│                                                                  │
│                          design a rate limiter            (you) │
│                                                                  │
│  ⚖ Fused · judge claude                                          │
│  ┌ claude ✓ ┐ ┌ codex ✓ ┐ ┌ gemini ✓ ┐                          │
│  │ token..  │ │ leaky..  │ │ sliding… │   ← live panes           │
│  └──────────┘ └──────────┘ └──────────┘                          │
│  CONSENSUS: a token bucket per tenant…                          │
│                                                                  │
│  > │ Message the panel…                                    [ ↑ ] │
└──────────────────────────────────────────────────────────────────┘
```

## Prerequisites

1. **`par` on your PATH.** Install Parley (see the [root README](../README.md));
   the app finds `par` on `PATH`, or set `PARLEY_BIN=/abs/path/to/par`.
2. **At least one agent CLI installed** (`par install <agent>`), authenticated.
3. **Rust** + the Tauri system deps for your OS
   (Linux: `webkit2gtk-4.1`, `libsoup-3.0`; see
   <https://tauri.app/start/prerequisites/>).

## Run it

From the repo root:

```sh
# one-time: the Tauri CLI (only needed for hot-reload dev + bundling)
cargo install tauri-cli --version '^2'

# dev (hot-reload)
cargo tauri dev --config desktop/src-tauri/tauri.conf.json
```

Or with no extra tooling — the frontend is static and embedded at build, so a
plain cargo run works:

```sh
cargo run -p parley-desktop
```

## Build a distributable app

```sh
cargo tauri build --config desktop/src-tauri/tauri.conf.json
# → desktop/src-tauri/target/release/bundle/ (.app / .dmg / .deb / .AppImage / .msi)
```

## How it's wired

```text
desktop/
  ui/                     static frontend (no npm, no bundler)
    index.html            chat layout
    styles.css            dark, responsive theme
    app.js                Tauri IPC + streaming render (global __TAURI__ API)
  src-tauri/
    src/main.rs           backend: list_agents + send_message commands
                          · resolves argv via `par --dry-run`
                          · spawns + streams stdout/stderr as `chat-event`s
                          · orchestrates fuse panes + judge concurrently
    tauri.conf.json       window + withGlobalTauri (no JS toolchain needed)
    capabilities/         window permissions (core events)
```

The frontend uses the global `window.__TAURI__` API (`withGlobalTauri`), so there
is **no Node/npm build step** — `ui/` is plain HTML/CSS/JS, embedded into the
binary at compile time.

## Settings (gear icon)

- **Working directory** — where the agents operate (defaults to `$HOME`). Point
  it at a repo to chat *about that codebase*.
- **Yolo** — let agents act without per-action prompts (on by default, so
  headless calls don't hang). Turn off for untrusted prompts.
- **Fuse panel** — comma-separated agents for Fuse mode (blank = `claude, codex,
  gemini`).

## Notes & limits (v1)

- Conversation context is replayed as a preamble each turn (capped). True
  per-agent session pinning (warm prompt cache across turns) is future work —
  it maps onto `par`'s cross-agent session resume.
- Output streams at line granularity from each CLI; token-level streaming
  (`--output-format stream-json`) is a future enhancement.
- The app is a separate workspace crate with its own dependencies; the core
  `par` CLI stays dependency-free.
