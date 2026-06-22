# The Parley Fusion Playbook

**How to make your coding agents ~10× smarter by running them as a panel instead of alone.**

This is the practical companion to Parley's positioning. Every pattern below works **today** on `par ask` and `par converse` — no new binary, no API keys. It borrows the two ideas that frontier labs have shown beat single models:

- **OpenRouter Fusion** — fan a prompt to a panel of models, then a *judge* synthesizes one answer from consensus, contradictions, gaps, and blind spots.
- **Sakana AB-MCTS** — search **wider** (new solutions) and **deeper** (refine the best), picking the strongest model for each step.

The whole game: **one model has one set of blind spots; a diverse panel covers each other's.**

---

## The mental model

| Move | What it buys you | Parley primitive |
| --- | --- | --- |
| **Panel** — same prompt to N agents in parallel | Coverage; consensus = high confidence | `par fuse` / mcp `fuse` tool (native), or `par ask` ×N |
| **Judge** — one agent synthesizes the panel | A single grounded answer, not N to read | `par ask` with the transcripts inlined |
| **Debate** — two agents argue to convergence | Surfaces hidden assumptions, kills weak ideas | `par converse --until` |
| **Wider** — K candidate solutions from scratch | Escapes one model's local optimum | `par ask` ×K, varied seed |
| **Deeper** — refine the best candidate | Polishes the winner | `par ask --context-from` |
| **Second opinion** — cross-agent review with context | Catches what your main agent missed | `par ask --context-from` |

---

## Rule 0 — Escalation, not autopilot

OpenRouter's own guidance: Fusion is for "questions worth spending more time and money to get a thorough answer." A panel costs N× the tokens and N× the wall-clock. **Don't fuse everything.** Fuse when the cost of being *wrong* dwarfs the cost of a few extra runs:

- ✅ Architecture decisions, security reviews, tricky migrations, "design X", root-causing a heisenbug, anything you'd ask a senior engineer to sanity-check.
- ❌ Renaming a variable, writing a commit message, formatting, one-line fixes. Use a single `par -p` for these.

If you can't say *why* a second model would disagree, you don't need a panel.

---

## Rule 1 — Diversity is the whole point

A panel of three Claude instances is barely better than one. The lift comes from **different model families with different training and failure modes.** Pick agents that are genuinely different:

- **Reasoning-leaning** + **code-leaning** + **a wildcard** beats three of the same.
- Good default trios: `cl,co,g` (Claude + Codex + Gemini) or `cl,co,k` (+ Kimi).
- Mix providers, not just models — different *vendors* disagree in more useful ways than different model sizes from one vendor.

Sakana's framing: treat each model's specialization (coding, reasoning, creative) as a complementary strength to exploit, not a limitation to average away.

---

## Pattern 1 — Panel + Judge (the Fusion move)

The core pattern. Fan out, then synthesize. Save as `fuse.sh`:

```sh
#!/usr/bin/env bash
# fuse.sh — panel of agents + judge synthesis (Fusion-style)
set -euo pipefail
PROMPT="$1"
PANEL="${2:-cl,co,g}"          # comma-separated agent codes
JUDGE="${3:-cl}"
DIR="$(mktemp -d)"

# 1. Fan out — every panelist answers the same prompt, in parallel.
IFS=',' read -ra AGENTS <<< "$PANEL"
for a in "${AGENTS[@]}"; do
  par ask -h "$a" -p "$PROMPT" > "$DIR/$a.md" &
done
wait

# 2. Build the judge prompt from the panel's answers.
JUDGE_PROMPT="You are the judge in a multi-model panel. ${#AGENTS[@]} agents independently answered the QUESTION below.

Produce, in order:
1. CONSENSUS — claims most/all agents agree on (treat as high-confidence).
2. CONTRADICTIONS — where they disagree, and which side is right and why.
3. GAPS — points only one agent raised that the others missed.
4. BLIND SPOTS — anything important NO agent addressed.
5. FINAL ANSWER — the single best answer, grounded in the above. Do not average; pick the strongest reasoning.

QUESTION:
$PROMPT
"
for a in "${AGENTS[@]}"; do
  JUDGE_PROMPT+="
--- Agent: $a ---
$(cat "$DIR/$a.md")
"
done

# 3. Judge synthesizes.
par ask -h "$JUDGE" -p "$JUDGE_PROMPT"
rm -rf "$DIR"
```

```sh
./fuse.sh "Design a rate limiter for a multi-tenant API. Trade-offs and a recommendation."
./fuse.sh "Is this migration safe to run on a live DB?" cl,co,k co
```

**Why it works:** consensus across independent models is a strong correctness signal; contradictions are exactly where a single agent would have silently been wrong. The judge turns N transcripts into one decision.

**The native way — `par fuse` (built in).** This is built into Parley, two ways over one engine:

```sh
par fuse "design a rate limiter for this service"        # panel: claude,codex,gemini; judge: claude
par fuse "..." --panel cl,co,k --judge co                # pick the panel and judge
```

And the same thing as an **MCP tool**, so the model convenes its own panel mid-task. Register once (`par mcp connect -h cl`), then from inside your agent:

> *"fuse this across codex and gemini: design a rate limiter for this service"*

The agent calls `fuse {prompt, panel?, judge?, context_from?}`; Parley runs the panel in parallel and a judge (Claude by default) synthesizes one answer. Reach for `fuse.sh` only when scripting fusion *outside* an agent (CI, a cron job) or to customize the judge prompt yourself.

