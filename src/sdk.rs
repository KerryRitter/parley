//! Stable, process-safe API used by language SDKs and native Rust callers.
//!
//! The process protocol is newline-delimited JSON. `par sdk run` reads one
//! request from stdin, keeps stdin open for control messages, and writes only
//! structured events to stdout. Secrets in `RuntimeRequest::env` are therefore
//! never placed in argv or echoed by the protocol.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::harness::{known_harnesses, normalize_harness, HarnessFactory, Request};
use crate::json::Json;

/// Version of the JSONL request/event protocol exposed by `par sdk`.
pub const SDK_PROTOCOL_VERSION: u32 = 1;

const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEFAULT_MAX_CAPTURE_CHARS: usize = 80_000;

/// A complete, harness-neutral agent request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRequest {
    pub harness: String,
    pub prompt: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub output_format: Option<String>,
    pub input_format: Option<String>,
    pub permission_mode: Option<String>,
    pub max_turns: Option<String>,
    pub session_id: Option<String>,
    pub resume_id: Option<String>,
    pub passthrough: Vec<String>,
    /// Permission bypass is deliberately opt-in on the SDK surface.
    pub yolo: bool,
    /// Override the adapter's normal binary (for example an exo/qwenp wrapper).
    pub executable: Option<String>,
    /// Values applied after the adapter's own environment settings.
    pub env: BTreeMap<String, String>,
    /// Variables removed after adapter defaults. Explicit `env` values win.
    pub unset_env: Vec<String>,
    pub inherit_env: bool,
    pub overall_timeout: Duration,
    pub idle_timeout: Duration,
    /// Maximum bytes retained in each of `RunResult::output` and `stderr`.
    /// Streaming events are not truncated.
    pub max_capture_chars: usize,
}

impl Default for RuntimeRequest {
    fn default() -> Self {
        Self {
            harness: "claude".to_string(),
            prompt: String::new(),
            provider: None,
            model: None,
            agent: None,
            cwd: None,
            output_format: None,
            input_format: None,
            permission_mode: None,
            max_turns: None,
            session_id: None,
            resume_id: None,
            passthrough: Vec::new(),
            yolo: false,
            executable: None,
            env: BTreeMap::new(),
            unset_env: Vec::new(),
            inherit_env: true,
            overall_timeout: DEFAULT_OVERALL_TIMEOUT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            max_capture_chars: DEFAULT_MAX_CAPTURE_CHARS,
        }
    }
}

impl RuntimeRequest {
    fn into_harness_request(self) -> Request {
        Request {
            harness: normalize_harness(&self.harness),
            provider: self.provider,
            model: self.model,
            output_format: self.output_format,
            input_format: self.input_format,
            permission_mode: self.permission_mode,
            max_turns: self.max_turns,
            agent: self.agent,
            cwd: self.cwd,
            prompt: Some(self.prompt),
            passthrough: self.passthrough,
            dry_run: false,
            yolo: self.yolo,
            session_id: self.session_id,
            resume_id: self.resume_id,
            executable: self.executable,
            env: self.env,
            unset_env: self.unset_env,
            inherit_env: self.inherit_env,
        }
    }

