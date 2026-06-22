# Parley — Positioning

> Repositioning brief + ready-to-ship copy. The thesis: Parley is the **collective-intelligence layer** for the coding agents already on your machine.

---

## The one-line shift

| Before | After |
| --- | --- |
| "One prompt interface for every AI coding agent — route, resume, bridge, and converse across them." | "Stop picking one agent. **Convene them.** Parley fuses Claude, Codex, Gemini and a dozen more into one answer — the multi-model deliberation behind Sakana's AB-MCTS and OpenRouter's Fusion, running over the CLIs already on your machine." |

Interoperability is the *mechanism*. Collective intelligence is the *payoff*. Lead with the payoff.

---

## Tagline options

1. **Your agents are smarter together. Parley is how.**
2. **One model has blind spots. A panel doesn't.**
3. **Fusion for your terminal — over the agents you already run.**
4. **Stop picking one agent. Convene them.**

Recommended primary: **"Your agents are smarter together. Parley is how."** — it names the outcome (smarter) and the role (the layer) in six words.

---

## Hero copy (drop-in README intro)

> **Your coding agents are smarter together. Parley is how.**
>
> A single model has a single set of blind spots. Parley convenes a *panel* of the agent CLIs you already run — Claude, Codex, Gemini, Cursor, Qwen, and more — sends them the same problem, and fuses their answers into one. Where they agree, you get high-confidence consensus. Where they disagree, you get a flag worth your attention. What none of them caught, the panel surfaces.
>
> This is the same idea as **Sakana's AB-MCTS** (multiple frontier models cooperating at inference time) and **OpenRouter's Fusion** (a panel of models plus a judge that synthesizes). Both report the same result: *combined models beat any single one.* Parley brings it to your terminal — over the CLIs you already trust, with their own auth and context, **no API keys and no new vendor.**
>
> ```sh
> par fuse "design a rate limiter for this service"                    # panel + judge → one fused answer
> par converse --a cl --b g -p "A proposes, B refutes; converge on a design."
> par ask -h g -p "find the flaw in this plan" --context-from cl       # cross-agent second opinion
> ```
>
> `fuse` is also an MCP tool — `par mcp connect -h cl`, then Claude convenes its own panel: *"fuse this across codex and gemini."*

---

## The "Why" — reframed around collective intelligence

Keep the existing four walls (silos, brittle scripts, duplicated config, stranded work) — they're true and they sell the interop. But lead the section with the new top-line wall:

> **You're trusting one model's judgment.** Every coding agent ships one model's training, one model's failure modes, one model's blind spots. On anything that matters — an architecture call, a security review, a tricky migration — a single agent is a single point of view. There's no second opinion, no disagreement to flag risk, no way to combine the model that's best at reasoning with the model that's best at code.
>
> Research labs already solved this. Sakana's AB-MCTS lets frontier models cooperate at inference time and reports problems *no single model could solve* becoming solvable. OpenRouter's Fusion runs a panel plus a judge and reports a fused pair beating every individual model. The catch: both are API-side, single-vendor, and your code leaves your machine.
>
> **Parley does it locally.** It already routes to a dozen agent CLIs, bridges context between them, and runs them headless. That's exactly the substrate deliberation needs — so Parley convenes those agents into a panel and fuses the result, using the auth, models, and permissions that already live with each agent.

---

## Why Parley's angle is different from Fusion / AB-MCTS

This is the differentiation to hammer — don't let it read as a clone.

| | OpenRouter Fusion | Sakana AB-MCTS | **Parley** |
| --- | --- | --- | --- |
| Runs over | Model APIs (one vendor) | Model APIs | **The agent CLIs on your machine** |
| Auth / billing | OpenRouter account | Per-provider keys | **Each agent's existing login** |
| Code leaves machine | Yes (to OpenRouter) | Yes | **No — local CLIs, your own context** |
| Agents have tools/repo access | Limited | No | **Yes — they're full coding agents in your repo** |
| Lock-in | Vendor router | Research/enterprise product | **None — open CLI, swap any agent by a flag** |

One sentence to remember: **Fusion fuses models; Parley fuses agents** — full coding agents with repo access, your tools, and your context, not stateless chat completions.

---

## Proof points (cite, don't claim as ours)

Use these as *evidence the approach works*, attributed to the source — never as Parley's own benchmark.

- AB-MCTS on ARC-AGI-2: single o4-mini ~23% → AB-MCTS ~27.5% → multi-model ~30%+; "problems unsolvable by any single model became solvable through collaboration." ([Sakana AI](https://sakana.ai/ab-mcts/))
- Fusion on the DRACO deep-research benchmark: Fable 5 + GPT-5.5 fused = 69.0% vs 65.3% solo; a budget panel beat individual frontier models at ~half the cost. ([OpenRouter](https://openrouter.ai/blog/announcements/fusion-beats-frontier/))

---

## What this implies for the product

Fusion ships **two ways over one engine**: the `par fuse` command (terminal) and the `fuse` MCP tool (an agent convenes its own panel mid-task). Both run the panel in parallel and have a judge — Claude by default — synthesize one answer. The MCP surface is what makes it escalation-not-autopilot: the model decides a question is worth a panel and calls `fuse {prompt, panel?, judge?}` itself. The positioning is now literally true:

- `par fuse "..."` from the terminal; or `par mcp connect -h cl` once, then Claude can convene its own panel.
- The [Fusion Playbook](./fusion-playbook.md) layers the *technique* on top — when to fuse, panel diversity, debate, and wider→deeper — using `par fuse` / the `fuse` tool plus `par converse` / `par ask`.

Next, optional: per-panelist models (`--panel cl:opus,co:gpt-5.4`), a structured-analysis return mode, and a wider→deeper loop.
