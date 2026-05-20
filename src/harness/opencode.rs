use super::{
    add_passthrough, add_yolo_args, is_json_output, provider_qualified_model, Harness, Invocation,
    Request,
};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(OpenCodeHarness)
}

struct OpenCodeHarness;

impl Harness for OpenCodeHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if request.prompt.is_some() {
            args.push("run".to_string());
        }

        if let Some(model) = provider_qualified_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(agent) = &request.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }
        if request.prompt.is_some() && is_json_output(request) {
            args.extend(["--format".to_string(), "json".to_string()]);
        }

        if request.prompt.is_some() {
            args = add_yolo_args(args, request)?;
        }
        if let Some(prompt) = &request.prompt {
            args.push(prompt.clone());
        }

        Ok(Invocation::new("opencode", add_passthrough(args, request)))
    }
}
