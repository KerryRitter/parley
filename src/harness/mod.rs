mod aider;
mod amazon_q;
mod antigravity;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod gemini;
mod goose;
mod invocation;
mod kimi;
mod meta;
mod opencode;
mod qwen;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::cli::CliOptions;
use crate::model::{ModelFactory, ModelFormat};
pub(crate) use invocation::Invocation;

pub(crate) trait Harness {
    fn build(&self, request: &Request) -> Result<Invocation, String>;
}

#[derive(Clone)]
pub(crate) struct Request {
    pub harness: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub output_format: Option<String>,
    pub input_format: Option<String>,
    pub permission_mode: Option<String>,
    pub max_turns: Option<String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub prompt: Option<String>,
    pub passthrough: Vec<String>,
    pub dry_run: bool,
    pub yolo: bool,
    pub session_id: Option<String>,
    pub resume_id: Option<String>,
}

impl Request {
    pub(crate) fn from_options(options: CliOptions, piped_input: String) -> Result<Self, String> {
        let prompt = merge_prompt(&piped_input, options.prompt.as_deref());

        Ok(Self {
            harness: normalize_harness(&options.harness),
            provider: options.provider,
            model: options.model,
            output_format: options.output_format,
            input_format: options.input_format,
            permission_mode: options.permission_mode,
            max_turns: options.max_turns,
            agent: options.agent,
            cwd: options.cwd,
            prompt,
            passthrough: options.passthrough,
            dry_run: options.dry_run,
            yolo: options.yolo,
            session_id: options.session_id,
            resume_id: options.resume_id,
        })
    }
}

/// Whether a `--resume-id` value means "the most recent session here" rather
/// than a specific id. Each adapter maps this to its own flag (`--last` for
/// codex, `latest` for gemini).
pub(crate) fn resume_is_latest(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "latest" | "last"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShimCommand {
    Install,
    List,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShimOptions {
    pub(crate) command: ShimCommand,
    pub(crate) dir: Option<String>,
    pub(crate) dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HarnessSpec {
    pub(crate) name: &'static str,
    pub(crate) binary: &'static str,
    pub(crate) shim: &'static str,
    pub(crate) yolo_args: &'static [&'static str],
}

const HARNESS_SPECS: &[HarnessSpec] = &[
    HarnessSpec {
        name: "claude",
        binary: "claude",
        shim: "claudey",
        yolo_args: &["--dangerously-skip-permissions"],
    },
    HarnessSpec {
        name: "codex",
        binary: "codex",
        shim: "codexy",
        yolo_args: &["--yolo"],
    },
    HarnessSpec {
        name: "cursor",
        binary: "cursor-agent",
        shim: "cursory",
        yolo_args: &["--force"],
    },
    HarnessSpec {
        name: "gemini",
        binary: "gemini",
        shim: "geminiy",
        yolo_args: &["--yolo"],
    },
    HarnessSpec {
        name: "goose",
        binary: "goose",
        shim: "goosey",
        yolo_args: &["auto"],
    },
    HarnessSpec {
        name: "opencode",
        binary: "opencode",
        shim: "opencodey",
        yolo_args: &["--dangerously-skip-permissions"],
    },
    HarnessSpec {
        name: "qwen",
        binary: "qwen",
        shim: "qweny",
        yolo_args: &["--yolo"],
    },
    HarnessSpec {
        name: "aider",
        binary: "aider",
        shim: "aidery",
        yolo_args: &["--yes-always"],
    },
    HarnessSpec {
        name: "amazon-q",
        binary: "q",
        shim: "qy",
        yolo_args: &[],
    },
    HarnessSpec {
        name: "copilot",
        binary: "copilot",
        shim: "copiloty",
        yolo_args: &["--yolo"],
    },
    HarnessSpec {
        name: "kimi",
        binary: "kimi",
        shim: "kimiy",
        yolo_args: &["--yolo"],
    },
    HarnessSpec {
        name: "antigravity",
        binary: "agy",
        shim: "agyy",
        yolo_args: &["--dangerously-skip-permissions"],
    },
];

type HarnessConstructor = fn() -> Box<dyn Harness>;

pub(crate) struct HarnessFactory {
    constructors: HashMap<&'static str, HarnessConstructor>,
}

impl Default for HarnessFactory {
    fn default() -> Self {
        let constructors: HashMap<&'static str, HarnessConstructor> = [
            ("claude", claude::new as HarnessConstructor),
            ("codex", codex::new as HarnessConstructor),
            ("aider", aider::new as HarnessConstructor),
            ("amazon-q", amazon_q::new as HarnessConstructor),
            ("antigravity", antigravity::new as HarnessConstructor),
            ("cursor", cursor::new as HarnessConstructor),
            ("gemini", gemini::new as HarnessConstructor),
            ("goose", goose::new as HarnessConstructor),
            ("opencode", opencode::new as HarnessConstructor),
            ("copilot", copilot::new as HarnessConstructor),
            ("kimi", kimi::new as HarnessConstructor),
            ("qwen", qwen::new as HarnessConstructor),
            // Meta-harnesses: harnesses that call back into `par` itself.
            ("auto", meta::auto as HarnessConstructor),
            ("fuse", meta::fuse as HarnessConstructor),
            ("solve", meta::solve as HarnessConstructor),
        ]
        .into_iter()
        .collect();

        Self { constructors }
    }
}

impl HarnessFactory {
    pub(crate) fn create(&self, name: &str) -> Result<Box<dyn Harness>, String> {
        self.constructors
            .get(name)
            .map(|constructor| constructor())
            .ok_or_else(|| {
                let mut names = self.constructors.keys().copied().collect::<Vec<_>>();
                names.sort_unstable();
                format!(
                    "unknown harness \"{name}\". Known harnesses: {}",
                    names.join(", ")
                )
            })
    }
}

pub(crate) fn known_harnesses() -> Vec<&'static str> {
    HARNESS_SPECS.iter().map(|spec| spec.name).collect()
}

