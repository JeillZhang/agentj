//! `RefreshingProvider`: keeps an OAuth-backed provider's access token fresh (design D8).
//! @since 0.27.0
//!
//! A kernel started once must keep working across refresh cycles: before every `chat` /
//! `chat_stream` the stored credential is resolved (rotating it when it is close to expiry),
//! and the inner provider is rebuilt **only** when the access token actually changed. Static
//! (api_key / flag / env) credentials pay zero cost.

use agentj_auth::ensure_access;
use agentj_auth::store::AuthStore;
use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
#[cfg(test)]
use agentj_core::sync::lock_or_recover;
use agentj_core::sync::{read_or_recover, write_or_recover};
use agentj_core::types::{ChatMessage, ChatResponse, ModelEvent, ModelOptions, ToolDefinition};
use futures::stream::BoxStream;
use std::sync::{Arc, RwLock};

/// Builds an inner provider for a given access key.
pub type Builder = Arc<dyn Fn(&str) -> Arc<dyn ModelProvider> + Send + Sync>;

/// Wraps a provider whose credential may rotate.
///
/// For OAuth the inner provider is built **lazily** on first use, so an initial
/// already-expired key never causes a wasted build and the builder is called only
/// with a freshly resolved access token.
pub struct RefreshingProvider {
    inner: RwLock<Option<Arc<dyn ModelProvider>>>,
    builder: Builder,
    store: Option<AuthStore>,
    provider: String,
    console_base: String,
    current_key: RwLock<String>,
}

impl RefreshingProvider {
    /// Static credential (api_key from flag/env, or an api_key file entry): no refresh.
    pub fn r#static(inner: Arc<dyn ModelProvider>) -> Self {
        Self {
            inner: RwLock::new(Some(inner)),
            builder: Arc::new(|_| unreachable!("static provider is built eagerly")),
            store: None,
            provider: String::new(),
            console_base: String::new(),
            current_key: RwLock::new(String::new()),
        }
    }

    /// OAuth credential: every chat call resolves (and if needed refreshes) first.
    pub fn oauth(
        builder: Builder,
        store: AuthStore,
        provider: &str,
        console_base: &str,
        key: String,
    ) -> Self {
        Self {
            inner: RwLock::new(None),
            builder,
            store: Some(store),
            provider: provider.to_string(),
            console_base: console_base.to_string(),
            current_key: RwLock::new(key),
        }
    }

    /// The inner provider, built lazily from the current key on first use.
    fn inner(&self) -> Arc<dyn ModelProvider> {
        if let Some(p) = read_or_recover(&self.inner).clone() {
            return p;
        }
        let built = (self.builder)(&read_or_recover(&self.current_key));
        *write_or_recover(&self.inner) = Some(built.clone());
        built
    }

    /// Resolve the credential and rebuild the inner provider iff the key changed.
    async fn ensure(&self) -> Result<(), ModelError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let resolved = ensure_access(store, &self.provider, &self.console_base)
            .await
            .map_err(|e| ModelError::Api {
                status: 401,
                message: format!("{e}（请运行 agentj auth login --device 重新登录）"),
            })?
            .ok_or_else(|| ModelError::Api {
                status: 401,
                message: "订阅凭据缺失（请运行 agentj auth login --device）".into(),
            })?;
        if resolved.key != *read_or_recover(&self.current_key) {
            let rebuilt = (self.builder)(&resolved.key);
            *write_or_recover(&self.inner) = Some(rebuilt);
            *write_or_recover(&self.current_key) = resolved.key;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelProvider for RefreshingProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        self.ensure().await?;
        self.inner().chat_stream(messages, tools, options).await
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        self.ensure().await?;
        self.inner().chat(messages, options).await
    }

    /// Reads the inner provider only — no refresh, no network.
    fn models(&self) -> Vec<String> {
        self.inner().models()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_auth::credential::Credential;

    /// Records chat calls so the tests can prove which instance served them.
    #[derive(Default)]
    struct Counting {
        hits: std::sync::Mutex<Vec<String>>,
        models: Vec<String>,
    }

    #[async_trait::async_trait]
    impl ModelProvider for Counting {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            lock_or_recover(&self.hits).push("chat".into());
            Ok(Box::pin(futures::stream::empty()))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            lock_or_recover(&self.hits).push("chat".into());
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
        fn models(&self) -> Vec<String> {
            self.models.clone()
        }
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

    async fn mount_token_success(server: &wiremock::MockServer, access: &str) {
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
    async fn oauth_credential_rotates_once_then_reuses_the_rebuilt_provider() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_oauth(dir.path(), agentj_auth::oauth::now_epoch() - 1);
        let server = wiremock::MockServer::start().await;
        mount_token_success(&server, "new-access").await;

        let built: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(Default::default());
        let last: Arc<std::sync::Mutex<Option<Arc<Counting>>>> = Arc::new(Default::default());
        let builder: Builder = {
            let built = built.clone();
            let last = last.clone();
            Arc::new(move |key: &str| {
                lock_or_recover(&built).push(key.to_string());
                let p = Arc::new(Counting::default());
                *lock_or_recover(&last) = Some(p.clone());
                p as Arc<dyn ModelProvider>
            })
        };

        let p =
            RefreshingProvider::oauth(builder, store, "opencode-go", &server.uri(), "acl".into());
        assert!(
            p.chat_stream(vec![], vec![], ModelOptions::default())
                .await
                .is_ok()
        );
        assert!(
            p.chat_stream(vec![], vec![], ModelOptions::default())
                .await
                .is_ok()
        );

        assert_eq!(lock_or_recover(&built).as_slice(), ["new-access"]);
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "one refresh only"
        );
        let counting = lock_or_recover(&last).as_ref().cloned().expect("counting");
        assert_eq!(
            lock_or_recover(&counting.hits).len(),
            2,
            "both chat calls are served by the rebuilt instance"
        );
    }

    #[tokio::test]
    async fn refresh_failure_yields_an_actionable_error_and_no_chat_call() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_oauth(dir.path(), agentj_auth::oauth::now_epoch() - 1);
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(
                wiremock::ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"})),
            )
            .mount(&server)
            .await;
        let built: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(Default::default());
        let builder: Builder = {
            let built = built.clone();
            Arc::new(move |k: &str| {
                lock_or_recover(&built).push(k.to_string());
                Arc::new(Counting::default()) as Arc<dyn ModelProvider>
            })
        };

        let p =
            RefreshingProvider::oauth(builder, store, "opencode-go", &server.uri(), "old".into());
        let err = match p.chat_stream(vec![], vec![], ModelOptions::default()).await {
            Err(e) => e,
            Ok(_) => panic!("expected a refresh failure"),
        };
        assert!(err.to_string().contains("auth login --device"), "got {err}");
        assert!(
            lock_or_recover(&built).is_empty(),
            "no provider may be built on a failed refresh"
        );
    }

    #[tokio::test]
    async fn static_provider_never_refreshes_and_is_built_eagerly() {
        let server = wiremock::MockServer::start().await;
        let inner = Arc::new(Counting {
            hits: Default::default(),
            models: vec!["m1".into()],
        });
        let p = RefreshingProvider::r#static(inner.clone());
        assert!(
            p.chat_stream(vec![], vec![], ModelOptions::default())
                .await
                .is_ok()
        );
        let _ = p.chat(vec![], ModelOptions::default()).await.unwrap();
        assert_eq!(p.models(), vec!["m1".to_string()]);
        assert_eq!(lock_or_recover(&inner.hits).len(), 2);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
