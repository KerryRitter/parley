use std::process::{Command, Stdio};

use crate::harness::Invocation;

pub(crate) fn run_invocation(
    invocation: Invocation,
    cwd: Option<&str>,
    inherit_stdin: bool,
) -> Result<(), String> {
    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .envs(&invocation.env)
        .stdin(if inherit_stdin {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;

    std::process::exit(status.code().unwrap_or(1));
}
