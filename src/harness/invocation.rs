use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub env_remove: Vec<String>,
    pub clear_env: bool,
}

impl Invocation {
    pub(crate) fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            clear_env: false,
        }
    }

    pub(crate) fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn without_env(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    pub fn with_clean_env(mut self) -> Self {
        self.clear_env = true;
        self
    }

    pub(crate) fn to_json(&self) -> String {
        let args = self
            .args
            .iter()
            .map(|arg| format!("\"{}\"", escape_json(arg)))
            .collect::<Vec<_>>()
            .join(", ");
        let env = self
            .env
            .iter()
            .map(|(key, value)| {
                let display_value = if is_sensitive_env_name(key) {
                    "<redacted>"
                } else {
                    value
                };
                format!(
                    "\"{}\": \"{}\"",
                    escape_json(key),
                    escape_json(display_value)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");

        let env_remove = self
            .env_remove
            .iter()
            .map(|key| format!("\"{}\"", escape_json(key)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "{{\n  \"command\": \"{}\",\n  \"args\": [{}],\n  \"env\": {{{}}},\n  \"envRemove\": [{}],\n  \"clearEnv\": {}\n}}",
            escape_json(&self.command),
            args,
            env,
            env_remove,
            self.clear_env,
        )
    }
}

fn is_sensitive_env_name(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH", "CREDENTIAL"]
        .iter()
        .any(|part| key.contains(part))
}

fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|char| match char {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            value => vec![value],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_redacts_secret_environment_values() {
        let invocation = Invocation::new("agent", Vec::new())
            .with_env("OPENAI_BASE_URL", "http://localhost/v1")
            .with_env("OPENAI_API_KEY", "do-not-print");
        let json = invocation.to_json();
        assert!(json.contains("http://localhost/v1"));
        assert!(json.contains("<redacted>"));
        assert!(!json.contains("do-not-print"));
    }
}
