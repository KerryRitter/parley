//! Multi-turn agent-to-agent conversation.
//!
//! `par converse` puts two agents in a loop: A speaks, B replies, A replies, and
//! so on. Each `ask` is stateless and headless, so `par` itself holds the
//! running cross-agent dialogue and re-injects it as context every turn — the
//! same context-bridge trick `par ask` uses, but accumulated across the loop.
//! Output streams turn by turn so you can watch the two agents talk.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::ask::{self, AskRequest};
use crate::cli::ConverseOptions;
use crate::harness::normalize_harness;
use crate::route;
use crate::session::{self, Turn};
use crate::signals;
use crate::telemetry::Event;

/// Hard cap on turns to keep a runaway loop from spawning endless agents.
const MAX_TURNS: usize = 50;

struct Speaker {
    harness: String,
    model: Option<String>,
}

pub(crate) fn run_cli(options: ConverseOptions) -> Result<(), String> {
    let a = options.a.ok_or("converse requires --a <agent>")?;
    let b = options.b.ok_or("converse requires --b <agent>")?;
    let topic = options
        .topic
        .ok_or("converse requires a topic: -p \"<task>\"")?;

    if options.turns == 0 {
        return Err("--turns must be at least 1".to_string());
    }
    if options.turns > MAX_TURNS {
        return Err(format!("--turns capped at {MAX_TURNS}"));
    }

    let cwd = match options.cwd {
        Some(path) => PathBuf::from(path),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };
    let max_context = options
        .max_context_chars
        .unwrap_or(session::DEFAULT_CONTEXT_CHARS);
    let until = options.until.as_deref().filter(|s| !s.is_empty());

    let speakers = [
        Speaker {
            harness: normalize_harness(&a),
            model: options.a_model,
        },
        Speaker {
            harness: normalize_harness(&b),
            model: options.b_model,
        },
    ];

    // Optional seed: a prior session transcript prepended to the first prompt.
    let seed = match &options.context_from {
        Some(spec) => {
            let (harness, sel) = match spec.split_once(':') {
                Some((h, s)) => (h, s),
                None => (spec.as_str(), ""),
            };
            Some(session::transcript_context(
                harness,
                sel,
                &cwd,
                max_context,
            )?)
        }
        None => None,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let started = Instant::now();
    let mut looped = false;
    let mut dialogue: Vec<Turn> = Vec::new();
    for turn in 0..options.turns {
        let speaker = &speakers[turn % 2];
        let other = &speakers[(turn + 1) % 2];

        let prompt = build_turn_prompt(
            &topic,
            &dialogue,
            &speaker.harness,
            &other.harness,
            seed.as_deref().filter(|_| turn == 0),
            until,
            max_context,
        );

        let req = AskRequest {
            harness: speaker.harness.clone(),
            prompt,
            model: speaker.model.clone(),
            provider: None,
            cwd: cwd.clone(),
            yolo: options.yolo,
            context: None,
            max_context_chars: max_context,
        };

        if options.dry_run {
            writeln!(
                out,
                "# turn {} ({}) — dry run\n{}",
                turn + 1,
                speaker.harness,
                ask::build(&req)?.to_json()
            )
            .ok();
            if turn == 0 {
                return Ok(());
            }
            continue;
        }

        let result = ask::run(&req)?;
        let text = result.stdout.trim().to_string();
        if !result.success {
            let err = if result.stderr.trim().is_empty() {
                &text
            } else {
                result.stderr.trim()
            };
            return Err(format!(
                "{} failed on turn {}: {err}",
                speaker.harness,
                turn + 1
            ));
        }

        writeln!(out, "\n━━ turn {} · {} ━━", turn + 1, speaker.harness).ok();
        writeln!(out, "{text}").ok();
        out.flush().ok();

        let stop = until
            .map(|u| text.to_lowercase().contains(&u.to_lowercase()))
            .unwrap_or(false);
        dialogue.push(Turn {
            role: speaker.harness.clone(),
            text,
        });
        if stop {
            writeln!(out, "\n(stop phrase \"{}\" reached)", until.unwrap()).ok();
            break;
        }
        // Loop detection: if the last few replies have collapsed onto the same
        // answer, the two agents have stopped making progress — stop early
        // rather than burning the rest of the turn budget.
        let replies: Vec<String> = dialogue.iter().map(|t| t.text.clone()).collect();
        if signals::replies_looping(&replies, 3) {
            writeln!(
                out,
                "\n(stopping: the conversation is looping — replies stopped progressing)"
            )
            .ok();
            looped = true;
            break;
        }
    }

    Event {
        cmd: "converse",
        panel: vec![speakers[0].harness.clone(), speakers[1].harness.clone()],
        task_class: Some(route::classify(&topic).name().to_string()),
        prompt: Some(topic.clone()),
        ok: !looped,
        duration_ms: started.elapsed().as_millis(),
        looped,
        ..Event::default()
    }
    .record();

    Ok(())
}

fn build_turn_prompt(
    topic: &str,
    dialogue: &[Turn],
    me: &str,
    other: &str,
    seed: Option<&str>,
    until: Option<&str>,
    max_context: usize,
) -> String {
    let mut p = String::new();
    if let Some(seed) = seed {
        p.push_str(seed);
        p.push_str("\n\n---\n\n");
    }

    if dialogue.is_empty() {
        p.push_str(&format!(
            "You are \"{me}\", starting a working conversation with another agent (\"{other}\") about the task below. Open the discussion.\n\nTask: {topic}"
        ));
    } else {
        let transcript = render_dialogue(dialogue, max_context);
        p.push_str(&format!(
            "You are \"{me}\", in a working conversation with \"{other}\".\n\n=== Conversation so far ===\n{transcript}\n\n=== Your turn ===\nRespond to the latest message and move the task forward. Be concise and concrete; don't repeat what was already said.\n\nTask: {topic}"
        ));
    }

    if let Some(until) = until {
        p.push_str(&format!(
            "\n\nWhen the task is fully resolved and you agree it is done, end your message with the exact word: {until}"
        ));
    }
    p
}

/// Render the running dialogue, keeping the most recent turns within budget.
fn render_dialogue(dialogue: &[Turn], max_chars: usize) -> String {
    let mut body = String::new();
    for turn in dialogue {
        body.push_str(&format!("[{}]\n{}\n\n", turn.role, turn.text.trim()));
    }
    let body = body.trim_end();
    if body.chars().count() <= max_chars {
        return body.to_string();
    }
    let start = body.chars().count() - max_chars;
    let tail: String = body.chars().skip(start).collect();
    format!("[...earlier turns omitted...]\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: &str, text: &str) -> Turn {
        Turn {
            role: role.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn first_turn_opens_the_discussion() {
        let p = build_turn_prompt("build X", &[], "claude", "gemini", None, None, 1000);
        assert!(p.contains("starting a working conversation"));
        assert!(p.contains("Task: build X"));
        assert!(!p.contains("Conversation so far"));
    }

    #[test]
    fn later_turn_includes_dialogue_and_stop_hint() {
        let d = vec![turn("claude", "hello"), turn("gemini", "hi back")];
        let p = build_turn_prompt("build X", &d, "claude", "gemini", None, Some("DONE"), 1000);
        assert!(p.contains("Conversation so far"));
        assert!(p.contains("[gemini]"));
        assert!(p.contains("end your message with the exact word: DONE"));
    }

    #[test]
    fn dialogue_truncates_to_recent() {
        let d = vec![turn("a", &"x".repeat(500)), turn("b", &"y".repeat(500))];
        let rendered = render_dialogue(&d, 100);
        assert!(rendered.starts_with("[...earlier turns omitted...]"));
        assert!(rendered.chars().count() < 200);
    }
}
