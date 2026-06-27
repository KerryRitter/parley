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

## What makes it more than a chat box

One conversation, every agent underneath — and three things a one-shot CLI call
can't give you:

- **Warm session pinning.** Each agent keeps a *resumed* session per chat
  (`claude --session-id`/`--resume`, `codex exec resume`, `gemini --resume`), so
  follow-ups reuse that agent's own warm prompt cache instead of re-sending the
  whole transcript cold. Turns marked **warm** in the UI did exactly this.
- **Shared cross-agent memory.** There's one canonical transcript. When a message
  goes to an agent, it resumes its own warm thread and is fed only the *delta* —
  what the *other* agents said since it last spoke — so every agent stays in the
  same conversation without paying to replay all of it. Type `@gemini …`,
  `@panel …`, or `@auto …` to direct a single message; they all share the thread.
- **Live fan-out + fusion.** Fuse streams every panelist into its own pane
  concurrently, then a judge synthesizes — the `par fuse` engine, but live. The
  judge's CONSENSUS / CONTRADICTIONS are surfaced as a compact strip.

Plus a **code cockpit** (⌥): point at a repo and watch the agents' `git diff`
live, with a guarded discard. And a **usage panel** (⚙): per-agent calls, time,
and which sessions are warm.

## Why it's just a thin shell

The app has no model logic of its own. It drives the [`par`](../README.md)
binary — Parley's CLI — which owns all the harness/routing/fusion/session logic.
For any message it asks `par … --dry-run` (with `--session-id`/`--resume-id` for
warm continuity) for the exact command to run, then spawns that and streams the
output live into the UI.

That means the desktop app inherits, for free, everything `par` already does:
**each agent's own auth, subscription, and prompt caching** — no API keys, your
code never leaves your machine.

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

## Notes & limits

- **Warm pinning** is exact for **claude** (we own the session id via
  `--session-id`, then `--resume <id>` — correct even with several chats open).
  **codex**/**gemini** resume the *most recent* session here (`--last` /
  `latest`), which is correct for a single active chat; other agents fall back to
  a cold context preamble. The UI badges each turn warm/cold honestly.
- Output streams at line granularity from each CLI; token-level streaming
  (`--output-format stream-json`) is a future enhancement.
- Per-agent **token/cost** numbers aren't shown yet (the usage panel reports
  calls + wall-clock + warm/cold); reading them needs each CLI's JSON usage
  envelope and is future work.
- The cockpit shows `git diff` and offers a guarded **discard**
  (`git checkout -- .`); per-hunk accept/reject is future work.
- The app is a separate workspace crate with its own dependencies; the core
  `par` CLI stays dependency-free.
