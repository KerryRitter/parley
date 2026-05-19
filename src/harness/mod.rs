mod aider;
mod amazon_q;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod gemini;
mod goose;
mod invocation;
mod opencode;
mod qwen;

use std::collections::HashMap;

use crate::cli::CliOptions;
use crate::model::{ModelFactory, ModelFormat};
pub(crate) use invocation::Invocation;

pub(crate) trait Harness {
    fn build(&self, request: &Request) -> Result<Invocation, String>;
}

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
    pub prompt: String,
    pub passthrough: Vec<String>,
    pub dry_run: bool,
}

impl Request {
    pub(crate) fn from_options(options: CliOptions, piped_input: String) -> Result<Self, String> {
        let prompt = merge_prompt(&piped_input, options.prompt.as_deref()).ok_or_else(|| {
            "missing prompt; pass -p \"...\", --prompt \"...\", a positional prompt, or stdin"
                .to_string()
        })?;

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
        })
    }
}

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
            ("cursor", cursor::new as HarnessConstructor),
            ("gemini", gemini::new as HarnessConstructor),
            ("goose", goose::new as HarnessConstructor),
            ("opencode", opencode::new as HarnessConstructor),
            ("copilot", copilot::new as HarnessConstructor),
            ("qwen", qwen::new as HarnessConstructor),
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

pub(crate) fn add_passthrough(mut args: Vec<String>, request: &Request) -> Vec<String> {
    args.extend(request.passthrough.iter().cloned());
    args
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

fn normalize_harness(harness: &str) -> String {
    match harness.to_ascii_lowercase().as_str() {
        "cursor-agent" => "cursor".to_string(),
        "open-code" => "opencode".to_string(),
        "google" | "google-gemini" => "gemini".to_string(),
        "openai" => "codex".to_string(),
        "github-copilot" => "copilot".to_string(),
        "amazonq" | "aws-q" | "amazon" => "amazon-q".to_string(),
        value => value.to_string(),
    }
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

        assert_eq!(request.prompt, "hello\n\nsummarize");
    }

    #[test]
    fn normalizes_aliases() {
        assert_eq!(normalize_harness("openai"), "codex");
        assert_eq!(normalize_harness("cursor-agent"), "cursor");
        assert_eq!(normalize_harness("aws-q"), "amazon-q");
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
}