/// Is this a meta-harness — one that calls back into `par` rather than driving a
/// single agent CLI (`auto`, `fuse`, `solve`)?
pub(crate) fn is_meta(name: &str) -> bool {
    matches!(normalize_harness(name).as_str(), "auto" | "fuse" | "solve")
}

/// Absolute path to the running `par` binary, so a recursive invocation works
/// regardless of the caller's PATH. Falls back to the bare name.
pub(crate) fn self_bin() -> String {
    env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "par".to_string())
}

pub(crate) fn spec_for_harness(name: &str) -> Option<&'static HarnessSpec> {
    let normalized = normalize_harness(name);
    HARNESS_SPECS.iter().find(|spec| spec.name == normalized)
}

pub(crate) fn add_passthrough(mut args: Vec<String>, request: &Request) -> Vec<String> {
    args.extend(request.passthrough.iter().cloned());
    args
}

pub(crate) fn add_yolo_args(
    mut args: Vec<String>,
    request: &Request,
) -> Result<Vec<String>, String> {
    if !request.yolo {
        return Ok(args);
    }

    let spec = spec_for_harness(&request.harness)
        .ok_or_else(|| format!("unknown harness \"{}\"", request.harness))?;
    // Yolo is on by default; a harness with no known bypass flag simply runs
    // without one rather than failing the whole invocation.
    if spec.yolo_args.is_empty() {
        return Ok(args);
    }
    args.extend(spec.yolo_args.iter().map(|arg| (*arg).to_string()));
    Ok(args)
}

pub(crate) fn is_json_output(request: &Request) -> bool {
    matches!(
        request.output_format.as_deref(),
        Some("json") | Some("stream-json")
    )
}

pub(crate) fn plain_model(request: &Request) -> Option<String> {
    ModelFactory::resolve(request.provider.as_deref(), request.model.as_deref())
        .map(|model| model.format(ModelFormat::Plain))
}

pub(crate) fn provider_qualified_model(request: &Request) -> Option<String> {
    ModelFactory::resolve(request.provider.as_deref(), request.model.as_deref())
        .map(|model| model.format(ModelFormat::ProviderQualified))
}