    fn from_json(value: &Json) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "SDK request must be a JSON object".to_string())?;
        let version = optional_u64(object, "protocolVersion")?
            .ok_or_else(|| "missing required integer field protocolVersion".to_string())?;
        if version != SDK_PROTOCOL_VERSION as u64 {
            return Err(format!(
                "unsupported SDK protocol version {version}; this binary supports {SDK_PROTOCOL_VERSION}"
            ));
        }

        let harness = required_string(object, "harness")?;
        let prompt = required_string(object, "prompt")?;
        let mut request = Self {
            harness,
            prompt,
            provider: optional_string(object, "provider")?,
            model: optional_string(object, "model")?,
            agent: optional_string(object, "agent")?,
            cwd: optional_string(object, "cwd")?,
            output_format: optional_string(object, "outputFormat")?,
            input_format: optional_string(object, "inputFormat")?,
            permission_mode: optional_string(object, "permissionMode")?,
            max_turns: optional_string_or_number(object, "maxTurns")?,
            session_id: optional_string(object, "sessionId")?,
            resume_id: optional_string(object, "resumeId")?,
            passthrough: optional_string_array(object, "passthrough")?,
            yolo: optional_bool(object, "yolo")?.unwrap_or(false),
            executable: optional_string(object, "executable")?,
            env: optional_string_map(object, "env")?,
            unset_env: optional_string_array(object, "unsetEnv")?,
            inherit_env: optional_bool(object, "inheritEnv")?.unwrap_or(true),
            overall_timeout: DEFAULT_OVERALL_TIMEOUT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            max_capture_chars: optional_u64(object, "maxCaptureChars")?
                .map(usize::try_from)
                .transpose()
                .map_err(|_| "maxCaptureChars is too large".to_string())?
                .unwrap_or(DEFAULT_MAX_CAPTURE_CHARS),
        };

        if let Some(timeouts) = object.get("timeout") {
            let timeouts = timeouts
                .as_object()
                .ok_or_else(|| "timeout must be an object".to_string())?;
            if let Some(ms) = optional_u64(timeouts, "overallMs")? {
                request.overall_timeout = Duration::from_millis(ms);
            }
            if let Some(ms) = optional_u64(timeouts, "idleMs")? {
                request.idle_timeout = Duration::from_millis(ms);
            }
        }

        validate_request(&request)?;
        Ok(request)
    }
}

/// Runtime controls can be retained by another thread and triggered at any time.
#[derive(Clone, Debug, Default)]
pub struct RunControl {
    cancelled: Arc<AtomicBool>,
}

impl RunControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminationReason {
    Cancelled,
    OverallTimeout,
    IdleTimeout,
}

impl TerminationReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::OverallTimeout => "overall_timeout",
            Self::IdleTimeout => "idle_timeout",
        }
    }
}

impl RunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub status: RunStatus,
    pub termination_reason: Option<TerminationReason>,
    pub exit_code: Option<i32>,
    pub output: String,
    pub stderr: String,
    pub duration: Duration,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunEvent {
    Started { harness: String, pid: u32 },
    Stdout { sequence: u64, data: String },
    Stderr { sequence: u64, data: String },
    Completed { result: RunResult },
}

/// Adapter features callers can use without knowing individual CLI syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessCapabilities {
    pub harness: String,
    pub headless: bool,
    pub model: bool,
    pub provider: bool,
    pub agent: bool,
    pub structured_output: bool,
    pub input_format: bool,
    pub permission_mode: bool,
    pub max_turns: bool,
    pub session_id: bool,
    pub resume: bool,
    pub yolo: bool,
    pub passthrough: bool,
    pub executable_override: bool,
    pub environment_override: bool,
    pub cancellation: bool,
    pub live_steering: bool,
}

/// Report the normalized capability set for one harness.
pub fn capabilities(harness: &str) -> Result<HarnessCapabilities, String> {
    let name = normalize_harness(harness);
    HarnessFactory::default().create(&name)?;

    let (model, provider, agent, structured, input, permission, turns, session, resume) =
        match name.as_str() {
            "claude" => (true, false, false, true, true, true, true, true, true),
            "codex" => (true, true, false, true, false, false, false, false, true),
            "cursor" => (true, false, false, true, false, false, false, false, false),
            "gemini" => (true, false, false, true, false, false, false, false, true),
            "goose" => (true, true, true, false, false, true, true, false, false),
            "opencode" => (true, true, true, true, false, false, false, false, false),
            "qwen" => (true, false, false, true, false, false, false, false, false),
            "aider" => (true, true, false, false, false, false, false, false, false),
            "amazon-q" => (false, false, true, false, false, false, false, false, false),
            "copilot" => (true, false, true, false, false, false, false, false, false),
            "kimi" => (true, false, false, true, false, false, false, false, false),
            "antigravity" => (true, false, false, false, false, false, false, false, false),
            "muse" => (true, false, false, true, false, true, true, true, true),
            "pi" => (true, true, false, true, false, false, false, true, true),
            "fuse" => (
                false, false, false, false, false, false, false, false, false,
            ),
            _ => unreachable!("factory accepted an unlisted harness"),
        };

    Ok(HarnessCapabilities {
        harness: name.clone(),
        headless: true,
        model,
        provider,
        agent,
        structured_output: structured,
        input_format: input,
        permission_mode: permission,
        max_turns: turns,
        session_id: session,
        resume,
        yolo: !matches!(name.as_str(), "amazon-q" | "pi"),
        passthrough: true,
        executable_override: true,
        environment_override: true,
        cancellation: true,
        live_steering: false,
    })
}

