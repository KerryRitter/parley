use super::{add_passthrough, provider_qualified_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AiderHarness)
}

struct AiderHarness;

impl Harness for AiderHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["--message".to_string(), request.prompt.clone()];

        if let Some(model) = provider_qualified_model(request) {
            args.extend(["--model".to_string(), model]);
        }

        Ok(Invocation::new("aider", add_passthrough(args, request)))
    }
}
