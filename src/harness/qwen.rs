use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(QwenHarness)
}

struct QwenHarness;

impl Harness for QwenHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["-p".to_string(), request.prompt.clone()];

        if let Some(model) = plain_model(request) {
            args.extend(["--model".to_string(), model]);
        }
        if let Some(format) = &request.output_format {
            args.extend(["--output-format".to_string(), format.clone()]);
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("qwen", add_passthrough(args, request)))
    }
}
