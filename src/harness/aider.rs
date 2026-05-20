use super::{
    add_passthrough, add_yolo_args, provider_qualified_model, Harness, Invocation, Request,
};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AiderHarness)
}

struct AiderHarness;

impl Harness for AiderHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if let Some(prompt) = &request.prompt {
            args.extend(["--message".to_string(), prompt.clone()]);
        }

        if let Some(model) = provider_qualified_model(request) {
            args.extend(["--model".to_string(), model]);
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("aider", add_passthrough(args, request)))
    }
}
