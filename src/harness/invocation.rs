use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Invocation {
    pub(crate) fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            env: BTreeMap::new(),
        }
    }

    pub(crate) fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
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
            .map(|(key, value)| format!("\"{}\": \"{}\"", escape_json(key), escape_json(value)))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "{{\n  \"command\": \"{}\",\n  \"args\": [{}],\n  \"env\": {{{}}}\n}}",
            escape_json(&self.command),
            args,
            env
        )
    }
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
