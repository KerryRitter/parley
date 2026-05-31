use std::path::{Path, PathBuf};

use super::{add_passthrough, add_yolo_args, plain_model, Harness, Invocation, Request};

pub(crate) fn new() -> Box<dyn Harness> {
    Box::new(KimiHarness)
}

struct KimiHarness;

impl Harness for KimiHarness {
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
        }

        // Kimi only auto-loads MCP from ~/.kimi/mcp.json (global). `par convert`
        // writes the project's MCP config to ./.kimi/mcp.json, which Kimi ignores
        // unless pointed at it. Load it explicitly when present so project MCP
        // servers (e.g. the ones generated from .mcp.json) are available.
        if let Some(mcp_path) = project_mcp_config(request) {
            args.push("--mcp-config-file".to_string());
            args.push(mcp_path.to_string_lossy().to_string());
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("kimi", add_passthrough(args, request)))
    }
}

/// Path to the project-level Kimi MCP config (`<cwd>/.kimi/mcp.json`) if it
/// exists, else `None`. Resolves relative to the request's cwd (or the process
/// cwd when unset).
fn project_mcp_config(request: &Request) -> Option<PathBuf> {
    let base = request.cwd.as_deref().unwrap_or(".");
    let path = Path::new(base).join(".kimi").join("mcp.json");
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliOptions;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("par-kimi-{}-{}-{}", std::process::id(), tag, n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build(cwd: &Path, prompt: &str) -> Invocation {
        let request = Request::from_options(
            CliOptions {
                harness: "kimi".to_string(),
                prompt: Some(prompt.to_string()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                ..CliOptions::default()
            },
            String::new(),
        )
        .unwrap();
        KimiHarness.build(&request).unwrap()
    }

    #[test]
    fn adds_mcp_config_flag_when_project_file_present() {
        let dir = temp_dir("present");
        fs::create_dir_all(dir.join(".kimi")).unwrap();
        fs::write(dir.join(".kimi/mcp.json"), "{\"mcpServers\":{}}").unwrap();

        let invocation = build(&dir, "go");
        let joined = invocation.args.join(" ");
        assert!(
            invocation.args.iter().any(|a| a == "--mcp-config-file"),
            "missing flag in {joined}"
        );
        assert!(joined.contains(".kimi/mcp.json"), "missing path in {joined}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn omits_mcp_config_flag_when_file_absent() {
        let dir = temp_dir("absent");
        let invocation = build(&dir, "go");
        assert!(
            !invocation.args.iter().any(|a| a == "--mcp-config-file"),
            "unexpected flag: {:?}",
            invocation.args
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
