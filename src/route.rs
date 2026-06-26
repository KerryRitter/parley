//! Auto-routing — pick the best agent for a prompt instead of making the user
//! choose `-h`. This is the zero-dependency port of workweave/router's routing
//! brain: their two-phase design (heavy offline training → a tiny frozen table →
//! runtime is pure arithmetic: score every candidate, argmax) transfers cleanly;
//! only their on-box neural embedder does not (it needs ONNX + a 100 MB model,
//! which would break `par`'s zero-dependency promise).
//!
//! So `par` swaps the embedder for a dependency-free keyword classifier: a
//! prompt is bucketed into a task class, then each agent is scored against a
//! `class × agent` quality table blended with a speed/cost axis by a single
//! `quality_bias` dial — exactly the router's `quality·α + (1-cost)·(1-α)`
//! shape. The quality numbers are a hand-seeded starting point (the router gets
//! the same cold-start fidelity from public benchmarks); `par stats` surfaces
//! real outcomes so the table can be tuned over time.
//!
//! Surfaces: `par route "<prompt>"` (explain the decision, like the router's
//! `/v1/route`), `par -h auto -p "..."` (route then run), and `--panel auto`
//! for `fuse` (pick a *diverse* panel — the router's "quality-tie band" idea).

use std::env;
use std::path::Path;

use crate::harness::{normalize_harness, spec_for_harness};

/// Default quality/price dial: 0 = cheapest/fastest, 1 = strongest. Matches the
/// router's default lean toward quality. Override with `--bias` or
/// `PARLEY_QUALITY_BIAS`.
pub(crate) const DEFAULT_BIAS: f64 = 0.7;

/// The task buckets a prompt can fall into. Kept small and coding-centric;
/// `General` is the safe fallback when nothing matches (router discipline:
/// conservative classification, never a wrong confident bucket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskClass {
    Debug,
    Code,
    Refactor,
    Test,
    Review,
    Architecture,
    Explain,
    General,
}

impl TaskClass {
    pub(crate) const ALL: [TaskClass; 8] = [
        TaskClass::Debug,
        TaskClass::Code,
        TaskClass::Refactor,
        TaskClass::Test,
        TaskClass::Review,
        TaskClass::Architecture,
        TaskClass::Explain,
        TaskClass::General,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            TaskClass::Debug => "debug",
            TaskClass::Code => "code",
            TaskClass::Refactor => "refactor",
            TaskClass::Test => "test",
            TaskClass::Review => "review",
            TaskClass::Architecture => "architecture",
            TaskClass::Explain => "explain",
            TaskClass::General => "general",
        }
    }

    fn index(self) -> usize {
        match self {
            TaskClass::Debug => 0,
            TaskClass::Code => 1,
            TaskClass::Refactor => 2,
            TaskClass::Test => 3,
            TaskClass::Review => 4,
            TaskClass::Architecture => 5,
            TaskClass::Explain => 6,
            TaskClass::General => 7,
        }
    }

    /// Phrases (multi-word ⇒ substring match, single word ⇒ token match) that
    /// signal this class. `General` has none — it wins only by default.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            TaskClass::Debug => &[
                "debug",
                "bug",
                "fix",
                "error",
                "crash",
                "panic",
                "traceback",
                "stack trace",
                "failing",
                "broken",
                "doesn't work",
                "not working",
                "regression",
                "exception",
                "segfault",
                "why is",
                "why does",
            ],
            TaskClass::Code => &[
                "implement",
                "write",
                "add",
                "create",
                "build",
                "feature",
                "function",
                "endpoint",
                "script",
                "generate",
                "code",
                "support for",
                "make a",
                "develop",
            ],
            TaskClass::Refactor => &[
                "refactor",
                "clean up",
                "cleanup",
                "rename",
                "restructure",
                "simplify",
                "extract",
                "deduplicate",
                "reorganize",
                "tidy",
                "modernize",
                "migrate",
            ],
            TaskClass::Test => &[
                "test",
                "tests",
                "unit test",
                "integration test",
                "coverage",
                "mock",
                "fixture",
                "assert",
                "test case",
                "tdd",
            ],
            TaskClass::Review => &[
                "review",
                "audit",
                "security",
                "vulnerability",
                "critique",
                "find bugs",
                "code review",
                "smell",
                "lint",
                "best practice",
                "feedback on",
            ],
            TaskClass::Architecture => &[
                "design",
                "architecture",
                "approach",
                "trade-off",
                "tradeoff",
                "scalable",
                "scale",
                "plan",
                "strategy",
                "should i",
                "high-level",
                "system design",
                "schema",
                "data model",
                "pattern",
            ],
            TaskClass::Explain => &[
                "explain",
                "what is",
                "what are",
                "how does",
                "how do",
                "understand",
                "document",
                "summarize",
                "describe",
                "walk me through",
                "tell me about",
                "overview",
                "clarify",
            ],
            TaskClass::General => &[],
        }
    }
}

