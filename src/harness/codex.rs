use super::{add_passthrough, is_json_output, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(CodexHarness)
}

struct CodexHarness;

impl Harness for CodexHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["exec".to_string()];

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if is_json_output(request) {
            args.push("--json".to_string());
        }

        args.push(request.prompt.clone());

        let mut invocation = Invocation::new("codex", add_passthrough(args, request));
        if let Some(provider) = &request.provider {
            invocation = invocation.with_env("AGENT_ROUTER_PROVIDER", provider);
        }
        Ok(invocation)
    }
}
