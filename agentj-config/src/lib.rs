pub mod merge;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_default_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        let providers = config.providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].0, "deepseek-chat");
    }

    #[test]
    fn env_overrides_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "[model]\nprovider = \"deepseek\"\n").unwrap();

        std::env::set_var("AGENTJ_MODEL_PROVIDER", "opencode-go");
        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.provider, "opencode-go");
        std::env::remove_var("AGENTJ_MODEL_PROVIDER");
    }

    #[test]
    fn kind_defaults_to_openai_compat_for_legacy_config() {
        // Legacy config (design 4.3): no `kind` field → "openai-compat" (zero break).
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.providers[0].kind, "openai-compat");
        assert_eq!(config.inner().model.default_model, "deepseek-chat");
    }

    #[test]
    fn opencode_go_kind_and_default_model_parse() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, r#"[model]
        provider = "opencode-go"
        default_model = "opencode-go/kimi-k3"
        [[model.providers]]
        name = "opencode-go"
        kind = "opencode-go"
        base_url = "https://opencode.ai/zen/go"
        api_key_env = "OPENCODE_GO_API_KEY"
        "#).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        let p = &config.inner().model.providers[0];
        assert_eq!(p.kind, "opencode-go");
        assert_eq!(p.base_url, "https://opencode.ai/zen/go");
        assert_eq!(config.inner().model.default_model, "opencode-go/kimi-k3");
        assert_eq!(config.get_str("model.default_model").as_deref(), Some("opencode-go/kimi-k3"));
    }
}
