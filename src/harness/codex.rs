use super::{
    add_passthrough, add_yolo_args, is_json_output, plain_model, Harness, Invocation, Request,
};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(CodexHarness)
}

struct CodexHarness;

impl Harness for CodexHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if request.prompt.is_some() {
            args.push("exec".to_string());
        }

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if request.prompt.is_some() && is_json_output(request) {
            args.push("--json".to_string());
        }
        if request.prompt.is_some() && request.yolo {
            args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        } else {
            args = add_yolo_args(args, request)?;
        }

        if let Some(prompt) = &request.prompt {
            args.push(prompt.clone());
        }

        let mut invocation = Invocation::new("codex", add_passthrough(args, request));
        if let Some(provider) = &request.provider {
            invocation = invocation.with_env("AGENT_ROUTER_PROVIDER", provider);
        }
        Ok(invocation)
    }
}
