use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(CopilotHarness)
}

struct CopilotHarness;

impl Harness for CopilotHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if let Some(prompt) = &request.prompt {
            args.extend(["-p".to_string(), prompt.clone()]);
        }

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(agent) = &request.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("copilot", add_passthrough(args, request)))
    }
}