/// A candidate agent and its seed quality per task class (indexed by
/// `TaskClass::index`) plus a speed/cost axis (higher = faster/cheaper). The
/// `family` groups same-vendor agents so panels stay diverse.
#[derive(Clone, Copy)]
struct AgentProfile {
    name: &'static str,
    family: &'static str,
    speed: f64,
    quality: [f64; 8],
}

// Seed table — opinionated relative strengths, NOT a benchmark. The point is a
// reasonable day-one router (like the router's pre-trained cold start); tune
// from `par stats` over time. Columns: Debug, Code, Refactor, Test, Review,
// Architecture, Explain, General.
const AGENTS: &[AgentProfile] = &[
    AgentProfile {
        name: "claude",
        family: "anthropic",
        speed: 0.55,
        quality: [0.90, 0.90, 0.92, 0.88, 0.93, 0.94, 0.93, 0.91],
    },
    AgentProfile {
        name: "codex",
        family: "openai",
        speed: 0.55,
        quality: [0.92, 0.93, 0.88, 0.90, 0.86, 0.88, 0.85, 0.89],
    },
    AgentProfile {
        name: "gemini",
        family: "google",
        speed: 0.72,
        quality: [0.84, 0.85, 0.84, 0.83, 0.85, 0.90, 0.92, 0.86],
    },
    AgentProfile {
        name: "cursor",
        family: "cursor",
        speed: 0.68,
        quality: [0.86, 0.88, 0.89, 0.84, 0.82, 0.80, 0.80, 0.85],
    },
    AgentProfile {
        name: "qwen",
        family: "alibaba",
        speed: 0.85,
        quality: [0.80, 0.83, 0.80, 0.80, 0.78, 0.78, 0.80, 0.80],
    },
    AgentProfile {
        name: "kimi",
        family: "moonshot",
        speed: 0.80,
        quality: [0.80, 0.82, 0.80, 0.79, 0.80, 0.83, 0.85, 0.81],
    },
    AgentProfile {
        name: "aider",
        family: "aider",
        speed: 0.75,
        quality: [0.84, 0.86, 0.88, 0.82, 0.78, 0.76, 0.74, 0.82],
    },
    AgentProfile {
        name: "opencode",
        family: "opencode",
        speed: 0.70,
        quality: [0.82, 0.84, 0.83, 0.82, 0.81, 0.82, 0.82, 0.83],
    },
    AgentProfile {
        name: "copilot",
        family: "openai",
        speed: 0.70,
        quality: [0.83, 0.85, 0.82, 0.85, 0.80, 0.78, 0.80, 0.83],
    },
    AgentProfile {
        name: "goose",
        family: "block",
        speed: 0.72,
        quality: [0.78, 0.80, 0.79, 0.78, 0.78, 0.79, 0.78, 0.79],
    },
    AgentProfile {
        name: "amazon-q",
        family: "amazon",
        speed: 0.74,
        quality: [0.76, 0.78, 0.76, 0.77, 0.78, 0.78, 0.80, 0.78],
    },
    AgentProfile {
        name: "antigravity",
        family: "google",
        speed: 0.65,
        quality: [0.78, 0.80, 0.79, 0.78, 0.78, 0.82, 0.83, 0.80],
    },
];

/// One agent's score for a prompt.
#[derive(Clone, Debug)]
pub(crate) struct Ranked {
    pub name: String,
    pub family: String,
    pub quality: f64,
    pub blend: f64,
    pub installed: bool,
}

/// The outcome of routing a prompt.
#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub class: &'static str,
    pub harness: String,
    pub reason: String,
    pub ranked: Vec<Ranked>,
}

