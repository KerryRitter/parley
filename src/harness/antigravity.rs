use super::{add_passthrough, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AntigravityHarness)
}

struct AntigravityHarness;

impl Harness for AntigravityHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let args = vec![request.prompt.clone()];

        Ok(Invocation::new("agy", add_passthrough(args, request)))
    }
}