pub fn known_capabilities() -> Vec<HarnessCapabilities> {
    let mut names = known_harnesses();
    names.push("fuse");
    names
        .into_iter()
        .filter_map(|name| capabilities(name).ok())
        .collect()
}

/// Run one request synchronously while delivering output events as they arrive.
/// Use `RunControl` from another thread for cooperative cancellation.
pub fn run<F>(
    request: RuntimeRequest,
    control: RunControl,
    mut on_event: F,
) -> Result<RunResult, String>
where
    F: FnMut(RunEvent),
{
    validate_request(&request)?;
    let started = Instant::now();
    let overall_timeout = request.overall_timeout;
    let idle_timeout = request.idle_timeout;
    let max_capture_chars = request.max_capture_chars;
    let harness_name = normalize_harness(&request.harness);
    let session_id = request
        .session_id
        .clone()
        .or_else(|| request.resume_id.clone().filter(|id| !is_latest(id)));
    let cwd = request.cwd.clone();
    let invocation = HarnessFactory::default().build(&request.into_harness_request())?;

    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if invocation.clear_env {
        command.env_clear();
    }
    for key in &invocation.env_remove {
        command.env_remove(key);
    }
    command.envs(&invocation.env);
    if let Some(cwd) = cwd.as_deref() {
        command.current_dir(cwd);
    }
    // A separate process group lets cancellation stop wrappers and the agent
    // subprocesses they launch, rather than orphaning grandchildren.
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", invocation.command))?;
    let pid = child.id();
    on_event(RunEvent::Started {
        harness: harness_name,
        pid,
    });

    let (chunk_tx, chunk_rx) = mpsc::channel();
    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| spawn_stream_reader(pipe, Stream::Stdout, chunk_tx.clone()));
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| spawn_stream_reader(pipe, Stream::Stderr, chunk_tx.clone()));
    drop(chunk_tx);

    let mut output = TailBuffer::new(max_capture_chars);
    let mut stderr = TailBuffer::new(max_capture_chars);
    let mut stdout_utf8 = Utf8Buffer::default();
    let mut stderr_utf8 = Utf8Buffer::default();
    let mut sequence = 0u64;
    let mut last_activity = Instant::now();
    let mut forced_status = None;
    let mut termination_reason = None;
    let mut exit_status: Option<ExitStatus> = None;
    let mut streams_closed = false;

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed waiting for child: {error}"))?
        {
            exit_status = Some(status);
            break;
        }
        if control.is_cancelled() {
            forced_status = Some(RunStatus::Cancelled);
            termination_reason = Some(TerminationReason::Cancelled);
            terminate_process_tree(&mut child);
            break;
        }
        if !overall_timeout.is_zero() && started.elapsed() >= overall_timeout {
            forced_status = Some(RunStatus::TimedOut);
            termination_reason = Some(TerminationReason::OverallTimeout);
            terminate_process_tree(&mut child);
            break;
        }
        let received = if streams_closed {
            thread::sleep(Duration::from_millis(50));
            Err(mpsc::RecvTimeoutError::Timeout)
        } else {
            chunk_rx.recv_timeout(Duration::from_millis(50))
        };
        match received {
            Ok(chunk) => {
                last_activity = Instant::now();
                emit_chunk(
                    chunk,
                    &mut sequence,
                    &mut output,
                    &mut stderr,
                    &mut stdout_utf8,
                    &mut stderr_utf8,
                    &mut on_event,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !idle_timeout.is_zero() && last_activity.elapsed() >= idle_timeout {
                    forced_status = Some(RunStatus::TimedOut);
                    termination_reason = Some(TerminationReason::IdleTimeout);
                    terminate_process_tree(&mut child);
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                streams_closed = true;
            }
        }
    }

    if exit_status.is_none() {
        exit_status = child.wait().ok();
    }
    if let Some(handle) = stdout_reader {
        let _ = handle.join();
    }
    if let Some(handle) = stderr_reader {
        let _ = handle.join();
    }
    while let Ok(chunk) = chunk_rx.try_recv() {
        emit_chunk(
            chunk,
            &mut sequence,
            &mut output,
            &mut stderr,
            &mut stdout_utf8,
            &mut stderr_utf8,
            &mut on_event,
        );
    }
    flush_utf8(
        Stream::Stdout,
        &mut stdout_utf8,
        &mut sequence,
        &mut on_event,
    );
    flush_utf8(
        Stream::Stderr,
        &mut stderr_utf8,
        &mut sequence,
        &mut on_event,
    );

    let status = forced_status.unwrap_or_else(|| {
        if exit_status.as_ref().is_some_and(ExitStatus::success) {
            RunStatus::Completed
        } else {
            RunStatus::Failed
        }
    });
    let result = RunResult {
        status,
        termination_reason,
        exit_code: exit_status.and_then(|status| status.code()),
        output: output.into_string(),
        stderr: stderr.into_string(),
        duration: started.elapsed(),
        session_id,
    };
    on_event(RunEvent::Completed {
        result: result.clone(),
    });
    Ok(result)
}

