use super::{add_passthrough, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(GooseHarness)
}

struct GooseHarness;

impl Harness for GooseHarness {
    fn build(&self, request: &Request) -> Result<Invocation, String> {
        let mut args = vec!["run".to_string()];

        if let Some(agent) = &request.agent {
            args.extend(["--with-builtin".to_string(), agent.clone()]);
        }

        args.extend(["-t".to_string(), request.prompt.clone()]);

        let mut invocation = Invocation::new("goose", add_passthrough(args, request));

        if let Some(provider) = &request.provider {
            invocation = invocation.with_env("GOOSE_PROVIDER", provider);
        }
        if let Some(model) = plain_model(request) {
            invocation = invocation.with_env("GOOSE_MODEL", model);
        }
        if let Some(mode) = &request.permission_mode {
            invocation = invocation.with_env("GOOSE_MODE", mode);
        }
        if let Some(max_turns) = &request.max_turns {
            invocation = invocation.with_env("GOOSE_MAX_TURNS", max_turns);
        }

        Ok(invocation)
    }
}
