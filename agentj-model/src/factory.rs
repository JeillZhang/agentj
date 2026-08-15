// In agentj-model/src/factory.rs (tests module)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_creates_provider_per_kind() {
        let openai = ProviderFactory::create("openai-compat", "deepseek-chat", "https://api.deepseek.com", "k".into());
        assert!(openai.is_ok());
        let responses = ProviderFactory::create("responses", "opencode-go", "https://opencode.ai/zen/go", "k".into());
        assert!(responses.is_ok());
        let anthropic = ProviderFactory::create("anthropic", "opencode-go", "https://opencode.ai/zen/go", "k".into());
        assert!(anthropic.is_ok());
        let og = ProviderFactory::create("opencode-go", "opencode-go", "https://opencode.ai/zen/go", "k".into());
        assert!(og.is_ok());
    }

    #[test]
    fn factory_rejects_unknown_kind() {
        let result = ProviderFactory::create("bogus", "x", "http://localhost", "k".into());
        assert!(matches!(result, Err(ModelError::Stream(_))));
    }

    #[tokio::test]
    async fn factory_opencode_go_routes_unknown_model() {
        let provider = ProviderFactory::create("opencode-go", "opencode-go", "https://opencode.ai/zen/go", "k".into()).unwrap();
        let result = provider.chat_stream(
            vec![], vec![],
            ModelOptions { model: "opencode-go/no-such".into(), ..Default::default() },
        ).await;
        assert!(matches!(result, Err(ModelError::UnknownModel { .. })));
    }
}
