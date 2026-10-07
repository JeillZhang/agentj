use crate::AgentJConfig;

/// Overlay `file` onto `base`: non-empty file fields win, otherwise base stays.
pub fn merge_config(base: AgentJConfig, file: AgentJConfig) -> AgentJConfig {
    let mut config = base;
    if !file.model.providers.is_empty() {
        config.model.providers = file.model.providers;
    }
    if !file.model.provider.is_empty() && file.model.provider != "deepseek" {
        config.model.provider = file.model.provider;
    }
    if !file.model.default_model.is_empty() {
        config.model.default_model = file.model.default_model;
    }
    if file.model.quota_warn_percent != crate::default_quota_warn_percent() {
        config.model.quota_warn_percent = file.model.quota_warn_percent;
    }
    if !file.model.console_base_url.is_empty()
        && file.model.console_base_url != crate::default_console_base_url()
    {
        config.model.console_base_url = file.model.console_base_url;
    }
    if file.permission.is_some() {
        config.permission = file.permission;
    }
    if file.session.is_some() {
        config.session = file.session;
    }
    if file.license.is_some() {
        config.license = file.license;
    }
    if file.mcp.is_some() {
        config.mcp = file.mcp;
    }
    config
}

/// Apply environment overrides (AGENTJ_* prefix) onto config — env always wins.
pub fn apply_env(mut config: AgentJConfig) -> AgentJConfig {
    if let Ok(val) = std::env::var("AGENTJ_MODEL_PROVIDER") {
        config.model.provider = val;
    }
    if let Ok(val) = std::env::var("AGENTJ_LICENSE_KEY")
        && !val.is_empty()
    {
        config
            .license
            .get_or_insert_with(crate::LicenseConfig::default)
            .key = Some(val);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentJConfig, ModelProviderConfig, ModelSection};

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
                quota_warn_percent: crate::default_quota_warn_percent(),
                console_base_url: crate::default_console_base_url(),
            },
            permission: None,
            session: None,
            license: None,
            mcp: None,
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
                quota_warn_percent: crate::default_quota_warn_percent(),
                console_base_url: crate::default_console_base_url(),
            },
            permission: None,
            session: None,
            license: None,
            mcp: None,
        };
        let merged = merge_config(base, file);
        assert_eq!(merged.model.default_model, "deepseek-chat");
    }
}
