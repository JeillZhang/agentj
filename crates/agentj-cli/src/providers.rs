//! Provider assembly: credential resolution + catalog build + keyless skipping
//! (design D7). Replaces the old "empty Bearer" registration loop.

use std::sync::Arc;

use agentj_auth::{AuthStore, resolve_credential};
use agentj_config::AgentJConfig;
use agentj_core::module::ModelProvider;
use agentj_model::catalog::ModelCatalog;
use agentj_model::factory::ProviderFactory;

/// Kinds that require a resolvable key; everything else (mock*) is key-free.
const KEY_REQUIRED_KINDS: [&str; 4] = ["openai-compat", "responses", "anthropic", "opencode-go"];

pub struct Assembly {
    pub providers: Vec<(String, Arc<dyn ModelProvider>)>,
    pub warnings: Vec<String>,
}

/// Build the provider registry entries, skipping keyless real providers with a warning.
///
/// A provider that needs a key but cannot resolve one is never registered, so no
/// request is ever issued with an empty `Authorization` header. If nothing usable
/// remains, this is a hard error (design D7 / G2).
pub async fn build_providers<F, Fut>(
    config: &AgentJConfig,
    flag: Option<&str>,
    store: &AuthStore,
    live: F,
) -> Result<Assembly, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Option<Vec<String>>, String>>,
{
    let mut providers: Vec<(String, Arc<dyn ModelProvider>)> = vec![];
    let mut warnings: Vec<String> = vec![];
    for pcfg in &config.model.providers {
        if !KEY_REQUIRED_KINDS.contains(&pcfg.kind.as_str()) {
            let provider =
                ProviderFactory::create(&pcfg.kind, &pcfg.name, &pcfg.base_url, String::new())
                    .map_err(|e| e.to_string())?;
            providers.push((pcfg.name.clone(), provider));
            continue;
        }
        let resolved =
            match resolve_credential(&pcfg.name, flag, pcfg.api_key_env.as_deref(), store)
                .map_err(|e| e.to_string())?
            {
                Some(r) => r,
                None => {
                    warnings.push(format!(
                        "provider {} 无可用密钥（api_key_env={:?}），已跳过",
                        pcfg.name, pcfg.api_key_env
                    ));
                    continue;
                }
            };
        if pcfg.kind == "opencode-go" {
            let live_ids = match live().await {
                Ok(v) => v,
                Err(e) => {
                    warnings.push(format!(
                        "opencode-go：live 目录不可用（{e}），回退提交件 overlay"
                    ));
                    None
                }
            };
            let catalog = ModelCatalog::from_overlay_and_live(ModelCatalog::builtin(), live_ids);
            warnings.extend(
                catalog
                    .unverified_ids()
                    .into_iter()
                    .map(|id| format!("opencode-go：模型路由未验证：{id}")),
            );
            let provider = ProviderFactory::create_with_catalog(
                "opencode-go",
                &pcfg.name,
                &pcfg.base_url,
                resolved.key.clone(),
                catalog,
            )
            .map_err(|e| e.to_string())?;
            providers.push((pcfg.name.clone(), provider));
        } else {
            let provider = ProviderFactory::create(
                &pcfg.kind,
                &pcfg.name,
                &pcfg.base_url,
                resolved.key.clone(),
            )
            .map_err(|e| e.to_string())?;
            providers.push((pcfg.name.clone(), provider));
        }
        // Provenance line: source + last 4 only; `redact` is the single redaction point.
        tracing::info!(
            "provider {} 密钥来源 {:?} {}",
            pcfg.name,
            resolved.source,
            agentj_auth::redact(&resolved.key)
        );
    }
    if providers.is_empty() {
        return Err("no usable model provider：未解析到任何密钥，见 agentj auth status".into());
    }
    Ok(Assembly {
        providers,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_config::ModelProviderConfig;
    use wiremock::MockServer;

    fn provider(name: &str, kind: &str, api_key_env: &str) -> ModelProviderConfig {
        ModelProviderConfig {
            name: name.into(),
            kind: kind.into(),
            api_key_env: Some(api_key_env.into()),
            base_url: "https://example.invalid".into(),
        }
    }

    fn provider_base(
        name: &str,
        kind: &str,
        api_key_env: &str,
        base_url: &str,
    ) -> ModelProviderConfig {
        ModelProviderConfig {
            name: name.into(),
            kind: kind.into(),
            api_key_env: Some(api_key_env.into()),
            base_url: base_url.into(),
        }
    }

    fn config_with(entries: Vec<ModelProviderConfig>) -> AgentJConfig {
        let mut c = AgentJConfig::default();
        c.model.providers = entries;
        c
    }

    #[tokio::test]
    async fn keyless_provider_is_skipped_with_a_warning_when_another_is_usable() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![
            provider("mock", "mock", ""),
            provider("deepseek", "openai-compat", "AGENTJ_MISSING"),
        ]);
        let out = build_providers(&config, None, &store, || async { Ok(None) })
            .await
            .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert_eq!(out.providers[0].0, "mock");
        assert!(out.warnings.iter().any(|w| w.contains("deepseek")));
    }

    #[tokio::test]
    async fn no_usable_provider_is_a_hard_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![provider(
            "deepseek",
            "openai-compat",
            "AGENTJ_MISSING",
        )]);
        let err = match build_providers(&config, None, &store, || async { Ok(None) }).await {
            Err(e) => e,
            Ok(_) => panic!("expected a hard error"),
        };
        assert!(err.contains("no usable model provider"), "{err}");
    }

    #[tokio::test]
    async fn mock_kind_needs_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![provider("mock", "mock", "")]);
        let out = build_providers(&config, None, &store, || async { Ok(None) })
            .await
            .unwrap();
        assert_eq!(out.providers.len(), 1);
    }

    #[tokio::test]
    async fn resolvable_env_key_registers_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        // SAFETY: unique test-local variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_ASSEMBLY_KEY", "k") };
        let config = config_with(vec![provider(
            "deepseek",
            "openai-compat",
            "AGENTJ_TEST_ASSEMBLY_KEY",
        )]);
        let out = build_providers(&config, None, &store, || async { Ok(None) })
            .await
            .unwrap();
        assert_eq!(out.providers.len(), 1);
    }

    #[tokio::test]
    async fn opencode_go_registers_with_overlay_when_live_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        // SAFETY: unique test-local variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_OG_KEY", "k") };
        let config = config_with(vec![provider(
            "opencode-go",
            "opencode-go",
            "AGENTJ_TEST_OG_KEY",
        )]);
        let out = build_providers(&config, None, &store, || async {
            Err("offline".to_string())
        })
        .await
        .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert!(
            out.warnings.iter().any(|w| w.contains("overlay")),
            "{:?}",
            out.warnings
        );
    }

    #[tokio::test]
    async fn keyless_provider_never_reaches_the_network() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![
            provider("mock", "mock", ""),
            provider_base("deepseek", "openai-compat", "AGENTJ_MISSING", &server.uri()),
        ]);
        let out = build_providers(&config, None, &store, || async { Ok(None) })
            .await
            .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "no empty-Bearer request may be sent"
        );
    }
}
