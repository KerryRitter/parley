use super::{add_passthrough, add_yolo_args, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(AntigravityHarness)
}

struct AntigravityHarness;

impl Harness for AntigravityHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = add_yolo_args(Vec::new(), request)?;
        args.push(request.prompt.clone());

        Ok(Invocation::new("agy", add_passthrough(args, request)))
    }
}
