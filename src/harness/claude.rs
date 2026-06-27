use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(ClaudeHarness)
}

struct ClaudeHarness;

impl Harness for ClaudeHarness {
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
            if let Some(format) = &request.input_format {
                args.extend(["--input-format".to_string(), format.clone()]);
            }
        }
        if let Some(mode) = &request.permission_mode {
            args.extend(["--permission-mode".to_string(), mode.clone()]);
        }
        if let Some(max_turns) = &request.max_turns {
            args.extend(["--max-turns".to_string(), max_turns.clone()]);
        }

        // Session continuity (print mode only). Resume an existing session for a
        // warm prompt cache, or set a specific id so the caller can resume later.
        // Resume wins if both are given.
        if request.prompt.is_some() {
            if let Some(resume) = &request.resume_id {
                args.extend(["--resume".to_string(), resume.clone()]);
            } else if let Some(session) = &request.session_id {
                args.extend(["--session-id".to_string(), session.clone()]);
            }
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("claude", add_passthrough(args, request)))
    }
}
