//! Provider assembly: credential resolution + catalog build + keyless skipping
//! (design D7). Replaces the old "empty Bearer" registration loop.

use std::sync::Arc;

use agentj_auth::credential::Credential;
use agentj_auth::{AuthStore, ResolvedKey, resolve_credential};
use agentj_config::{AgentJConfig, ModelProviderConfig};
use agentj_core::module::ModelProvider;
use agentj_model::catalog::ModelCatalog;
use agentj_model::factory::ProviderFactory;

use crate::refreshing_provider::{Builder as RefreshingBuilder, RefreshingProvider};

/// Kinds that require a resolvable key; everything else (mock*) is key-free.
const KEY_REQUIRED_KINDS: [&str; 4] = ["openai-compat", "responses", "anthropic", "opencode-go"];

/// Reported (never fatal here) when nothing resolved. An IPC-serving command must still boot
/// so a client can link a credential at runtime (design D2/D3/D7), while one-shot `run` has no
/// such client and turns this into its startup error — `main` owns that decision.
/// Single source of the message so both paths read identically.
pub const NO_USABLE_PROVIDER: &str =
    "no usable model provider：未解析到任何密钥，见 agentj auth status";

/// Base URL of the built-in opencode-go entry (design D5). Single source of truth lives in
/// `agentj_model::catalog` beside the generated endpoint catalog.
pub const BUILTIN_OPENCODE_GO_BASE: &str = agentj_model::catalog::OPENCODE_GO_BASE;

/// `config.model.providers` ∪ the built-in opencode-go entry (design D5): convention over
/// configuration. An explicit `kind = "opencode-go"` entry wins, so a user's `base_url` /
/// `api_key_env` keeps applying and no duplicate entry is appended.
pub fn effective_provider_cfgs(config: &AgentJConfig) -> Vec<ModelProviderConfig> {
    let mut out = config.model.providers.clone();
    if !out.iter().any(|p| p.kind == "opencode-go") {
        out.push(ModelProviderConfig {
            name: "opencode-go".into(),
            kind: "opencode-go".into(),
            api_key_env: None,
            base_url: BUILTIN_OPENCODE_GO_BASE.into(),
        });
    }
    out
}

/// The cfg list `CliProviderService` is built from — the startup derivation itself, so the
/// provider set the UI sees can never drift from the set the kernel assembles (design D5).
pub fn service_cfgs(config: &AgentJConfig) -> Vec<ModelProviderConfig> {
    effective_provider_cfgs(config)
}

pub struct Assembly {
    pub providers: Vec<(String, Arc<dyn ModelProvider>)>,
    pub warnings: Vec<String>,
}

/// One provider's assembly outcome: the registry entry (absent when the provider was
/// skipped) plus every warning it produced, in emission order.
struct ProviderBuild {
    provider: Option<Arc<dyn ModelProvider>>,
    warnings: Vec<String>,
}

/// A successful build with no warnings of its own.
fn built(provider: Arc<dyn ModelProvider>) -> ProviderBuild {
    ProviderBuild {
        provider: Some(provider),
        warnings: vec![],
    }
}

/// Build the provider registry entries, skipping keyless real providers with a warning.
///
/// A provider that needs a key but cannot resolve one is never registered, so no
/// request is ever issued with an empty `Authorization` header. An empty registry is
/// returned as an empty [`Assembly`] carrying [`NO_USABLE_PROVIDER`] among its warnings;
/// whether that is fatal is the caller's decision (`serve`/`dashboard` boot on and let a
/// client link a key, `run` does not).
pub async fn build_providers<F, Fut>(
    config: &AgentJConfig,
    flag: Option<&str>,
    store: &AuthStore,
    live: F,
    console_base: &str,
) -> Result<Assembly, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Option<Vec<String>>, String>>,
{
    let mut providers: Vec<(String, Arc<dyn ModelProvider>)> = vec![];
    let mut warnings: Vec<String> = vec![];
    for pcfg in &effective_provider_cfgs(config) {
        let build = assemble(pcfg, flag, store, &live, console_base).await?;
        warnings.extend(build.warnings);
        if let Some(provider) = build.provider {
            providers.push((pcfg.name.clone(), provider));
        }
    }
    if providers.is_empty() {
        warnings.push(NO_USABLE_PROVIDER.to_string());
    }
    Ok(Assembly {
        providers,
        warnings,
    })
}