/// Entrypoint for the language-neutral JSONL transport.
pub(crate) fn run_cli(arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("capabilities") => run_capabilities_cli(&arguments[1..]),
        Some("run") => run_protocol_cli(&arguments[1..]),
        Some("--help" | "-h") | None => {
            println!("{}", sdk_usage());
            Ok(())
        }
        Some(other) => Err(format!("unknown sdk command: {other}")),
    }
}

fn run_capabilities_cli(arguments: &[String]) -> Result<(), String> {
    let harness = match arguments {
        [] => None,
        [flag, value] if flag == "--harness" || flag == "-h" => Some(value.as_str()),
        [single] if single.starts_with("--harness=") => single.split_once('=').map(|(_, v)| v),
        _ => return Err("usage: par sdk capabilities [--harness <name>]".to_string()),
    };
    let value = if let Some(harness) = harness {
        capabilities_json(&capabilities(harness)?)
    } else {
        json_object(vec![
            ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
            (
                "harnesses",
                Json::Array(known_capabilities().iter().map(capabilities_json).collect()),
            ),
        ])
    };
    println!("{}", value.to_compact_string());
    Ok(())
}

fn run_protocol_cli(arguments: &[String]) -> Result<(), String> {
    if !arguments.is_empty() {
        return Err("usage: par sdk run (request JSON is read from stdin)".to_string());
    }

    let (line_tx, line_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if line_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let first = line_rx
        .recv()
        .map_err(|_| "expected one JSON request on stdin".to_string())?;
    let request = RuntimeRequest::from_json(&Json::parse(&first)?)?;
    let control = RunControl::new();
    let listener_control = control.clone();
    thread::spawn(move || {
        for line in line_rx {
            let Ok(value) = Json::parse(&line) else {
                continue;
            };
            if value.get("type").and_then(Json::as_str) == Some("cancel") {
                listener_control.cancel();
            }
        }
    });

    let stdout = io::stdout();
    let mut output = stdout.lock();
    run(request, control, |event| {
        let line = event_json(&event).to_compact_string();
        let _ = writeln!(output, "{line}");
        let _ = output.flush();
    })?;
    Ok(())
}

fn sdk_usage() -> &'static str {
    "Usage:\n  par sdk capabilities [--harness <name>]\n  par sdk run  # JSONL request/control on stdin; events on stdout"
}