fn merge_prompt(piped_input: &str, prompt: Option<&str>) -> Option<String> {
    let piped = piped_input.trim_end();
    match (piped.is_empty(), prompt) {
        (false, Some(prompt)) => Some(format!("{piped}\n\n{prompt}")),
        (false, None) => Some(piped.to_string()),
        (true, Some(prompt)) if !prompt.is_empty() => Some(prompt.to_string()),
        _ => None,
    }
}

pub(crate) fn normalize_harness(harness: &str) -> String {
    match harness.to_ascii_lowercase().as_str() {
        // Two-letter shorthands (e.g. `par -h cl`).
        "cl" => "claude".to_string(),
        "co" => "codex".to_string(),
        "cu" => "cursor".to_string(),
        "g" => "gemini".to_string(),
        "go" => "goose".to_string(),
        "oc" => "opencode".to_string(),
        "q" => "qwen".to_string(),
        "k" => "kimi".to_string(),
        "a" | "ai" => "aider".to_string(),
        "aq" => "amazon-q".to_string(),
        "cp" => "copilot".to_string(),
        "ag" => "antigravity".to_string(),
        // Long aliases.
        "cursor-agent" => "cursor".to_string(),
        "open-code" => "opencode".to_string(),
        "google" | "google-gemini" => "gemini".to_string(),
        "openai" => "codex".to_string(),
        "github-copilot" => "copilot".to_string(),
        "amazonq" | "aws-q" | "amazon" => "amazon-q".to_string(),
        "moonshot" | "kimi-code" => "kimi".to_string(),
        "agy" | "google-antigravity" => "antigravity".to_string(),
        value => value.to_string(),
    }
}

pub(crate) fn run_shims(options: ShimOptions) -> Result<(), String> {
    match options.command {
        ShimCommand::List => {
            for spec in HARNESS_SPECS {
                if spec.yolo_args.is_empty() {
                    println!("{:<12} unsupported", spec.shim);
                } else {
                    println!("{:<12} {} \"$@\"", spec.shim, shim_command_preview(spec));
                }
            }
            Ok(())
        }
        ShimCommand::Install => {
            let dir = match options.dir {
                Some(dir) => PathBuf::from(dir),
                None => default_shim_dir()?,
            };
            if !options.dry_run {
                fs::create_dir_all(&dir)
                    .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
            }
            for spec in HARNESS_SPECS {
                if spec.yolo_args.is_empty() {
                    println!("skip: {} has no known yolo flag", spec.name);
                    continue;
                }
                let path = dir.join(spec.shim);
                let script = shim_script(spec);
                if options.dry_run {
                    println!("dry-run: write {}", path.display());
                    continue;
                }
                fs::write(&path, script)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
                #[cfg(unix)]
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .map_err(|error| format!("failed to chmod {}: {error}", path.display()))?;
                println!("installed: {}", path.display());
            }
            Ok(())
        }
    }
}

fn default_shim_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = env::var("PAR_SHIM_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set; cannot find shim dir")?;
    Ok(PathBuf::from(home).join(".local").join("bin"))
}

fn shim_script(spec: &HarnessSpec) -> String {
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nexec {} \"$@\"\n",
        shim_command_preview(spec)
    )
}

