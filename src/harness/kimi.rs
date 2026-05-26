use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(KimiHarness)
}

struct KimiHarness;

impl Harness for KimiHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if let Some(prompt) = &request.prompt {
            args.extend(["-p".to_string(), prompt.clone()]);
        }

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if request.prompt.is_some() {
            if let Some(format) = &request.output_format {
                args.extend(["--output-format".to_string(), format.clone()]);
            }
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("kimi", add_passthrough(args, request)))
    }
}
