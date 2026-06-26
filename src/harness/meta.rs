//! Meta-harnesses — harnesses that, instead of driving one agent CLI, call back
//! into `par` itself. Because every surface (`fuse` panelists, `converse`
//! participants, `ask`, the run path) turns a harness into an `Invocation` and
//! spawns it, registering these as ordinary harnesses makes them compose
//! *everywhere* for free:
//!
//! ```text
//! par -h auto -p "..."                 # route to the best agent
//! par fuse --panel auto,auto,auto      # three independently-routed panelists
//! par converse --a fuse --b claude     # a whole panel debates one agent
//! par fuse --panel claude,solve        # a panelist that self-escalates
//! par ask -h fuse -p "..."             # ask "the panel" as if it were one agent
//! ```
//!
//! `auto` resolves the router and **delegates** to the chosen real harness in
//! the same process (no extra spawn). `fuse` / `solve` genuinely need to run a
//! `par` subcommand, so they emit a recursive `par <sub>` invocation that the
//! capture path runs and reads back. A depth counter carried in the
//! `PARLEY_META_DEPTH` env var (inherited across the recursive spawns) caps the
//! nesting so `--panel fuse,fuse,...` can't fork-bomb.

use std::env;

use super::{is_meta, normalize_harness, self_bin, Harness, HarnessFactory, Invocation, Request};
use crate::route;

const META_DEPTH_ENV: &str = "PARLEY_META_DEPTH";
const MAX_META_DEPTH: u32 = 4;

pub(crate) fn auto() -> Box<dyn Harness> {
    Box::new(AutoHarness)
}

pub(crate) fn fuse() -> Box<dyn Harness> {
    Box::new(SubHarness { sub: "fuse" })
}

pub(crate) fn solve() -> Box<dyn Harness> {
    Box::new(SubHarness { sub: "solve" })
}

/// `auto` — route the prompt to the best real agent and build *its* invocation
/// directly, so no extra process is spawned.
struct AutoHarness;

impl Harness for AutoHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let prompt = request.prompt.as_deref().unwrap_or_default();
        let (chosen, _reason) = route::resolve_harness("auto", prompt, None);
        let chosen = normalize_harness(&chosen);
        // The router only ever picks real agents; guard anyway so a future
        // table change can't create an auto→meta→auto cycle.
        if is_meta(&chosen) {
            return Err(format!(
                "auto routed to meta-harness \"{chosen}\"; refusing"
            ));
        }
        let mut delegated = request.clone();
        delegated.harness = chosen.clone();
        HarnessFactory::default().create(&chosen)?.build(&delegated)
    }
}

/// `fuse` / `solve` — recurse into `par <sub> -p <prompt>`. The capture path
/// runs it and reads the fused/solved answer back, so the meta behaves like a
/// single agent everywhere a harness is accepted.
struct SubHarness {
    sub: &'static str,
}

impl Harness for SubHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let depth = current_depth();
        if depth >= MAX_META_DEPTH {
            return Err(format!(
                "metaharness recursion limit ({MAX_META_DEPTH}) reached for \"{}\"",
                self.sub
            ));
        }
        let prompt = request
            .prompt
            .clone()
            .ok_or_else(|| format!("the \"{}\" metaharness needs a prompt", self.sub))?;

        let mut args = vec![self.sub.to_string(), "-p".to_string(), prompt];
        if !request.yolo {
            args.push("--no-yolo".to_string());
        }
        // Forward anything after `--` to the nested subcommand, so the nested
        // panel/judge can be configured: `par -h fuse -p x -- --panel cl,co`.
        args.extend(request.passthrough.iter().cloned());

        Ok(Invocation::new(self_bin(), args).with_env(META_DEPTH_ENV, (depth + 1).to_string()))
    }
}

fn current_depth() -> u32 {
    env::var(META_DEPTH_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliOptions;

    fn request(harness: &str, prompt: &str) -> Request {
        Request::from_options(
            CliOptions {
                harness: harness.to_string(),
                prompt: Some(prompt.to_string()),
                yolo: true,
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap()
    }

    #[test]
    fn fuse_meta_recurses_into_par_fuse() {
        let inv = HarnessFactory::default()
            .create("fuse")
            .unwrap()
            .build(&request("fuse", "design X"))
            .unwrap();
        // command is the running par binary; args invoke the fuse subcommand.
        assert_eq!(inv.args, vec!["fuse", "-p", "design X"]);
        assert_eq!(inv.env.get(META_DEPTH_ENV).map(String::as_str), Some("1"));
    }

    #[test]
    fn solve_meta_forwards_passthrough_and_no_yolo() {
        let mut req = request("solve", "fix it");
        req.yolo = false;
        req.passthrough = vec!["--panel".to_string(), "cl,co".to_string()];
        let inv = HarnessFactory::default()
            .create("solve")
            .unwrap()
            .build(&req)
            .unwrap();
        assert_eq!(
            inv.args,
            vec!["solve", "-p", "fix it", "--no-yolo", "--panel", "cl,co"]
        );
    }

    #[test]
    fn auto_meta_delegates_to_a_real_agent() {
        // "design ... architecture" routes to a real agent; the built invocation
        // must be that agent's, never `par`.
        let inv = HarnessFactory::default()
            .create("auto")
            .unwrap()
            .build(&request("auto", "design a scalable architecture"))
            .unwrap();
        assert!(!is_meta(&inv.command));
        assert_ne!(inv.command, self_bin());
    }

    #[test]
    fn depth_guard_trips_at_the_limit() {
        env::set_var(META_DEPTH_ENV, MAX_META_DEPTH.to_string());
        let result = HarnessFactory::default()
            .create("fuse")
            .unwrap()
            .build(&request("fuse", "x"));
        env::remove_var(META_DEPTH_ENV);
        assert!(result.is_err());
    }
}
