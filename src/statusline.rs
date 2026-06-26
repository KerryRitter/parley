//! `par statusline` — a transcript-free status badge for agents that support a
//! command-driven status line (e.g. Claude Code). The router reconstructs
//! routed-vs-requested model and savings from the session transcript piped on
//! stdin; `par` doesn't route models, so its badge is simpler and needs no
//! transcript at all: the default agent and how many panel agents are available.
//!
//! Wire it into Claude Code's `settings.json`:
//! `"statusLine": { "type": "command", "command": "par statusline" }`.

use std::io::{self, IsTerminal, Read};

use crate::config::DefaultConfig;
use crate::json::Json;
use crate::route;

pub(crate) fn run() -> Result<(), String> {
    // Drain stdin (the harness pipes a JSON blob) so we never block, and pull
    // the cwd if it's there — but the badge works fine without any of it.
    let mut input = String::new();
    if !io::stdin().is_terminal() {
        let _ = io::stdin().read_to_string(&mut input);
    }
    let _cwd = Json::parse(&input).ok().and_then(|j| {
        j.get("workspace")
            .and_then(|w| w.get("current_dir"))
            .or_else(|| j.get("cwd"))
            .and_then(Json::as_str)
            .map(str::to_string)
    });

    let default = DefaultConfig::load()
        .ok()
        .and_then(|c| c.selection.harness)
        .unwrap_or_else(|| "claude".to_string());
    let installed = route::installed_agents();

    println!(
        "⚖ parley · default {default} · {} agents ready",
        installed.len()
    );
    Ok(())
}
