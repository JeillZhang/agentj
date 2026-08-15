#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentJConfig, ModelSection};

    #[test]
    fn merge_precedence() {
        let base = AgentJConfig::default();
        let file = AgentJConfig {
            model: ModelSection {
                provider: "deepseek".into(),
                providers: vec![ModelProviderConfig {
                    name: "opencode-go".into(),
                    base_url: "http://localhost:8000".into(),
                    api_key_env: None,
                    kind: "opencode-go".into(),
                }],
                default_model: "opencode-go/kimi-k3".into(),
            },
        };
        let merged = merge_config(base, file);
        assert_eq!(merged.model.providers.len(), 1);
        assert_eq!(merged.model.providers[0].name, "opencode-go");
        assert_eq!(merged.model.providers[0].kind, "opencode-go");
        assert_eq!(merged.model.default_model, "opencode-go/kimi-k3");
    }

    #[test]
    fn default_model_keeps_base_when_file_empty() {
        let base = AgentJConfig::default();
        let file = AgentJConfig {
            model: ModelSection {
                provider: "deepseek".into(),
                providers: vec![],
                default_model: String::new(), // empty → keep base
            },
        };
        let merged = merge_config(base, file);
        assert_eq!(merged.model.default_model, "deepseek-chat");
    }
}
