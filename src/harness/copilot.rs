use super::{add_passthrough, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(CopilotHarness)
}

struct CopilotHarness;

impl Harness for CopilotHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["-p".to_string(), request.prompt.clone()];

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(agent) = &request.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }

        Ok(Invocation::new("copilot", add_passthrough(args, request)))
    }
}
