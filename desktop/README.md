<div align="center">

# Parley Desktop

**One chat. Every agent. Fused.**

A small, fast desktop chat app over the coding-agent CLIs already on your
machine — Claude, Codex, Antigravity, and more. Pick a **primary** agent, flip on
**Fuse** to convene a panel your primary then judges, and switch models (and
provider, for Cursor/OpenCode) inline. One conversation, warm sessions
underneath.

Built with **Tauri** (Rust) + **React** + **[CruzUI](https://www.cruzjs.dev/)**.

</div>

---

## What makes it more than a chat box

One conversation, every agent underneath — with the things a one-shot CLI call
can't give you (all implemented in the Rust backend, see [../README.md](../README.md)):

- **Primary + Fuse.** Choose your primary agent; toggle **Fuse** and the same
  message fans out to a panel (configurable chips) that your **primary judges**
  into one answer — panes stream live with each agent's color, a consensus strip
  (agree/clash), and the fused result.
- **Warm session pinning.** Each agent keeps a *resumed* session per chat, so
  follow-ups reuse its warm prompt cache. Turns are badged **warm**.
- **Shared cross-agent memory.** One canonical transcript; each agent is fed only
  the *delta* since it last spoke. `@agent` / `@panel` / `@auto` direct a single
  message.
- **Model picker.** Inline model selection with quick-presets; **provider +
  model** for Cursor/OpenCode.
- **Code cockpit.** Live `git diff` of the agents' file changes alongside the chat.

Because it drives the real CLIs, it inherits each agent's own auth,
subscription, and caching — no API keys, your code never leaves your machine.

## Architecture

```text
desktop/
  app/                    React + CruzUI frontend (Vite)
    src/
      App.tsx             the chat UI (CruzUI: Select, Switch, Popover, Card,
                          AiPromptInput, ScrollArea, Badge, …)
      bridge.ts           Tauri invoke/listen + a browser mock for dev/preview
      agents.ts           per-agent colors, names, model presets
      cruz.ts             cherry-picked CruzUI components (by subpath)
      index.css           Tailwind v4 + the Parley theme tokens (@theme)
  src-tauri/              Rust backend (stateful orchestrator over `par`)
    src/main.rs           list_agents · send_message · git_diff · usage_stats
    tauri.conf.json       loads the Vite dev server / built dist
```

The frontend has no model logic — it drives the [`par`](../README.md) binary via
the Tauri backend, which owns all harness/route/fuse/session logic.

## Prerequisites

1. **`par` on your PATH** (see the [root README](../README.md)); or set
   `PARLEY_BIN=/abs/path/to/par`.
2. **At least one agent CLI installed** (`par install <agent>`), authenticated.
3. **Node ≥ 18** and **Rust**, plus Tauri's system deps
   (Linux: `webkit2gtk-4.1`, `libsoup-3.0` — see
   <https://tauri.app/start/prerequisites/>).
4. **CruzUI** — `@cruzjs/ui` installs from **public npm** (the `@cruzjs` scope is
   pinned to public in `app/.npmrc`); no special registry needed.

## Run it

```sh
cd desktop/app && npm install        # installs React + CruzUI + Tauri api
cd ..

# one-time: the Tauri CLI
cargo install tauri-cli --version '^2'

# dev (Vite HMR + the Tauri window)
cargo tauri dev --config desktop/src-tauri/tauri.conf.json

# production bundle (.app / .dmg / .deb / .AppImage / .msi)
cargo tauri build --config desktop/src-tauri/tauri.conf.json
```

The frontend can also be previewed in a plain browser without Tauri — `bridge.ts`
falls back to a mock that streams realistic events:

```sh
cd desktop/app && npm run dev    # http://localhost:1420
```

## Notes & limits

- Warm pinning is exact for **claude** (own session id); **codex**/**antigravity**
  /**gemini** resume the most recent session here; others fall back to a cold
  context preamble. Badged honestly per turn.
- Output streams at line granularity; token-level streaming is future work.
- The model picker is free-text with presets (model names change fast) — the
  presets are starting points, edit freely.
- `@cruzjs/ui` ships TS source coupled (via its barrel) to the CruzJS app
  framework; this app imports the components it needs **by subpath** (`cruz.ts`)
  to stay a standalone Tauri app. Tailwind v4 scans the package for classes via
  the `@source` directive in `index.css`.
