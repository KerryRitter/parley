use super::{add_passthrough, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(CursorHarness)
}

struct CursorHarness;

impl Harness for CursorHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["-p".to_string(), request.prompt.clone()];

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(format) = &request.output_format {
            args.extend(["--output-format".to_string(), format.clone()]);
        }

        Ok(Invocation::new(
            "cursor-agent",
            add_passthrough(args, request),
        ))
    }
}