/// Resolve the quality/price dial: explicit value wins, else env, else default.
pub(crate) fn resolve_bias(explicit: Option<f64>) -> f64 {
    explicit
        .or_else(|| {
            env::var("PARLEY_QUALITY_BIAS")
                .ok()
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(DEFAULT_BIAS)
        .clamp(0.0, 1.0)
}

/// Classify a prompt into its most likely task class. Ties and no-match fall
/// back to `General`.
pub(crate) fn classify(prompt: &str) -> TaskClass {
    let lower = prompt.to_ascii_lowercase();
    let tokens: std::collections::HashSet<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();

    let mut best = TaskClass::General;
    let mut best_score = 0.0_f64;
    for class in TaskClass::ALL {
        if class == TaskClass::General {
            continue;
        }
        let mut score = 0.0;
        for kw in class.keywords() {
            let hit = if kw.contains(' ') {
                lower.contains(kw)
            } else {
                tokens.contains(kw)
            };
            if hit {
                // Multi-word phrases are stronger, more specific signals.
                score += if kw.contains(' ') { 2.0 } else { 1.0 };
            }
        }
        if score > best_score {
            best_score = score;
            best = class;
        }
    }
    best
}

/// Rank every candidate agent for a prompt under `bias`. When `only_installed`,
/// uninstalled agents are dropped — unless that would empty the field, in which
/// case the full list is kept (soft filter, mirroring the router's eligibility
/// fallbacks).
pub(crate) fn rank(prompt: &str, bias: f64, only_installed: bool) -> (TaskClass, Vec<Ranked>) {
    let class = classify(prompt);
    let idx = class.index();
    let mut ranked: Vec<Ranked> = AGENTS
        .iter()
        .map(|a| {
            let quality = a.quality[idx];
            Ranked {
                name: a.name.to_string(),
                family: a.family.to_string(),
                quality,
                blend: bias * quality + (1.0 - bias) * a.speed,
                installed: is_installed(a.name),
            }
        })
        .collect();

    if only_installed && ranked.iter().any(|r| r.installed) {
        ranked.retain(|r| r.installed);
    }
    // Highest blended score first; stable tiebreak by name for determinism.
    ranked.sort_by(|a, b| {
        b.blend
            .partial_cmp(&a.blend)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    (class, ranked)
}

/// Pick the single best agent for a prompt.
pub(crate) fn pick(prompt: &str, bias: f64, only_installed: bool) -> Decision {
    let (class, ranked) = rank(prompt, bias, only_installed);
    let top = ranked.first().cloned().unwrap_or(Ranked {
        name: "claude".into(),
        family: "anthropic".into(),
        quality: 0.0,
        blend: 0.0,
        installed: false,
    });
    let reason = format!(
        "prompt looks like '{}'; {} ranks highest (quality {:.2}, blend {:.2} at bias {:.2})",
        class.name(),
        top.name,
        top.quality,
        top.blend,
        bias
    );
    Decision {
        class: class.name(),
        harness: top.name.clone(),
        reason,
        ranked,
    }
}

/// Pick a diverse panel of up to `n` agents: the highest-scoring agent from each
/// distinct family (so a panel is never three of the same vendor). This is the
/// router's "quality-tie band → diverse set" idea applied to `fuse`.
pub(crate) fn pick_panel(prompt: &str, n: usize, bias: f64, only_installed: bool) -> Vec<String> {
    let (_, ranked) = rank(prompt, bias, only_installed);
    let mut seen_families = std::collections::HashSet::new();
    let mut panel = Vec::new();
    for r in &ranked {
        if seen_families.insert(r.family.clone()) {
            panel.push(r.name.clone());
            if panel.len() >= n {
                break;
            }
        }
    }
    panel
}

/// The agents from the seed table whose CLI is currently on PATH.
pub(crate) fn installed_agents() -> Vec<String> {
    AGENTS
        .iter()
        .filter(|a| is_installed(a.name))
        .map(|a| a.name.to_string())
        .collect()
}

/// Is an agent's CLI on PATH? Best-effort; failures read as "not installed".
fn is_installed(name: &str) -> bool {
    let Some(spec) = spec_for_harness(name) else {
        return false;
    };
    let Ok(path) = env::var("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| {
        let candidate = dir.join(spec.binary);
        candidate.exists() || Path::new(&format!("{}.exe", candidate.display())).exists()
    })
}

/// `par route` — explain the routing decision (and a suggested panel) without
/// running anything. The dependency-free analog of the router's `/v1/route`.
pub(crate) fn run_cli(
    prompt: &str,
    bias: Option<f64>,
    panel_size: usize,
    json_out: bool,
) -> Result<(), String> {
    let bias = resolve_bias(bias);
    let decision = pick(prompt, bias, true);
    let panel = pick_panel(prompt, panel_size, bias, true);

    if json_out {
        println!("{}", decision_json(&decision, &panel).to_pretty_string());
        return Ok(());
    }

    println!("task class : {}", decision.class);
    println!("→ route to : {}", decision.harness);
    println!("  reason   : {}", decision.reason);
    println!("  panel    : {}", panel.join(", "));
    println!();
    println!(
        "  {:<12} {:<10} {:>8} {:>8}  INSTALLED",
        "AGENT", "FAMILY", "QUALITY", "BLEND"
    );
    for r in &decision.ranked {
        println!(
            "  {:<12} {:<10} {:>8.2} {:>8.2}  {}",
            r.name,
            r.family,
            r.quality,
            r.blend,
            if r.installed { "yes" } else { "—" }
        );
    }
    Ok(())
}

fn decision_json(decision: &Decision, panel: &[String]) -> crate::json::Json {
    use crate::json::Json;
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Json> = BTreeMap::new();
    map.insert("task_class".into(), Json::Str(decision.class.to_string()));
    map.insert("harness".into(), Json::Str(decision.harness.clone()));
    map.insert("reason".into(), Json::Str(decision.reason.clone()));
    map.insert(
        "panel".into(),
        Json::Array(panel.iter().map(|p| Json::Str(p.clone())).collect()),
    );
    let candidates: Vec<Json> = decision
        .ranked
        .iter()
        .map(|r| {
            let mut m: BTreeMap<String, Json> = BTreeMap::new();
            m.insert("name".into(), Json::Str(r.name.clone()));
            m.insert("family".into(), Json::Str(r.family.clone()));
            m.insert("quality".into(), Json::Number(r.quality));
            m.insert("blend".into(), Json::Number(r.blend));
            m.insert("installed".into(), Json::Bool(r.installed));
            Json::Object(m)
        })
        .collect();
    map.insert("candidates".into(), Json::Array(candidates));
    Json::Object(map)
}

/// Resolve a harness selector that may be `auto`: when it is, route the prompt
/// and return the chosen real harness plus the human reason; otherwise pass the
/// selector through unchanged with no reason.
pub(crate) fn resolve_harness(
    selector: &str,
    prompt: &str,
    bias: Option<f64>,
) -> (String, Option<String>) {
    if normalize_harness(selector) != "auto" && selector != "auto" {
        return (selector.to_string(), None);
    }
    let decision = pick(prompt, resolve_bias(bias), true);
    (decision.harness.clone(), Some(decision.reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_intents() {
        assert_eq!(
            classify("fix the failing test, it crashes"),
            TaskClass::Debug
        );
        assert_eq!(classify("implement a new login endpoint"), TaskClass::Code);
        assert_eq!(
            classify("refactor this module and simplify it"),
            TaskClass::Refactor
        );
        assert_eq!(
            classify("design a scalable rate limiter architecture"),
            TaskClass::Architecture
        );
        assert_eq!(classify("explain how this works"), TaskClass::Explain);
        assert_eq!(
            classify("do a security review of this diff"),
            TaskClass::Review
        );
        assert_eq!(classify("hello there friend"), TaskClass::General);
    }

    #[test]
    fn bias_tilts_between_quality_and_speed() {
        // High bias favors the strongest agent; low bias favors a fast/cheap one.
        let (_, high) = rank("design an architecture", 1.0, false);
        let (_, low) = rank("design an architecture", 0.0, false);
        assert_eq!(high[0].name, "claude");
        // At bias 0 the ranking is pure speed; qwen is the fastest seed.
        assert_eq!(low[0].name, "qwen");
    }

    #[test]
    fn panel_is_vendor_diverse() {
        let panel = pick_panel("review this code", 3, DEFAULT_BIAS, false);
        assert_eq!(panel.len(), 3);
        // No two panelists share a family.
        let fams: Vec<&'static str> = panel
            .iter()
            .map(|n| AGENTS.iter().find(|a| a.name == n).unwrap().family)
            .collect();
        let distinct: std::collections::HashSet<_> = fams.iter().collect();
        assert_eq!(distinct.len(), fams.len());
    }

    #[test]
    fn resolve_harness_passes_through_non_auto() {
        let (h, reason) = resolve_harness("codex", "anything", None);
        assert_eq!(h, "codex");
        assert!(reason.is_none());
    }

    #[test]
    fn resolve_harness_routes_auto() {
        let (h, reason) = resolve_harness("auto", "design an architecture", Some(1.0));
        assert_eq!(h, "claude");
        assert!(reason.unwrap().contains("architecture"));
    }

    #[test]
    fn bias_resolution_clamps_and_defaults() {
        assert_eq!(resolve_bias(Some(2.0)), 1.0);
        assert_eq!(resolve_bias(Some(-1.0)), 0.0);
        assert_eq!(resolve_bias(None), DEFAULT_BIAS);
    }
}
