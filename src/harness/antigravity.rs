use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AntigravityHarness)
}

struct AntigravityHarness;

impl Harness for AntigravityHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();

        // `agy` defaults to an interactive TUI session (bubbletea) that needs a
        // `/dev/tty`; in Parley's headless/routed context that hangs waiting for
        // input or fails to open a TTY. `--print` runs a single prompt
        // non-interactively and prints the response.
        if let Some(prompt) = &request.prompt {
            args.extend(["--print".to_string(), prompt.clone()]);
        }

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("agy", add_passthrough(args, request)))
    }
}
