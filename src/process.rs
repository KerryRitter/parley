use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::Invocation;

/// Run an invocation with inherited stdio and **replace this process** by
/// exiting with the child's status. Used for interactive launches and `mcp
/// connect` where `par` has nothing more to do once the child finishes.
pub(crate) fn run_invocation(
    invocation: Invocation,
    cwd: Option<&str>,
    inherit_stdin: bool,
) -> Result<(), String> {
    let status = run_invocation_status(invocation, cwd, inherit_stdin)?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Run an invocation with inherited stdio and **return** its exit status instead
/// of exiting, so the caller can do post-run work (e.g. record telemetry)
/// before terminating.
pub(crate) fn run_invocation_status(
    invocation: Invocation,
    cwd: Option<&str>,
    inherit_stdin: bool,
) -> Result<std::process::ExitStatus, String> {
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

    command
        .status()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))
}

/// Output captured from a child process (used by `par ask` / the MCP
/// `ask_agent` tool, where one agent's reply is the value, not a stream).
pub(crate) struct Captured {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    /// True when the watchdog killed the child for exceeding a time budget
    /// (overall or idle/no-output). The captured streams hold whatever arrived
    /// before the kill.
    pub timed_out: bool,
}

/// Run an invocation to completion, capturing stdout/stderr instead of
/// inheriting them. stdin is closed so the child cannot block on a prompt.
pub(crate) fn capture_invocation(
    invocation: Invocation,
    cwd: Option<&str>,
) -> Result<Captured, String> {
    capture_invocation_timeout(invocation, cwd, Timeouts::disabled())
}

/// Time budgets for a captured run. `overall` caps total wall-clock; `idle`
/// caps the gap between bytes of output (a hung agent that emits nothing).
/// Either may be zero to disable that bound.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timeouts {
    pub overall: Duration,
    pub idle: Duration,
}

impl Timeouts {
    pub(crate) fn disabled() -> Self {
        Self {
            overall: Duration::ZERO,
            idle: Duration::ZERO,
        }
    }

    /// Resolve the default budgets for headless captured calls (ask/fuse/solve)
    /// from the environment. `PARLEY_TIMEOUT` and `PARLEY_IDLE_TIMEOUT` are in
    /// seconds; `0` disables that bound. Defaults: 600s overall, 180s idle.
    pub(crate) fn from_env() -> Self {
        let overall = env_secs("PARLEY_TIMEOUT").unwrap_or(600);
        let idle = env_secs("PARLEY_IDLE_TIMEOUT").unwrap_or(180);
        Self {
            overall: Duration::from_secs(overall),
            idle: Duration::from_secs(idle),
        }
    }
}

fn env_secs(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

/// Run an invocation captured, with a watchdog that kills the child if it
/// exceeds the overall budget or stops producing output for longer than the
/// idle budget. Output is streamed into buffers off-thread so a wedged child
/// can't deadlock a full pipe.
pub(crate) fn capture_invocation_timeout(
    invocation: Invocation,
    cwd: Option<&str>,
    timeouts: Timeouts,
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

    // No bounds set: the simple blocking path is enough and avoids the extra
    // threads entirely.
    if timeouts.overall.is_zero() && timeouts.idle.is_zero() {
        let output = command
            .output()
            .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;
        return Ok(Captured {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
            timed_out: false,
        });
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;

    let out_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let err_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (beat_tx, beat_rx) = mpsc::channel::<()>();

    let out_reader = child
        .stdout
        .take()
        .map(|pipe| spawn_reader(pipe, Arc::clone(&out_buf), beat_tx.clone()));
    let err_reader = child
        .stderr
        .take()
        .map(|pipe| spawn_reader(pipe, Arc::clone(&err_buf), beat_tx.clone()));
    // Drop our own sender so the channel disconnects once both readers finish.
    drop(beat_tx);

    let started = Instant::now();
    let mut timed_out = false;
    // Wait at idle granularity (or a short tick when only an overall bound is
    // set), reacting to output heartbeats and process exit.
    let tick = pick_tick(timeouts);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if !timeouts.overall.is_zero() && started.elapsed() >= timeouts.overall {
            let _ = child.kill();
            timed_out = true;
            break;
        }
        match beat_rx.recv_timeout(tick) {
            Ok(()) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Readers closed: output is complete, just reap the child.
                let _ = child.wait();
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !timeouts.idle.is_zero() && started.elapsed() >= timeouts.idle {
                    // Approximation: no heartbeat within the idle window. Good
                    // enough — a steadily-emitting child keeps resetting it.
                    let _ = child.kill();
                    timed_out = true;
                    break;
                }
            }
        }
    }

    let status = child.wait().ok();
    if let Some(handle) = out_reader {
        let _ = handle.join();
    }
    if let Some(handle) = err_reader {
        let _ = handle.join();
    }

    let stdout = String::from_utf8_lossy(&out_buf.lock().unwrap()).into_owned();
    let stderr = String::from_utf8_lossy(&err_buf.lock().unwrap()).into_owned();
    let success = !timed_out && status.map(|s| s.success()).unwrap_or(false);

    Ok(Captured {
        stdout,
        stderr,
        success,
        timed_out,
    })
}

/// How often the watchdog wakes to re-check budgets. Bounded so an overall-only
/// budget is still enforced promptly, and never longer than the idle window.
fn pick_tick(timeouts: Timeouts) -> Duration {
    let mut tick = Duration::from_millis(250);
    if !timeouts.idle.is_zero() && timeouts.idle < tick {
        tick = timeouts.idle;
    }
    tick
}

fn spawn_reader<R: Read + Send + 'static>(
    mut pipe: R,
    buf: Arc<Mutex<Vec<u8>>>,
    beat: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.lock().unwrap().extend_from_slice(&chunk[..n]);
                    // A dead receiver just means the watchdog already moved on.
                    let _ = beat.send(());
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Invocation;

    fn inv(command: &str, args: &[&str]) -> Invocation {
        Invocation::new(command, args.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn captures_stdout_without_timeout() {
        let out = capture_invocation(inv("printf", &["hello"]), None).unwrap();
        assert_eq!(out.stdout, "hello");
        assert!(out.success);
        assert!(!out.timed_out);
    }

    #[test]
    fn overall_timeout_kills_a_hung_child() {
        let timeouts = Timeouts {
            overall: Duration::from_millis(300),
            idle: Duration::ZERO,
        };
        let out = capture_invocation_timeout(inv("sleep", &["10"]), None, timeouts).unwrap();
        assert!(out.timed_out);
        assert!(!out.success);
    }

    #[test]
    fn idle_timeout_kills_a_silent_child() {
        let timeouts = Timeouts {
            overall: Duration::ZERO,
            idle: Duration::from_millis(300),
        };
        let out = capture_invocation_timeout(inv("sleep", &["10"]), None, timeouts).unwrap();
        assert!(out.timed_out);
    }

    #[test]
    fn completes_before_timeout() {
        let timeouts = Timeouts {
            overall: Duration::from_secs(5),
            idle: Duration::from_secs(5),
        };
        let out = capture_invocation_timeout(inv("printf", &["done"]), None, timeouts).unwrap();
        assert_eq!(out.stdout, "done");
        assert!(out.success);
        assert!(!out.timed_out);
    }
}