/// Assemble one provider entry: key-free kinds are created directly, key-requiring kinds
/// resolve their credential first and are skipped (with a warning) when none exists.
async fn assemble<F, Fut>(
    pcfg: &ModelProviderConfig,
    flag: Option<&str>,
    store: &AuthStore,
    live: &F,
    console_base: &str,
) -> Result<ProviderBuild, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Option<Vec<String>>, String>>,
{
    if !KEY_REQUIRED_KINDS.contains(&pcfg.kind.as_str()) {
        return Ok(built(create_provider(pcfg, String::new())?));
    }
    let resolved = match resolve_credential(&pcfg.name, flag, pcfg.api_key_env.as_deref(), store)
        .map_err(|e| e.to_string())?
    {
        Some(r) => r,
        None => return Ok(skipped(pcfg)),
    };
    let build = if pcfg.kind == "opencode-go" {
        opencode_go(pcfg, store, live, console_base, &resolved).await?
    } else {
        built(create_provider(pcfg, resolved.key.clone())?)
    };
    // Provenance line: source + last 4 only; `redact` is the single redaction point.
    tracing::info!(
        "provider {} 密钥来源 {:?} {}",
        pcfg.name,
        resolved.source,
        agentj_auth::redact(&resolved.key)
    );
    Ok(build)
}

/// A plain provider instance for `pcfg` built around an already-resolved key.
fn create_provider(
    pcfg: &ModelProviderConfig,
    key: String,
) -> Result<Arc<dyn ModelProvider>, String> {
    ProviderFactory::create(&pcfg.kind, &pcfg.name, &pcfg.base_url, key).map_err(|e| e.to_string())
}

/// The outcome for a provider that needs a key and resolved none: no entry, one warning.
fn skipped(pcfg: &ModelProviderConfig) -> ProviderBuild {
    let mut warning = format!(
        "provider {} 无可用密钥（api_key_env={:?}），已跳过",
        pcfg.name, pcfg.api_key_env
    );
    if pcfg.kind == "opencode-go" {
        warning.push_str("；可在设置 · 订阅中链接 API Key");
    }
    ProviderBuild {
        provider: None,
        warnings: vec![warning],
    }
}

/// `opencode-go` (design D5/D8): catalog from overlay+live, entry wrapped for OAuth
/// refresh. Live-catalog failures are reported, never fatal.
async fn opencode_go<F, Fut>(
    pcfg: &ModelProviderConfig,
    store: &AuthStore,
    live: &F,
    console_base: &str,
    resolved: &ResolvedKey,
) -> Result<ProviderBuild, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Option<Vec<String>>, String>>,
{
    let mut warnings: Vec<String> = vec![];
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
    let provider = opencode_go_provider(pcfg, store, console_base, catalog, &resolved.key)?;
    Ok(ProviderBuild {
        provider: Some(provider),
        warnings,
    })
}

