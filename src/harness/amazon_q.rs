use super::{add_passthrough, add_yolo_args, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AmazonQHarness)
}

struct AmazonQHarness;

impl Harness for AmazonQHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["chat".to_string()];

        if let Some(agent) = &request.agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }

        args = add_yolo_args(args, request)?;
        args.push(request.prompt.clone());

        Ok(Invocation::new("q", add_passthrough(args, request)))
    }
}
