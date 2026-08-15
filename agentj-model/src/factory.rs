use std::sync::Arc;

use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;

use crate::anthropic::AnthropicProvider;
use crate::openai_compat::OpenAICompatibleProvider;
use crate::opencode_go::OpenCodeGoProvider;
use crate::responses::ResponsesProvider;

/// Single instantiation entry for model providers (design 4.3). The config `kind`
/// field ("openai-compat" | "responses" | "anthropic" | "opencode-go") selects the
/// concrete provider; unknown kinds are a config error (no silent fallback).
pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create(
        kind: &str,
        name: &str,
        base_url: &str,
        api_key: String,
    ) -> Result<Arc<dyn ModelProvider>, ModelError> {
        let provider: Arc<dyn ModelProvider> = match kind {
            "openai-compat" => Arc::new(OpenAICompatibleProvider::new(
                base_url.to_string(),
                api_key,
                name.to_string(),
            )),
            "responses" => Arc::new(ResponsesProvider::new(base_url.to_string(), api_key)),
            "anthropic" => Arc::new(AnthropicProvider::new(base_url.to_string(), api_key)),
            "opencode-go" => Arc::new(OpenCodeGoProvider::new(api_key)),
            other => {
                return Err(ModelError::Stream(format!(
                    "unknown provider kind: {other}"
                )));
            }
        };
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::types::ModelOptions;

    #[test]
    fn factory_creates_provider_per_kind() {
        let openai = ProviderFactory::create(
            "openai-compat",
            "deepseek-chat",
            "https://api.deepseek.com",
            "k".into(),
        );
        assert!(openai.is_ok());
        let responses = ProviderFactory::create(
            "responses",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(responses.is_ok());
        let anthropic = ProviderFactory::create(
            "anthropic",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(anthropic.is_ok());
        let og = ProviderFactory::create(
            "opencode-go",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(og.is_ok());
    }

    #[test]
    fn factory_rejects_unknown_kind() {
        let result = ProviderFactory::create("bogus", "x", "http://localhost", "k".into());
        assert!(matches!(result, Err(ModelError::Stream(_))));
    }

    #[tokio::test]
    async fn factory_opencode_go_routes_unknown_model() {
        let provider = ProviderFactory::create(
            "opencode-go",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        )
        .unwrap();
        let result = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/no-such".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(result, Err(ModelError::UnknownModel { .. })));
    }
}