/// The `opencode-go` entry: an OAuth credential keeps its 600s access token fresh before
/// every chat call, an api-key gets the same catalog behind a static instance.
fn opencode_go_provider(
    pcfg: &ModelProviderConfig,
    store: &AuthStore,
    console_base: &str,
    catalog: ModelCatalog,
    key: &str,
) -> Result<Arc<dyn ModelProvider>, String> {
    if let Some(Credential::OAuth { .. }) = store.get(&pcfg.name).ok().flatten() {
        let build: RefreshingBuilder = {
            let catalog = catalog.clone();
            let name = pcfg.name.clone();
            let base = pcfg.base_url.clone();
            Arc::new(move |key: &str| {
                ProviderFactory::create_with_catalog(
                    "opencode-go",
                    &name,
                    &base,
                    key.to_string(),
                    catalog.clone(),
                )
                .expect("opencode-go provider")
            })
        };
        return Ok(Arc::new(RefreshingProvider::oauth(
            build,
            store.clone(),
            &pcfg.name,
            console_base,
            key.to_string(),
        )));
    }
    let provider = ProviderFactory::create_with_catalog(
        "opencode-go",
        &pcfg.name,
        &pcfg.base_url,
        key.to_string(),
        catalog,
    )
    .map_err(|e| e.to_string())?;
    Ok(Arc::new(RefreshingProvider::r#static(provider)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_config::ModelProviderConfig;
    use agentj_core::types::ModelOptions;
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
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert_eq!(out.providers[0].0, "mock");
        assert!(out.warnings.iter().any(|w| w.contains("deepseek")));
    }

    #[tokio::test]
    async fn keyless_provider_set_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![provider(
            "deepseek",
            "openai-compat",
            "AGENTJ_MISSING",
        )]);
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        assert!(
            out.providers.is_empty(),
            "nothing resolved, so nothing is registered"
        );
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("no usable model provider")),
            "the empty assembly must still be reported: {:?}",
            out.warnings
        );
    }

    #[tokio::test]
    async fn mock_kind_needs_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let config = config_with(vec![provider("mock", "mock", "")]);
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
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
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
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
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Err("offline".to_string()) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert!(
            out.warnings.iter().any(|w| w.contains("overlay")),
            "{:?}",
            out.warnings
        );
        assert_eq!(
            out.providers[0].1.models(),
            agentj_model::catalog::ModelCatalog::builtin().ids(),
            "the overlay catalog must be injected when live fails"
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
        let out = build_providers(
            &config,
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        assert_eq!(out.providers.len(), 1);
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "no empty-Bearer request may be sent"
        );
    }

    fn config_with_opencode_go(base_url: &str) -> AgentJConfig {
        config_with(vec![ModelProviderConfig {
            name: "opencode-go".into(),
            kind: "opencode-go".into(),
            api_key_env: None,
            base_url: base_url.into(),
        }])
    }

    fn store_with_api_key(dir: &std::path::Path, key: &str) -> AuthStore {
        let store = AuthStore::with_path(dir.join("auth.json"));
        store
            .set(
                "opencode-go",
                Credential::ApiKey {
                    key: key.into(),
                    label: None,
                    created_at: None,
                },
            )
            .unwrap();
        store
    }

    fn store_with_oauth(dir: &std::path::Path, expires: i64) -> AuthStore {
        let store = AuthStore::with_path(dir.join("auth.json"));
        store
            .set(
                "opencode-go",
                Credential::try_from_value(&serde_json::json!({
                    "type":"oauth","access":"old-access","refresh":"old-refresh","expires":expires
                }))
                .unwrap(),
            )
            .unwrap();
        store
    }

    async fn mount_token_success(server: &MockServer, access: &str) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": access, "refresh_token": "new-refresh", "expires_in": 600,
                "org_id": "org_1" })),
            )
            .mount(server)
            .await;
    }

    /// The provider's own base_url is unreachable so the chat call never reaches the token
    /// server; only the refresh request is observable there.
    const DEAD_PROVIDER_BASE: &str = "http://127.0.0.1:9";

    #[tokio::test]
    async fn oauth_credentials_produce_a_refreshing_provider() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_oauth(dir.path(), agentj_auth::oauth::now_epoch() - 1);
        let server = MockServer::start().await;
        mount_token_success(&server, "new-access").await;

        let cfg = config_with_opencode_go(DEAD_PROVIDER_BASE);
        let asm = build_providers(&cfg, None, &store, || async { Ok(None) }, &server.uri())
            .await
            .unwrap();
        let provider = asm.providers[0].1.clone();
        assert!(!provider.models().is_empty());
        let _ = provider
            .chat_stream(vec![], vec![], ModelOptions::default())
            .await;
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "the decorator refreshes once before the first chat"
        );
    }

    #[test]
    fn builtin_opencode_go_entry_is_appended_when_config_has_none() {
        let cfgs = effective_provider_cfgs(&config_with(vec![provider("mock", "mock", "")]));
        let og: Vec<_> = cfgs.iter().filter(|c| c.kind == "opencode-go").collect();
        assert_eq!(og.len(), 1);
        assert_eq!(og[0].base_url, "https://opencode.ai/zen/go");
        assert!(og[0].api_key_env.is_none());
    }

    #[test]
    fn explicit_opencode_go_entry_wins_over_the_builtin() {
        let explicit = provider_base(
            "opencode-go",
            "opencode-go",
            "AGENTJ_TEST_OG_ENV",
            "https://custom.invalid",
        );
        let cfgs = effective_provider_cfgs(&config_with(vec![explicit]));
        let og: Vec<_> = cfgs.iter().filter(|c| c.kind == "opencode-go").collect();
        assert_eq!(og.len(), 1, "the built-in entry must not be appended");
        assert_eq!(og[0].base_url, "https://custom.invalid");
        assert_eq!(og[0].api_key_env.as_deref(), Some("AGENTJ_TEST_OG_ENV"));
    }

    #[test]
    fn startup_and_service_derive_the_same_entry() {
        let config = config_with(vec![provider("mock", "mock", "")]);
        let from_startup = effective_provider_cfgs(&config);
        let from_service = service_cfgs(&config);
        assert_eq!(from_startup, from_service, "one derivation source (D5)");
    }

    #[tokio::test]
    async fn builtin_entry_without_a_key_warns_with_a_link_hint() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let asm = build_providers(
            &config_with(vec![provider("mock", "mock", "")]),
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        assert!(
            asm.warnings
                .iter()
                .any(|w| w.contains("订阅") && w.contains("API Key")),
            "{:?}",
            asm.warnings
        );
    }

    #[tokio::test]
    async fn stored_credential_registers_the_builtin_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_api_key(dir.path(), "sk-builtin");
        let asm = build_providers(
            &config_with(vec![provider("mock", "mock", "")]),
            None,
            &store,
            || async { Ok(None) },
            "https://opencode.ai/console",
        )
        .await
        .unwrap();
        let names: Vec<&str> = asm.providers.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"opencode-go"), "{:?}", names);
    }

    #[tokio::test]
    async fn api_key_credentials_do_not_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_api_key(dir.path(), "sk-k");
        let server = MockServer::start().await;
        let cfg = config_with_opencode_go(DEAD_PROVIDER_BASE);
        let asm = build_providers(&cfg, None, &store, || async { Ok(None) }, &server.uri())
            .await
            .unwrap();
        let provider = asm.providers[0].1.clone();
        let _ = provider
            .chat_stream(vec![], vec![], ModelOptions::default())
            .await;
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "an api_key credential must never hit the token endpoint"
        );
    }
}