---

## Pattern 2 — Debate to convergence (the deliberation move)

When the answer needs *pressure-testing*, not just collecting, make two agents argue. This already ships:

```sh
par converse --a cl --b g \
  -p "A proposes an approach; B's job is to find the strongest objection. Converge only when both agree, then emit AGREED." \
  --turns 8 --until AGREED
```

Use it for: choosing between two designs, validating a risky plan, "red-team this." The `--until` phrase ends it the moment they settle. Watch the turns — disagreement that *persists* is a signal the problem is genuinely hard, not that the tool failed.

---

## Pattern 3 — Wider then deeper (the AB-MCTS move)

For hard problems where the first idea is rarely the best: generate **wide**, judge, then refine **deep**.

```sh
#!/usr/bin/env bash
# explore.sh — K wide candidates, pick best, refine deep.
set -euo pipefail
PROMPT="$1"; K="${2:-3}"; DIR="$(mktemp -d)"

# WIDER: K independent attempts, nudged to differ.
ANGLES=("Optimize for simplicity." "Optimize for performance." "Optimize for correctness/safety.")
for i in $(seq 0 $((K-1))); do
  par ask -h cl -p "$PROMPT

Constraint for this attempt: ${ANGLES[$i % ${#ANGLES[@]}]}" > "$DIR/cand$i.md" &
done
wait

# JUDGE picks the strongest candidate.
SEL="Pick the single strongest candidate below and explain in one line why. Output only its number.
$(for i in $(seq 0 $((K-1))); do echo "--- Candidate $i ---"; cat "$DIR/cand$i.md"; done)"
BEST=$(par ask -h cl -p "$SEL" | grep -oE '[0-9]+' | head -1)

# DEEPER: a second model refines the winner (different eyes on the best idea).
par ask -h co -p "Refine and harden this solution. Fix weaknesses, add edge cases, tighten the code:
$(cat "$DIR/cand${BEST:-0}.md")"
rm -rf "$DIR"
```

The varied "angles" force genuine width (Sakana's go-wider); a *different* model doing the refine pass is go-deeper with fresh eyes. For the strongest version, run the wide step across *different agents* rather than one agent K times.

---

## Pattern 4 — Always-on second opinion

The cheapest 80% of the benefit, for daily use. After your main agent does the work, have a *different* agent review it with full context — over MCP, your agent can do this itself:

```sh
# After a Claude session, get Gemini to red-team it with that context:
par ask -h g -p "Review the approach in 3 bullets. What's the biggest risk we're not seeing?" \
  --context-from cl
```

Wire `par mcp` into your main agent and the instruction becomes natural language: *"ask Gemini to review this with my Claude context."* This is panel-of-two with near-zero ceremony — make it a habit on anything you'd hesitate to ship.

---

## Rule 2 — Make the judge earn its keep

The judge is where fusion adds value or wastes tokens. Good judge prompts:

- **Forbid averaging.** "Pick the strongest reasoning; do not blend." Averaging regresses toward the mediocre answer.
- **Demand the structure** (consensus / contradiction / gap / blind spot). Unstructured "summarize these" loses the disagreement signal that's the entire point.
- **Make it resolve, not relay.** The final answer must be a decision, not "Agent A said X, Agent B said Y."
- **Consider a different judge than panelists**, or rotate. A judge grading its own panel answer is biased toward it.

---

## Rule 3 — Budget like it's compute, because it is

- A 3-agent panel + judge ≈ **4× tokens and 4× the slowest agent's latency.** Parallelize the panel (the scripts do); the judge is the only serial step.
- Cap injected context: `par ask --max-context 8000` keeps judge prompts from ballooning.
- Start with a **2-agent panel** for most "worth a second opinion" tasks; reserve 3–4 for genuinely high-stakes calls.
- Budget panels work: pairing cheaper models can beat a single frontier model at lower total cost (OpenRouter's budget-panel result). Don't assume you need the most expensive agent in every seat.

---

## Anti-patterns

- **Homogeneous panels** — three of the same model family. Near-zero lift; pure waste. (Rule 1)
- **Fusing trivia** — panels on a rename or a commit message. (Rule 0)
- **Averaging judge** — "blend these into a compromise." Produces the bland middle, not the best answer. (Rule 2)
- **Silent truncation** — letting long transcripts blow the judge's context with no cap. Use `--max-context`. (Rule 3)
- **Infinite debates** — `par converse` with no `--until` and a high `--turns`. Set both.
- **Trusting consensus blindly** — three models can share the *same* training bias and be confidently, identically wrong. Consensus raises confidence; it doesn't guarantee truth. Keep a human on the high-stakes call.

---

## TL;DR — the 10× checklist

1. Fuse **selectively** — high-stakes only (Rule 0).
2. Pick a **diverse** panel — different vendors, not different sizes (Rule 1).
3. Default move: **panel + judge** — `par fuse "..."` (or the `fuse` MCP tool); judge defaults to Claude.
4. Need pressure, not coverage? **Debate** with `par converse --until`.
5. Hard problem? **Wider then deeper** — many candidates, pick best, refine with fresh eyes.
6. Daily habit: **second opinion** via `par ask --context-from` / `par mcp`.
7. Make the judge **resolve and structure**, never average.
8. Parallelize the panel; **cap context**; start at 2 agents.
