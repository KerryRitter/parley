use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AntigravityHarness)
}

struct AntigravityHarness;

impl Harness for AntigravityHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();

        // `agy --model` wants an exact model *label* (e.g. "Gemini 3.1 Pro
        // (High)", "Claude Sonnet 4.6 (Thinking)") as printed by `agy models` —
        // there is no provider/model slash form, so pass it through plain.
        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }

        let mut args = add_yolo_args(args, request)?;

        // With a prompt, run non-interactively via `--print <prompt>`. `--print`
        // is a *string flag* that takes the prompt as its value, so it must come
        // last and be immediately followed by the prompt. A bare positional
        // prompt instead drops `agy` into its interactive TUI, which then hangs
        // (or errors "could not open TTY") under headless capture — the source
        // of the ask/fuse hangups against antigravity. No prompt → interactive.
        if let Some(prompt) = &request.prompt {
            args.extend(["--print".to_string(), prompt.clone()]);
        }

        Ok(Invocation::new("agy", add_passthrough(args, request)))
    }
}
