pub mod merge;
use agentj_core::module::ConfigProvider;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    /// Wire-format discriminator (design 4.3): "openai-compat" | "responses" |
    /// "anthropic" | "opencode-go". Default "openai-compat" keeps legacy configs working.
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "openai-compat".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSection {
    pub provider: String,
    #[serde(default)]
    pub providers: Vec<ModelProviderConfig>,
    /// Default model id for AgentLoop (design 4.3). Injected into ModelOptions via
    /// ConfigProvider::get_str("model.default_model"); default "deepseek-chat".
    #[serde(default = "default_model")]
    pub default_model: String,
}

fn default_model() -> String {
    "deepseek-chat".into()
}

impl Default for ModelSection {
    fn default() -> Self {
        Self {
            provider: "deepseek".into(),
            providers: vec![],
            default_model: default_model(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentJConfig {
    #[serde(default)]
    pub model: ModelSection,
}

impl Default for AgentJConfig {
    fn default() -> Self {
        Self {
            model: ModelSection {
                provider: "deepseek".into(),
                providers: vec![ModelProviderConfig {
                    name: "deepseek-chat".into(),
                    base_url: "https://api.deepseek.com".into(),
                    api_key_env: Some("DEEPSEEK_API_KEY".into()),
                    kind: default_kind(),
                }],
                default_model: default_model(),
            },
        }
    }
}

pub struct Config {
    inner: AgentJConfig,
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = AgentJConfig::default();
        if let Some(p) = path
            && p.exists()
        {
            let content = std::fs::read_to_string(&p)?;
            let file_config: AgentJConfig = toml::from_str(&content)?;
            config = merge::merge_config(config, file_config);
        }
        config = merge::apply_env(config);
        Ok(Self { inner: config })
    }

    pub fn inner(&self) -> &AgentJConfig {
        &self.inner
    }
}

impl ConfigProvider for Config {
    fn get_str(&self, key: &str) -> Option<String> {
        match key {
            "model.provider" => Some(self.inner.model.provider.clone()),
            "model.default_model" => Some(self.inner.model.default_model.clone()),
            _ => None,
        }
    }
    fn get_int(&self, _key: &str) -> Option<i64> {
        None
    }
    fn providers(&self) -> Vec<(String, String)> {
        self.inner
            .model
            .providers
            .iter()
            .map(|p| (p.name.clone(), p.base_url.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#,
        )
        .unwrap();

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

        unsafe { std::env::set_var("AGENTJ_MODEL_PROVIDER", "opencode-go") };
        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.provider, "opencode-go");
        unsafe { std::env::remove_var("AGENTJ_MODEL_PROVIDER") };
    }

    #[test]
    fn kind_defaults_to_openai_compat_for_legacy_config() {
        // Legacy config (design 4.3): no `kind` field → "openai-compat" (zero break).
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.providers[0].kind, "openai-compat");
        assert_eq!(config.inner().model.default_model, "deepseek-chat");
    }

    #[test]
    fn opencode_go_kind_and_default_model_parse() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "opencode-go"
        default_model = "opencode-go/kimi-k3"
        [[model.providers]]
        name = "opencode-go"
        kind = "opencode-go"
        base_url = "https://opencode.ai/zen/go"
        api_key_env = "OPENCODE_GO_API_KEY"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        let p = &config.inner().model.providers[0];
        assert_eq!(p.kind, "opencode-go");
        assert_eq!(p.base_url, "https://opencode.ai/zen/go");
        assert_eq!(config.inner().model.default_model, "opencode-go/kimi-k3");
        assert_eq!(
            config.get_str("model.default_model").as_deref(),
            Some("opencode-go/kimi-k3")
        );
    }
}