fn shim_command_preview(spec: &HarnessSpec) -> String {
    if spec.name == "goose" {
        return format!("env GOOSE_MODE=auto {}", spec.binary);
    }
    if spec.name == "opencode" {
        return format!("{} run {}", spec.binary, spec.yolo_args.join(" "));
    }
    format!("{} {}", spec.binary, spec.yolo_args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliOptions;

    #[test]
    fn combines_piped_input_and_prompt() {
        let request = Request::from_options(
            CliOptions {
                harness: "gemini".to_string(),
                prompt: Some("summarize".to_string()),
                ..CliOptions::default()
            },
            "hello\n".to_string(),
        )
        .unwrap();

        assert_eq!(request.prompt.as_deref(), Some("hello\n\nsummarize"));
    }

    #[test]
    fn normalizes_aliases() {
        assert_eq!(normalize_harness("openai"), "codex");
        assert_eq!(normalize_harness("cursor-agent"), "cursor");
        assert_eq!(normalize_harness("aws-q"), "amazon-q");
        assert_eq!(normalize_harness("agy"), "antigravity");
    }

    #[test]
    fn normalizes_short_harness_codes() {
        assert_eq!(normalize_harness("cl"), "claude");
        assert_eq!(normalize_harness("co"), "codex");
        assert_eq!(normalize_harness("oc"), "opencode");
        assert_eq!(normalize_harness("k"), "kimi");
        assert_eq!(normalize_harness("cu"), "cursor");
        assert_eq!(normalize_harness("aq"), "amazon-q");
    }

    #[test]
    fn allows_missing_prompt_for_interactive_default() {
        let request = Request::from_options(
            CliOptions {
                harness: "claude".to_string(),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "claude");
        assert_eq!(invocation.args, Vec::<String>::new());
    }

    #[test]
    fn factory_builds_opencode_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "opencode".to_string(),
                provider: Some("anthropic".to_string()),
                model: Some("claude-sonnet-4-6".to_string()),
                agent: Some("reviewer".to_string()),
                prompt: Some("review diff".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "opencode");
        assert_eq!(
            invocation.args,
            vec![
                "run",
                "--model",
                "anthropic/claude-sonnet-4-6",
                "--agent",
                "reviewer",
                "review diff",
            ]
        );
    }

    #[test]
    fn factory_builds_qwen_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "qwen".to_string(),
                model: Some("qwen3-coder-plus".to_string()),
                output_format: Some("json".to_string()),
                prompt: Some("review this".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "qwen");
        assert_eq!(
            invocation.args,
            vec![
                "-p",
                "review this",
                "--model",
                "qwen3-coder-plus",
                "--output-format",
                "json",
            ]
        );
    }

    #[test]
    fn factory_builds_claude_yolo_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "claude".to_string(),
                prompt: Some("fix it".to_string()),
                yolo: true,
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(
            invocation.args,
            vec!["-p", "fix it", "--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn factory_builds_codex_yolo_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "codex".to_string(),
                prompt: Some("fix it".to_string()),
                yolo: true,
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(
            invocation.args,
            vec![
                "exec",
                "--dangerously-bypass-approvals-and-sandbox",
                "fix it"
            ]
        );
    }

    #[test]
    fn factory_builds_goose_yolo_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "goose".to_string(),
                prompt: Some("fix it".to_string()),
                yolo: true,
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.args, vec!["run", "-t", "fix it"]);
        assert_eq!(invocation.env.get("GOOSE_MODE"), Some(&"auto".to_string()));
    }

    #[test]
    fn factory_builds_aider_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "aider".to_string(),
                provider: Some("anthropic".to_string()),
                model: Some("claude-sonnet-4-6".to_string()),
                prompt: Some("fix lint".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "aider");
        assert_eq!(
            invocation.args,
            vec![
                "--message",
                "fix lint",
                "--model",
                "anthropic/claude-sonnet-4-6"
            ]
        );
    }

    #[test]
    fn factory_builds_amazon_q_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "aws-q".to_string(),
                agent: Some("builder".to_string()),
                prompt: Some("explain this stack".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "q");
        assert_eq!(
            invocation.args,
            vec!["chat", "--agent", "builder", "explain this stack"]
        );
    }

    #[test]
    fn factory_builds_goose_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "goose".to_string(),
                provider: Some("openai".to_string()),
                model: Some("gpt-5.4".to_string()),
                permission_mode: Some("auto".to_string()),
                max_turns: Some("50".to_string()),
                agent: Some("developer".to_string()),
                prompt: Some("fix the failing tests".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "goose");
        assert_eq!(
            invocation.args,
            vec![
                "run",
                "--with-builtin",
                "developer",
                "-t",
                "fix the failing tests",
            ]
        );
        assert_eq!(
            invocation.env.get("GOOSE_PROVIDER"),
            Some(&"openai".to_string())
        );
        assert_eq!(
            invocation.env.get("GOOSE_MODEL"),
            Some(&"gpt-5.4".to_string())
        );
        assert_eq!(invocation.env.get("GOOSE_MODE"), Some(&"auto".to_string()));
        assert_eq!(
            invocation.env.get("GOOSE_MAX_TURNS"),
            Some(&"50".to_string())
        );
    }

    #[test]
    fn factory_builds_antigravity_invocation() {
        let request = Request::from_options(
            CliOptions {
                harness: "agy".to_string(),
                model: Some("gemini-3-pro".to_string()),
                permission_mode: Some("always-proceed".to_string()),
                prompt: Some("review this".to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "agy");
        assert_eq!(
            invocation.args,
            vec!["--model", "gemini-3-pro", "--print", "review this"]
        );
    }

    #[test]
    fn antigravity_headless_uses_print_and_yolo() {
        let request = Request::from_options(
            CliOptions {
                harness: "agy".to_string(),
                prompt: Some("summarize the repo".to_string()),
                yolo: true,
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        // Yolo flag before --print; prompt is --print's value and comes last so
        // agy runs non-interactively instead of opening its TUI.
        assert_eq!(
            invocation.args,
            vec![
                "--dangerously-skip-permissions",
                "--print",
                "summarize the repo"
            ]
        );
    }

    #[test]
    fn antigravity_interactive_has_no_print() {
        let request = Request::from_options(
            CliOptions {
                harness: "agy".to_string(),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();

        let invocation = HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap();

        assert_eq!(invocation.command, "agy");
        assert!(!invocation.args.iter().any(|a| a == "--print"));
    }

    fn build(options: CliOptions) -> Invocation {
        let request = Request::from_options(options, String::new()).unwrap();
        HarnessFactory::default()
            .create(&request.harness)
            .unwrap()
            .build(&request)
            .unwrap()
    }

    #[test]
    fn claude_sets_then_resumes_a_session() {
        let set = build(CliOptions {
            harness: "claude".to_string(),
            prompt: Some("hi".to_string()),
            session_id: Some("sess-1".to_string()),
            ..CliOptions::default()
        });
        assert!(set
            .args
            .windows(2)
            .any(|w| w == ["--session-id".to_string(), "sess-1".to_string()]));

        let resumed = build(CliOptions {
            harness: "claude".to_string(),
            prompt: Some("more".to_string()),
            // resume wins over session_id when both are present
            session_id: Some("sess-1".to_string()),
            resume_id: Some("sess-1".to_string()),
            ..CliOptions::default()
        });
        assert!(resumed
            .args
            .windows(2)
            .any(|w| w == ["--resume".to_string(), "sess-1".to_string()]));
        assert!(!resumed.args.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn codex_resume_by_id_and_last() {
        let by_id = build(CliOptions {
            harness: "codex".to_string(),
            prompt: Some("go".to_string()),
            resume_id: Some("uuid-9".to_string()),
            ..CliOptions::default()
        });
        assert_eq!(by_id.args[0], "exec");
        assert_eq!(by_id.args[1], "resume");
        assert_eq!(by_id.args[2], "uuid-9");

        let last = build(CliOptions {
            harness: "codex".to_string(),
            prompt: Some("go".to_string()),
            resume_id: Some("latest".to_string()),
            ..CliOptions::default()
        });
        assert_eq!(&last.args[0..3], &["exec", "resume", "--last"]);
    }

    #[test]
    fn gemini_resume_latest_and_id() {
        let latest = build(CliOptions {
            harness: "gemini".to_string(),
            prompt: Some("go".to_string()),
            resume_id: Some("last".to_string()),
            ..CliOptions::default()
        });
        assert!(latest
            .args
            .windows(2)
            .any(|w| w == ["--resume".to_string(), "latest".to_string()]));

        let by_id = build(CliOptions {
            harness: "gemini".to_string(),
            prompt: Some("go".to_string()),
            resume_id: Some("5".to_string()),
            ..CliOptions::default()
        });
        assert!(by_id
            .args
            .windows(2)
            .any(|w| w == ["--resume".to_string(), "5".to_string()]));
    }
}
