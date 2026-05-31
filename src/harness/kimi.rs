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

        // Kimi only auto-loads MCP from ~/.kimi/mcp.json (global). Point it at the
        // project's MCP config so project servers are available: prefer the
        // Kimi-specific ./.kimi/mcp.json (written by `par convert`), else fall back
        // to the standard ./.mcp.json — same `{mcpServers:{...}}` shape Kimi accepts.
        if let Some(mcp_path) = project_mcp_config(request) {
            args.push("--mcp-config-file".to_string());
            args.push(mcp_path.to_string_lossy().to_string());
        }

        let args = add_yolo_args(args, request)?;
        Ok(Invocation::new("kimi", add_passthrough(args, request)))
    }
}

/// Path to the project-level MCP config: prefers `<cwd>/.kimi/mcp.json`, falling
/// back to the standard `<cwd>/.mcp.json`. Returns the first that exists, else
/// `None`. Resolves relative to the request's cwd (or the process cwd when unset).
fn project_mcp_config(request: &Request) -> Option<PathBuf> {
    let base = Path::new(request.cwd.as_deref().unwrap_or("."));
    let kimi = base.join(".kimi").join("mcp.json");
    if kimi.exists() {
        return Some(kimi);
    }
    let standard = base.join(".mcp.json");
    standard.exists().then_some(standard)
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
    fn falls_back_to_standard_mcp_json_when_kimi_absent() {
        let dir = temp_dir("fallback");
        fs::write(dir.join(".mcp.json"), "{\"mcpServers\":{}}").unwrap();

        let invocation = build(&dir, "go");
        let joined = invocation.args.join(" ");
        assert!(
            invocation.args.iter().any(|a| a == "--mcp-config-file"),
            "missing flag in {joined}"
        );
        assert!(
            joined.contains(".mcp.json") && !joined.contains(".kimi"),
            "expected standard .mcp.json path in {joined}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_kimi_mcp_over_standard() {
        let dir = temp_dir("prefer");
        fs::create_dir_all(dir.join(".kimi")).unwrap();
        fs::write(dir.join(".kimi/mcp.json"), "{\"mcpServers\":{}}").unwrap();
        fs::write(dir.join(".mcp.json"), "{\"mcpServers\":{}}").unwrap();

        let invocation = build(&dir, "go");
        let joined = invocation.args.join(" ");
        assert!(joined.contains(".kimi/mcp.json"), "expected .kimi path in {joined}");

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
