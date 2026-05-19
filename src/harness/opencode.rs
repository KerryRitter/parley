use super::{
    add_passthrough, is_json_output, provider_qualified_model, Harness, Invocation, Request,
};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(OpenCodeHarness)
}

struct OpenCodeHarness;

impl Harness for OpenCodeHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["run".to_string()];

        if let Some(model) = provider_qualified_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(agent) = &request.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }
        if is_json_output(request) {
            args.extend(["--format".to_string(), "json".to_string()]);
        }

        args.push(request.prompt.clone());

        Ok(Invocation::new("opencode", add_passthrough(args, request)))
    }
}