fn validate_request(request: &RuntimeRequest) -> Result<(), String> {
    if request.harness.trim().is_empty() {
        return Err("harness cannot be empty".to_string());
    }
    if request.prompt.is_empty() {
        return Err("prompt cannot be empty for an SDK run".to_string());
    }
    if request
        .executable
        .as_ref()
        .is_some_and(|value| value.is_empty())
    {
        return Err("executable cannot be empty".to_string());
    }
    for key in request.env.keys().chain(request.unset_env.iter()) {
        validate_env_name(key)?;
    }
    HarnessFactory::default()
        .create(&normalize_harness(&request.harness))
        .map(|_| ())
}

fn validate_env_name(key: &str) -> Result<(), String> {
    if key.is_empty() || key.contains('=') || key.contains('\0') {
        return Err(format!("invalid environment variable name: {key:?}"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

struct StreamChunk {
    stream: Stream,
    bytes: Vec<u8>,
}

fn spawn_stream_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    sender: mpsc::Sender<StreamChunk>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if sender
                        .send(StreamChunk {
                            stream,
                            bytes: buffer[..count].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

fn emit_chunk<F>(
    chunk: StreamChunk,
    sequence: &mut u64,
    output: &mut TailBuffer,
    stderr: &mut TailBuffer,
    stdout_utf8: &mut Utf8Buffer,
    stderr_utf8: &mut Utf8Buffer,
    on_event: &mut F,
) where
    F: FnMut(RunEvent),
{
    match chunk.stream {
        Stream::Stdout => {
            output.push(&chunk.bytes);
            if let Some(data) = stdout_utf8.push(&chunk.bytes) {
                *sequence += 1;
                on_event(RunEvent::Stdout {
                    sequence: *sequence,
                    data,
                });
            }
        }
        Stream::Stderr => {
            stderr.push(&chunk.bytes);
            if let Some(data) = stderr_utf8.push(&chunk.bytes) {
                *sequence += 1;
                on_event(RunEvent::Stderr {
                    sequence: *sequence,
                    data,
                });
            }
        }
    }
}

#[derive(Default)]
struct Utf8Buffer {
    pending: Vec<u8>,
}

impl Utf8Buffer {
    fn push(&mut self, bytes: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(bytes);
        match std::str::from_utf8(&self.pending) {
            Ok(value) => {
                let value = value.to_string();
                self.pending.clear();
                (!value.is_empty()).then_some(value)
            }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                if valid == 0 {
                    return None;
                }
                let suffix = self.pending.split_off(valid);
                let value = String::from_utf8(self.pending.clone()).ok();
                self.pending = suffix;
                value
            }
            Err(_) => {
                let value = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                Some(value)
            }
        }
    }

    fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(std::mem::take(&mut self.pending).as_slice()).into_owned())
        }
    }
}

fn flush_utf8<F>(stream: Stream, buffer: &mut Utf8Buffer, sequence: &mut u64, on_event: &mut F)
where
    F: FnMut(RunEvent),
{
    let Some(data) = buffer.finish() else {
        return;
    };
    *sequence += 1;
    match stream {
        Stream::Stdout => on_event(RunEvent::Stdout {
            sequence: *sequence,
            data,
        }),
        Stream::Stderr => on_event(RunEvent::Stderr {
            sequence: *sequence,
            data,
        }),
    }
}

struct TailBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl TailBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.limit == 0 {
            return;
        }
        if bytes.len() >= self.limit {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&bytes[bytes.len() - self.limit..]);
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(self.limit);
        if overflow > 0 {
            self.bytes.drain(..overflow);
        }
        self.bytes.extend_from_slice(bytes);
    }

    fn into_string(self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as i32);
        // SAFETY: `kill` is called with the child's process-group id and a
        // standard signal number. The child was placed in this group above.
        unsafe {
            unix_kill(process_group, 15);
        }
        let deadline = Instant::now() + Duration::from_millis(750);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        // SAFETY: same process group as above; SIGKILL ensures descendants do
        // not survive a cancelled or timed-out SDK job.
        unsafe {
            unix_kill(process_group, 9);
        }
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = child.kill();
    }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn unix_kill(pid: i32, signal: i32) -> i32;
}

