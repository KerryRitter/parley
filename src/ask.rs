//! Agent-to-agent calls: run one agent headless and return its reply as a
//! value, optionally seeded with another agent's session transcript.
//!
//! This is the building block behind `par ask` and the MCP `ask_agent` tool:
//! because `par` already routes a prompt to any agent, "Claude asks Gemini" is
//! just routing the prompt to Gemini headless and capturing its stdout. With a
//! `context` reference, `par` first reads the source agent's transcript (via the
//! session module) and prepends it, so the answer is informed by that history.

use std::env;
use std::path::PathBuf;

use crate::cli::{AskOptions, CliOptions};
use crate::harness::{HarnessFactory, Invocation, Request};
use crate::process::{capture_invocation_timeout, Captured, Timeouts};
use crate::session;

/// A reference to a prior session to inject as context: which agent, and which
/// session (`""`/`"latest"` for the newest in the cwd, or an explicit id).
#[derive(Clone, Debug)]
pub(crate) struct ContextRef {
    pub harness: String,
    pub session: String,
}

/// A fully-resolved request to ask one agent something.
pub(crate) struct AskRequest {
    pub harness: String,
    pub prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub cwd: PathBuf,
    pub yolo: bool,
    pub context: Option<ContextRef>,
    pub max_context_chars: usize,
}

/// Build the headless invocation, injecting transcript context into the prompt
/// when requested. Separated from running so `--dry-run` can show the command.
pub(crate) fn build(req: &AskRequest) -> Result<Invocation, String> {
    let prompt = match &req.context {
        Some(ctx) => {
            let preamble = session::transcript_context(
                &ctx.harness,
                &ctx.session,
                &req.cwd,
                req.max_context_chars,
            )?;
            format!(
                "{preamble}\n\n---\n\nUsing the conversation above as context, respond to this:\n\n{}",
                req.prompt
            )
        }
        None => req.prompt.clone(),
    };

    let options = CliOptions {
        harness: req.harness.clone(),
        provider: req.provider.clone(),
        model: req.model.clone(),
        cwd: req.cwd.to_str().map(str::to_string),
        prompt: Some(prompt),
        yolo: req.yolo,
        ..CliOptions::default()
    };

    let request = Request::from_options(options, String::new())?;
    let harness = HarnessFactory::default().create(&request.harness)?;
    harness.build(&request)
}

/// Build and run the call, returning the target agent's captured output. A
/// watchdog (configurable via `PARLEY_TIMEOUT` / `PARLEY_IDLE_TIMEOUT`) kills a
/// hung agent so a single stuck panelist can't wedge a whole `fuse`.
pub(crate) fn run(req: &AskRequest) -> Result<Captured, String> {
    let invocation = build(req)?;
    capture_invocation_timeout(invocation, req.cwd.to_str(), Timeouts::from_env())
}

/// `par ask` entry point: resolve options, then run (or print under dry-run).
pub(crate) fn run_cli(options: AskOptions) -> Result<(), String> {
    let dry_run = options.dry_run;
    let req = resolve(options)?;

    if dry_run {
        println!("{}", build(&req)?.to_json());
        return Ok(());
    }

    let out = run(&req)?;
    print!("{}", out.stdout);
    if !out.stdout.ends_with('\n') {
        println!();
    }
    if !out.success {
        if !out.stderr.trim().is_empty() {
            eprint!("{}", out.stderr);
        }
        return Err(format!("{} exited with a failure status", req.harness));
    }
    Ok(())
}

fn resolve(options: AskOptions) -> Result<AskRequest, String> {
    let cwd = match options.cwd {
        Some(path) => PathBuf::from(path),
        None => env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?,
    };
    Ok(AskRequest {
        harness: options.harness.ok_or("ask requires a target agent")?,
        prompt: options.prompt.ok_or("ask requires a prompt")?,
        model: options.model,
        provider: options.provider,
        cwd,
        yolo: options.yolo,
        context: options.context_from.as_deref().map(parse_context_spec),
        max_context_chars: options
            .max_context_chars
            .unwrap_or(session::DEFAULT_CONTEXT_CHARS),
    })
}

/// Parse a `harness[:session]` context spec. Missing session means "latest".
fn parse_context_spec(spec: &str) -> ContextRef {
    match spec.split_once(':') {
        Some((harness, session)) => ContextRef {
            harness: harness.to_string(),
            session: session.to_string(),
        },
        None => ContextRef {
            harness: spec.to_string(),
            session: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_spec_defaults_to_latest() {
        let c = parse_context_spec("claude");
        assert_eq!(c.harness, "claude");
        assert_eq!(c.session, "");
    }

    #[test]
    fn context_spec_parses_session_id() {
        let c = parse_context_spec("co:abc-123");
        assert_eq!(c.harness, "co");
        assert_eq!(c.session, "abc-123");
    }
}
