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
    if file.model.max_context_tokens != crate::default_max_context_tokens() {
        config.model.max_context_tokens = file.model.max_context_tokens;
    }
    if !file.model.console_base_url.is_empty()
        && file.model.console_base_url != crate::default_console_base_url()
    {
        config.model.console_base_url = file.model.console_base_url;
    }
    if file.agent.max_tool_iterations != crate::default_max_tool_iterations() {
        config.agent.max_tool_iterations = file.agent.max_tool_iterations;
    }
    if file.agent.max_tool_result_chars != crate::default_max_tool_result_chars() {
        config.agent.max_tool_result_chars = file.agent.max_tool_result_chars;
    }
    if file.agent.question_timeout_s != crate::default_question_timeout_s() {
        config.agent.question_timeout_s = file.agent.question_timeout_s;
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
    if file.web.is_some() {
        config.web = file.web;
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
                max_context_tokens: crate::default_max_context_tokens(),
            },
            agent: crate::AgentSection::default(),
            permission: None,
            session: None,
            license: None,
            mcp: None,
            web: None,
        };
        let merged = merge_config(base, file);
        assert_eq!(merged.model.providers.len(), 1);
        assert_eq!(merged.model.providers[0].name, "opencode-go");
        assert_eq!(merged.model.providers[0].kind, "opencode-go");
        assert_eq!(merged.model.default_model, "opencode-go/kimi-k3");
    }

    /// A file-level `[agent] max_tool_iterations` must survive the overlay (the key used to be
    /// unreachable from config at all). @since 0.29.9
    #[test]
    fn file_tool_iteration_budget_wins_over_the_default() {
        let base = AgentJConfig::default();
        let file = AgentJConfig {
            agent: crate::AgentSection {
                max_tool_iterations: 120,
                ..crate::AgentSection::default()
            },
            ..AgentJConfig::default()
        };
        assert_eq!(merge_config(base, file).agent.max_tool_iterations, 120);
    }

    #[test]
    fn merge_keeps_file_level_max_tool_result_chars() {
        let mut file = crate::AgentJConfig::default();
        file.agent.max_tool_result_chars = 2_000;
        let merged = crate::merge::merge_config(crate::AgentJConfig::default(), file);
        assert_eq!(merged.agent.max_tool_result_chars, 2_000);
    }

    #[test]
    fn merge_keeps_file_level_question_timeout_s() {
        let mut file = crate::AgentJConfig::default();
        file.agent.question_timeout_s = 300;
        let merged = crate::merge::merge_config(crate::AgentJConfig::default(), file);
        assert_eq!(merged.agent.question_timeout_s, 300);
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
                max_context_tokens: crate::default_max_context_tokens(),
            },
            agent: crate::AgentSection::default(),
            permission: None,
            session: None,
            license: None,
            mcp: None,
            web: None,
        };
        let merged = merge_config(base, file);
        assert_eq!(merged.model.default_model, "deepseek-chat");
    }

    #[test]
    fn web_section_merges_only_when_the_file_has_one() {
        let base = AgentJConfig::default();
        let file = AgentJConfig {
            web: Some(crate::WebSection {
                trust_proxy_headers: true,
                state_dir: None,
                ..Default::default()
            }),
            ..AgentJConfig::default()
        };
        assert!(merge_config(base, file).web.unwrap().trust_proxy_headers);
        assert!(
            merge_config(AgentJConfig::default(), AgentJConfig::default())
                .web
                .is_none()
        );
    }
}