fn is_latest(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "latest" | "last")
}

fn required_string(object: &BTreeMap<String, Json>, key: &str) -> Result<String, String> {
    optional_string(object, key)?.ok_or_else(|| format!("missing required string field {key}"))
}

fn optional_string(object: &BTreeMap<String, Json>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn optional_string_or_number(
    object: &BTreeMap<String, Json>,
    key: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(value)) => Ok(Some(value.clone())),
        Some(Json::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            Ok(Some((*value as i64).to_string()))
        }
        Some(_) => Err(format!("{key} must be a string or integer")),
    }
}

fn optional_bool(object: &BTreeMap<String, Json>, key: &str) -> Result<Option<bool>, String> {
    match object.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

fn optional_u64(object: &BTreeMap<String, Json>, key: &str) -> Result<Option<u64>, String> {
    match object.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Number(value))
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= u64::MAX as f64 =>
        {
            Ok(Some(*value as u64))
        }
        Some(_) => Err(format!("{key} must be a non-negative integer")),
    }
}

fn optional_string_array(
    object: &BTreeMap<String, Json>,
    key: &str,
) -> Result<Vec<String>, String> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| format!("{key} must be an array of strings"))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} must contain only strings"))
        })
        .collect()
}

fn optional_string_map(
    object: &BTreeMap<String, Json>,
    key: &str,
) -> Result<BTreeMap<String, String>, String> {
    let Some(value) = object.get(key) else {
        return Ok(BTreeMap::new());
    };
    let map = value
        .as_object()
        .ok_or_else(|| format!("{key} must be an object of string values"))?;
    map.iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| "env values must be strings".to_string())
        })
        .collect()
}

fn event_json(event: &RunEvent) -> Json {
    match event {
        RunEvent::Started { harness, pid } => json_object(vec![
            ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
            ("type", Json::Str("started".to_string())),
            ("harness", Json::Str(harness.clone())),
            ("pid", Json::Number(*pid as f64)),
        ]),
        RunEvent::Stdout { sequence, data } => json_object(vec![
            ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
            ("type", Json::Str("stdout".to_string())),
            ("sequence", Json::Number(*sequence as f64)),
            ("data", Json::Str(data.clone())),
        ]),
        RunEvent::Stderr { sequence, data } => json_object(vec![
            ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
            ("type", Json::Str("stderr".to_string())),
            ("sequence", Json::Number(*sequence as f64)),
            ("data", Json::Str(data.clone())),
        ]),
        RunEvent::Completed { result } => json_object(vec![
            ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
            ("type", Json::Str("completed".to_string())),
            ("result", result_json(result)),
        ]),
    }
}

fn result_json(result: &RunResult) -> Json {
    json_object(vec![
        ("status", Json::Str(result.status.as_str().to_string())),
        (
            "terminationReason",
            result
                .termination_reason
                .map(|reason| Json::Str(reason.as_str().to_string()))
                .unwrap_or(Json::Null),
        ),
        (
            "exitCode",
            result
                .exit_code
                .map(|code| Json::Number(code as f64))
                .unwrap_or(Json::Null),
        ),
        ("output", Json::Str(result.output.clone())),
        ("stderr", Json::Str(result.stderr.clone())),
        (
            "durationMs",
            Json::Number(result.duration.as_millis() as f64),
        ),
        (
            "sessionId",
            result
                .session_id
                .clone()
                .map(Json::Str)
                .unwrap_or(Json::Null),
        ),
    ])
}

