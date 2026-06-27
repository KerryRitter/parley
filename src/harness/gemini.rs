use super::{
    add_passthrough, add_yolo_args, plain_model, resume_is_latest, Harness, Invocation, Request,
};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(GeminiHarness)
}

struct GeminiHarness;

impl Harness for GeminiHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = Vec::new();
        if let Some(prompt) = &request.prompt {
            args.extend(["--prompt".to_string(), prompt.clone()]);
        }

        // Resume a prior session for warm context: `--resume <id>` or `latest`.
        if request.prompt.is_some() {
            if let Some(resume) = &request.resume_id {
                let value = if resume_is_latest(resume) {
                    "latest".to_string()
                } else {
                    resume.clone()
                };
                args.extend(["--resume".to_string(), value]);
            }
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
        Ok(Invocation::new("gemini", add_passthrough(args, request)))
    }
}
