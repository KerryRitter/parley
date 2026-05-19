#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelSelection {
    provider: Option<String>,
    name: String,
    original: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelFormat {
    Plain,
    ProviderQualified,
}

pub(crate) struct ModelFactory;

impl ModelFactory {
    pub(crate) fn resolve(provider: Option<&str>, model: Option<&str>) -> Option<ModelSelection> {
        let original = model?;
        let (provider, name) = match original.split_once('/') {
            Some((provider, name)) => (Some(provider.to_string()), name.to_string()),
            None => (provider.map(str::to_string), original.to_string()),
        };

        Some(ModelSelection {
            provider,
            name,
            original: original.to_string(),
        })
    }
}

impl ModelSelection {
    pub(crate) fn format(&self, format: ModelFormat) -> String {
        match format {
            ModelFormat::Plain => self.original.clone(),
            ModelFormat::ProviderQualified if self.original.contains('/') => self.original.clone(),
            ModelFormat::ProviderQualified => match &self.provider {
                Some(provider) => format!("{provider}/{}", self.name),
                None => self.name.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_provider_qualified_model() {
        let model = ModelFactory::resolve(Some("anthropic"), Some("claude-sonnet-4-6")).unwrap();
        assert_eq!(
            model.format(ModelFormat::ProviderQualified),
            "anthropic/claude-sonnet-4-6"
        );
    }

    #[test]
    fn preserves_embedded_provider() {
        let model = ModelFactory::resolve(Some("ignored"), Some("openai/gpt-5.4")).unwrap();
        assert_eq!(
            model.format(ModelFormat::ProviderQualified),
            "openai/gpt-5.4"
        );
    }
}