fn capabilities_json(value: &HarnessCapabilities) -> Json {
    json_object(vec![
        ("protocolVersion", Json::Number(SDK_PROTOCOL_VERSION as f64)),
        ("harness", Json::Str(value.harness.clone())),
        ("headless", Json::Bool(value.headless)),
        ("model", Json::Bool(value.model)),
        ("provider", Json::Bool(value.provider)),
        ("agent", Json::Bool(value.agent)),
        ("structuredOutput", Json::Bool(value.structured_output)),
        ("inputFormat", Json::Bool(value.input_format)),
        ("permissionMode", Json::Bool(value.permission_mode)),
        ("maxTurns", Json::Bool(value.max_turns)),
        ("sessionId", Json::Bool(value.session_id)),
        ("resume", Json::Bool(value.resume)),
        ("yolo", Json::Bool(value.yolo)),
        ("passthrough", Json::Bool(value.passthrough)),
        ("executableOverride", Json::Bool(value.executable_override)),
        (
            "environmentOverride",
            Json::Bool(value.environment_override),
        ),
        ("cancellation", Json::Bool(value.cancellation)),
        ("liveSteering", Json::Bool(value.live_steering)),
    ])
}

fn json_object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_are_safe() {
        let request = RuntimeRequest::from_json(
            &Json::parse(r#"{"protocolVersion":1,"harness":"codex","prompt":"hello"}"#).unwrap(),
        )
        .unwrap();
        assert!(!request.yolo);
        assert!(request.inherit_env);
        assert_eq!(request.overall_timeout, DEFAULT_OVERALL_TIMEOUT);
    }

    #[test]
    fn request_requires_a_supported_protocol_version() {
        let missing = Json::parse(r#"{"harness":"codex","prompt":"hello"}"#).unwrap();
        assert!(RuntimeRequest::from_json(&missing)
            .unwrap_err()
            .contains("protocolVersion"));

        let future =
            Json::parse(r#"{"protocolVersion":2,"harness":"codex","prompt":"hello"}"#).unwrap();
        assert!(RuntimeRequest::from_json(&future)
            .unwrap_err()
            .contains("unsupported SDK protocol"));
    }

    #[test]
    fn parses_environment_and_timeouts() {
        let request = RuntimeRequest::from_json(
            &Json::parse(
                r#"{"protocolVersion":1,"harness":"qwen","prompt":"hello","executable":"qwenp","env":{"OPENAI_BASE_URL":"http://exo","OPENAI_API_KEY":"test"},"unsetEnv":["OLD_KEY"],"inheritEnv":false,"timeout":{"overallMs":12,"idleMs":3}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(request.executable.as_deref(), Some("qwenp"));
        assert_eq!(request.env["OPENAI_BASE_URL"], "http://exo");
        assert!(!request.inherit_env);
        assert_eq!(request.overall_timeout, Duration::from_millis(12));
    }

    #[test]
    fn tail_buffer_retains_only_the_end() {
        let mut buffer = TailBuffer::new(5);
        buffer.push(b"abc");
        buffer.push(b"defg");
        assert_eq!(buffer.into_string(), "cdefg");
    }

    #[test]
    fn utf8_buffer_preserves_codepoints_split_between_reads() {
        let mut buffer = Utf8Buffer::default();
        let bytes = "aéb".as_bytes();
        assert_eq!(buffer.push(&bytes[..2]).as_deref(), Some("a"));
        assert_eq!(buffer.push(&bytes[2..]).as_deref(), Some("éb"));
        assert!(buffer.finish().is_none());
    }

    #[test]
    fn capabilities_include_runtime_controls() {
        let codex = capabilities("co").unwrap();
        assert_eq!(codex.harness, "codex");
        assert!(codex.environment_override);
        assert!(codex.executable_override);
        assert!(codex.cancellation);
        assert!(!codex.live_steering);
    }
}
