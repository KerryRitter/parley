use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::thread;

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

/// Output captured from a child process (used by `par ask` / the MCP
/// `ask_agent` tool, where one agent's reply is the value, not a stream).
pub(crate) struct Captured {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Run an invocation to completion, capturing stdout/stderr instead of
/// inheriting them. stdin is closed so the child cannot block on a prompt.
pub(crate) fn capture_invocation(
    invocation: Invocation,
    cwd: Option<&str>,
) -> Result<Captured, String> {
    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .envs(&invocation.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;

    Ok(Captured {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    })
}

/// Like [`capture_invocation`], but invokes `on_stdout_line` for each line of
/// stdout as it arrives, so a caller can surface live progress while still
/// getting the full captured output at the end. stderr is drained on a separate
/// thread so a chatty child can't deadlock by filling its stderr pipe while we
/// block reading stdout.
pub(crate) fn capture_streaming(
    invocation: Invocation,
    cwd: Option<&str>,
    mut on_stdout_line: impl FnMut(&str),
) -> Result<Captured, String> {
    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .envs(&invocation.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;

    let stdout = child.stdout.take().ok_or("failed to capture stdout")?;
    let mut stderr = child.stderr.take().ok_or("failed to capture stderr")?;

    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    let mut stdout_text = String::new();
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) => {
                on_stdout_line(&line);
                stdout_text.push_str(&line);
                stdout_text.push('\n');
            }
            Err(_) => break,
        }
    }

    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for {}: {error}", invocation.command))?;
    let stderr_text = stderr_handle.join().unwrap_or_default();

    Ok(Captured {
        stdout: stdout_text,
        stderr: stderr_text,
        success: status.success(),
    })
}
