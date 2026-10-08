//! Runtime (de)registration of the `opencode-go` provider (design D1/G3): one construction
//! path shared by startup and the credential handlers, so a linked key takes effect without
//! restarting the kernel.

use std::sync::Arc;

use agentj_auth::{AuthStore, KeySource, resolve_credential};
use agentj_core::module::{ModelProvider, ProviderRegistry};
use agentj_model::catalog::ModelCatalog;
use agentj_model::factory::ProviderFactory;

use crate::provider_service::ProviderCfg;
use crate::refreshing_provider::{Builder as RefreshingBuilder, RefreshingProvider};

/// Everything the OAuth decorator needs: the store holding the rotating credential and the
/// console base whose `/auth/device/token` mints the next access token (design D8).
pub struct OAuthRefresh {
    pub store: AuthStore,
    pub console_base: String,
}

/// The opencode-go instance for one key (design D1/G3): `oauth` keeps the access token fresh
/// before every chat call, `None` builds a static (api_key / env) instance.
///
/// The injected catalog carries the routing base, so the configured `base_url` is what the
/// registered instance actually talks to.
pub fn build_opencode_go(
    cfg: &ProviderCfg,
    key: &str,
    mut catalog: ModelCatalog,
    oauth: Option<OAuthRefresh>,
) -> Arc<dyn ModelProvider> {
    catalog.base_url = cfg.base_url.clone();
    let Some(oauth) = oauth else {
        return Arc::new(RefreshingProvider::r#static(create(cfg, key, catalog)));
    };
    let build: RefreshingBuilder = {
        let catalog = catalog.clone();
        let name = cfg.name.clone();
        let base = cfg.base_url.clone();
        Arc::new(move |key: &str| {
            create(
                &ProviderCfg {
                    name: name.clone(),
                    kind: "opencode-go".into(),
                    base_url: base.clone(),
                    api_key_env: None,
                },
                key,
                catalog.clone(),
            )
        })
    };
    Arc::new(RefreshingProvider::oauth(
        build,
        oauth.store,
        &cfg.name,
        &oauth.console_base,
        key.to_string(),
    ))
}

/// Register (or replace) the opencode-go provider after a credential change (design G3),
/// so `session/run`, `model/list` and `auth/status` use the new key immediately.
pub fn activate_opencode_go(
    registry: &ProviderRegistry,
    cfg: &ProviderCfg,
    key: &str,
    catalog: ModelCatalog,
    oauth: Option<OAuthRefresh>,
) -> Result<(), String> {
    registry.register(
        cfg.name.clone(),
        build_opencode_go(cfg, key, catalog, oauth),
    );
    Ok(())
}

/// Converge the registration with the credential that still resolves (design D1): an
/// `api_key_env` key rebuilds the instance (startup's env precedence), nothing left
/// unregisters it, and a stored credential leaves the live instance untouched.
pub fn deactivate_opencode_go(
    registry: &ProviderRegistry,
    cfg: &ProviderCfg,
    store: &AuthStore,
    live: Option<Vec<String>>,
) -> Result<(), String> {
    let resolved = resolve_credential(&cfg.name, None, cfg.api_key_env.as_deref(), store)
        .map_err(|e| e.to_string())?;
    match resolved {
        Some(key) if key.source == KeySource::Env => {
            let catalog = merged_catalog(live);
            registry.register(
                cfg.name.clone(),
                build_opencode_go(cfg, &key.key, catalog, None),
            );
        }
        Some(_) => {}
        None => {
            registry.remove(&cfg.name);
        }
    }
    Ok(())
}

/// A non-blank key is guaranteed by `resolve_credential`, and the kind is a literal, so the
/// factory cannot fail here (same expectation as the startup assembly).
fn create(cfg: &ProviderCfg, key: &str, catalog: ModelCatalog) -> Arc<dyn ModelProvider> {
    ProviderFactory::create_with_catalog(
        "opencode-go",
        &cfg.name,
        &cfg.base_url,
        key.to_string(),
        catalog,
    )
    .expect("opencode-go provider")
}

/// Committed overlay ∪ live ids; an empty or failed live probe falls back to the overlay.
fn merged_catalog(live: Option<Vec<String>>) -> ModelCatalog {
    ModelCatalog::from_overlay_and_live(ModelCatalog::builtin(), live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_auth::credential::Credential;
    use agentj_core::types::ModelOptions;
    use wiremock::MockServer;

    /// A provider base that refuses connections, so only the mock server is observable.
    const DEAD_BASE: &str = "http://127.0.0.1:9";

    fn go_cfg(base_url: &str) -> ProviderCfg {
        ProviderCfg {
            name: "opencode-go".into(),
            kind: "opencode-go".into(),
            base_url: base_url.into(),
            api_key_env: None,
        }
    }

    fn go_cfg_with_env(base_url: &str, api_key_env: &str) -> ProviderCfg {
        ProviderCfg {
            api_key_env: Some(api_key_env.into()),
            ..go_cfg(base_url)
        }
    }

    fn catalog_of(ids: &[&str]) -> ModelCatalog {
        let live: Vec<String> = ids.iter().map(|id| (*id).to_string()).collect();
        ModelCatalog::from_overlay_and_live(ModelCatalog::builtin(), Some(live))
    }

    /// The facade routes by `options.model`, so every chat call names a catalog id.
    fn options_for(model: &str) -> ModelOptions {
        ModelOptions {
            model: model.into(),
            ..ModelOptions::default()
        }
    }

    async fn mount_chat_ok(server: &MockServer) {
        wiremock::Mock::given(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("data: [DONE]\n\n"),
            )
            .mount(server)
            .await;
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

    #[tokio::test]
    async fn activate_registers_a_provider_serving_the_catalog() {
        let reg = Arc::new(ProviderRegistry::new());
        let cfg = go_cfg("http://127.0.0.1:9");
        activate_opencode_go(
            &reg,
            &cfg,
            "sk-x",
            catalog_of(&["opencode-go/kimi-k3"]),
            None,
        )
        .unwrap();
        let p = reg.get("opencode-go").expect("registered");
        assert_eq!(p.models(), vec!["opencode-go/kimi-k3".to_string()]);
    }

    #[tokio::test]
    async fn deactivate_without_any_remaining_key_unregisters() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let reg = Arc::new(ProviderRegistry::new());
        activate_opencode_go(&reg, &go_cfg(DEAD_BASE), "sk-x", catalog_of(&[]), None).unwrap();
        deactivate_opencode_go(&reg, &go_cfg(DEAD_BASE), &store, None).unwrap();
        assert!(reg.get("opencode-go").is_none());
    }

    #[tokio::test]
    async fn registered_provider_receives_chat_requests() {
        let server = wiremock::MockServer::start().await; // stands in for the provider base
        mount_chat_ok(&server).await;
        let reg = ProviderRegistry::new();
        let cfg = go_cfg(&server.uri());
        activate_opencode_go(
            &reg,
            &cfg,
            "sk-x",
            catalog_of(&["opencode-go/kimi-k3"]),
            None,
        )
        .unwrap();
        let p = reg.get("opencode-go").unwrap();
        // The facade routes by `options.model`; the default (`deepseek-chat`) is not in the
        // test catalog, so the chat names the id the catalog actually holds.
        let _ = p
            .chat_stream(vec![], vec![], options_for("opencode-go/kimi-k3"))
            .await;
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "the runtime-registered instance is the one serving runs"
        );
    }

    #[test]
    fn concurrent_activation_leaves_one_entry() {
        let reg = Arc::new(ProviderRegistry::new());
        let cfg = go_cfg(DEAD_BASE);
        std::thread::scope(|s| {
            s.spawn(|| {
                activate_opencode_go(&reg, &cfg, "k1", catalog_of(&["opencode-go/a"]), None)
                    .unwrap()
            });
            s.spawn(|| {
                activate_opencode_go(&reg, &cfg, "k2", catalog_of(&["opencode-go/b"]), None)
                    .unwrap()
            });
            s.spawn(|| {
                let _ = reg.list_with_models();
            });
        });
        assert_eq!(reg.list().iter().filter(|n| *n == "opencode-go").count(), 1);
        let (_, models) = reg
            .list_with_models()
            .into_iter()
            .find(|(n, _)| n == "opencode-go")
            .expect("one entry");
        assert!(
            models == vec!["opencode-go/a".to_string()]
                || models == vec!["opencode-go/b".to_string()],
            "the surviving instance belongs to one of the two catalogs: {models:?}"
        );
    }

    #[tokio::test]
    async fn oauth_activation_refreshes_once_before_chatting() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_oauth(dir.path(), agentj_auth::oauth::now_epoch() - 1);
        let console = MockServer::start().await;
        mount_token_success(&console, "new-access").await;

        let reg = Arc::new(ProviderRegistry::new());
        activate_opencode_go(
            &reg,
            &go_cfg(DEAD_BASE),
            "old-access",
            catalog_of(&["opencode-go/kimi-k3"]),
            Some(OAuthRefresh {
                store,
                console_base: console.uri(),
            }),
        )
        .unwrap();
        let p = reg.get("opencode-go").expect("registered");
        let _ = p
            .chat_stream(vec![], vec![], options_for("opencode-go/kimi-k3"))
            .await;
        assert_eq!(
            console.received_requests().await.unwrap().len(),
            1,
            "the decorator refreshes exactly once before the first chat"
        );
    }

    #[tokio::test]
    async fn deactivate_rebuilds_from_the_env_key() {
        // SAFETY: unique test-local variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_DEACTIVATE_KEY", "sk-env-9") };
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let server = MockServer::start().await;
        mount_chat_ok(&server).await;
        let reg = ProviderRegistry::new();
        let cfg = go_cfg_with_env(&server.uri(), "AGENTJ_TEST_DEACTIVATE_KEY");
        activate_opencode_go(&reg, &cfg, "sk-stale", catalog_of(&["opencode-go/a"]), None).unwrap();

        deactivate_opencode_go(&reg, &cfg, &store, None).unwrap();

        let p = reg
            .get("opencode-go")
            .expect("still registered from the env key");
        // `live == None` rebuilds from the committed overlay, so the chat names an overlay id.
        let _ = p
            .chat_stream(vec![], vec![], options_for("opencode-go/kimi-k3"))
            .await;
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("authorization").unwrap(),
            "Bearer sk-env-9",
            "the rebuilt instance uses the env key, not the stale one"
        );
    }
}
