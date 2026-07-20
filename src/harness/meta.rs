//! Meta-harness — a harness that, instead of driving one agent CLI, calls back
//! into `par` itself. Because every surface (`fuse` panelists, `converse`
//! participants, `ask`, the run path) turns a harness into an `Invocation` and
//! spawns it, registering this as an ordinary harness makes it compose
//! *everywhere* for free:
//!
//! ```text
//! par converse --a fuse --b claude     # a whole panel debates one agent
//! par ask -h fuse -p "..."             # ask "the panel" as if it were one agent
//! ```
//!
//! `fuse` genuinely needs to run a `par` subcommand, so it emits a recursive
//! `par fuse` invocation that the capture path runs and reads back. A depth
//! counter carried in the `PARLEY_META_DEPTH` env var (inherited across the
//! recursive spawns) caps the nesting so `--panel fuse,fuse,...` can't
//! fork-bomb.

use std::env;

use super::{self_bin, Harness, Invocation, Request};

const META_DEPTH_ENV: &str = "PARLEY_META_DEPTH";
const MAX_META_DEPTH: u32 = 4;

pub(crate) fn fuse() -> Box<dyn Harness> {
    Box::new(SubHarness { sub: "fuse" })
}

/// `fuse` — recurse into `par fuse -p <prompt>`. The capture path runs it and
/// reads the fused answer back, so the meta behaves like a single agent
/// everywhere a harness is accepted.
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
    use crate::harness::HarnessFactory;

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
    fn fuse_meta_forwards_passthrough_and_no_yolo() {
        let mut req = request("fuse", "design it");
        req.yolo = false;
        req.passthrough = vec!["--panel".to_string(), "cl,co".to_string()];
        let inv = HarnessFactory::default()
            .create("fuse")
            .unwrap()
            .build(&req)
            .unwrap();
        assert_eq!(
            inv.args,
            vec!["fuse", "-p", "design it", "--no-yolo", "--panel", "cl,co"]
        );
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
