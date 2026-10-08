//! `CliProviderService` — the `agentj-ipc` `ProviderService` implementation that backs
//! `model/list`, `auth/status`, `provider/usage`, `auth/deviceStart` and `auth/devicePoll`
//! (design D4/D5/D6/D9). @since 0.27.0
//!
//! Owns credential resolution (env → file, OAuth-aware), the 300s usage cache, the 60s
//! catalog reuse window and the three-tier `model/list` fallback. It never emits a secret:
//! api_key credentials are reported through `redact()` and OAuth through `org_id`/`expires_at`.

use agentj_auth::credential::{Credential, redact};
use agentj_auth::ensure_access;
use agentj_auth::store::AuthStore;
use agentj_core::error::ModelError;
use agentj_core::module::ProviderRegistry;
use agentj_ipc::{DispatchError, ProviderService};
use agentj_model::catalog::ModelCatalog;
use agentj_model::usage as mu;
use agentj_protocol::provider::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::activation;

const USAGE_TTL_SECS: i64 = 300;
const MODEL_TTL_SECS: i64 = 60;
/// OAuth credentials are flagged `expiring_soon` below this many seconds of life.
const EXPIRING_SOON_SECS: i64 = 120;

fn now_epoch() -> i64 {
    agentj_auth::oauth::now_epoch()
}

/// One configured provider, projected from `config.model.providers`.
#[derive(Debug, Clone)]
pub struct ProviderCfg {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
}

/// The service handle injected into the IPC server (design D4).
pub struct CliProviderService {
    store: AuthStore,
    cfgs: HashMap<String, ProviderCfg>,
    default_provider: String,
    default_model: String,
    console_base: String,
    warn_percent: u8,
    registry: Arc<ProviderRegistry>,
    usage_cache: Mutex<HashMap<String, (ProviderUsageResult, i64)>>,
    model_cache: Mutex<HashMap<String, (ModelListResult, i64)>>,
}

impl CliProviderService {
    pub fn new(
        store: AuthStore,
        cfgs: Vec<ProviderCfg>,
        default_provider: String,
        console_base: String,
        warn_percent: u8,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self {
            store,
            cfgs: cfgs.into_iter().map(|c| (c.name.clone(), c)).collect(),
            default_provider,
            default_model: String::new(),
            console_base,
            warn_percent,
            registry,
            usage_cache: Mutex::new(HashMap::new()),
            model_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Set the configured default model id (reported by `model/list`).
    pub fn with_default_model(mut self, model: String) -> Self {
        self.default_model = model;
        self
    }

    fn cfg(&self, provider: Option<&str>) -> Result<&ProviderCfg, DispatchError> {
        let name = provider.unwrap_or(&self.default_provider);
        self.cfgs.get(name).ok_or_else(|| {
            let mut registered: Vec<&String> = self.cfgs.keys().collect();
            registered.sort();
            DispatchError::invalid_params_with_data(
                "InvalidParams",
                serde_json::json!({
                    "field": "provider",
                    "reason": "unknown_provider",
                    "registered": registered,
                }),
            )
        })
    }

    fn require_kind<'a>(
        &self,
        cfg: &'a ProviderCfg,
        capability: &str,
    ) -> Result<&'a ProviderCfg, DispatchError> {
        if cfg.kind == "opencode-go" {
            Ok(cfg)
        } else {
            Err(DispatchError::invalid_params_with_data(
                "InvalidParams",
                serde_json::json!({
                    "field": "provider",
                    "reason": "unsupported_provider",
                    "capability": capability,
                }),
            ))
        }
    }

    /// Credential precedence: provider `api_key_env` (non-empty) → file store (OAuth-aware).
    async fn key(
        &self,
        cfg: &ProviderCfg,
    ) -> Result<(Option<String>, CredentialSource), DispatchError> {
        if let Some(var) = cfg.api_key_env.as_deref()
            && let Ok(v) = std::env::var(var)
            && !v.trim().is_empty()
        {
            return Ok((Some(v.trim().to_string()), CredentialSource::Env));
        }
        let resolved = ensure_access(&self.store, &cfg.name, &self.console_base)
            .await
            .map_err(credential_error)?;
        // Defence in depth for the "never send an empty Bearer" invariant: an upstream that
        // answers 200 with an empty access_token must not become an empty Authorization header.
        Ok((
            resolved.map(|k| k.key).filter(|k| !k.is_empty()),
            CredentialSource::File,
        ))
    }

    /// Read-only credential resolution for `auth/status`: reports what is stored **without**
    /// rotating anything (a pure status call must never touch the network).
    fn key_read_only(
        &self,
        cfg: &ProviderCfg,
    ) -> Result<(Option<String>, CredentialSource), DispatchError> {
        if let Some(var) = cfg.api_key_env.as_deref()
            && let Ok(v) = std::env::var(var)
            && !v.trim().is_empty()
        {
            return Ok((Some(v.trim().to_string()), CredentialSource::Env));
        }
        let stored = self.store.get(&cfg.name).map_err(credential_error)?;
        let key = stored
            .as_ref()
            .and_then(|c| c.key().ok())
            .filter(|k| !k.is_empty())
            .map(str::to_string);
        Ok((key, CredentialSource::File))
    }

    fn cached_usage(&self, provider: &str, now: i64, refresh: bool) -> Option<ProviderUsageResult> {
        if refresh {
            return None;
        }
        self.usage_cache
            .lock()
            .unwrap()
            .get(provider)
            .filter(|(_, at)| now - at < USAGE_TTL_SECS)
            .map(|(v, _)| v.clone())
    }

    fn stale_usage(&self, provider: &str) -> Option<ProviderUsageResult> {
        self.usage_cache
            .lock()
            .unwrap()
            .get(provider)
            .map(|(v, _)| v.clone())
    }

    fn usage_error(
        provider: &str,
        warn_percent: u8,
        kind: &str,
        message: String,
        cached: Option<ProviderUsageResult>,
    ) -> ProviderUsageResult {
        match cached {
            Some(mut prev) => {
                prev.stale = true;
                prev.error = Some(ProviderUsageError {
                    kind: kind.into(),
                    message,
                });
                prev
            }
            None => ProviderUsageResult {
                provider: provider.into(),
                fetched_at: now_epoch(),
                stale: false,
                warn_percent,
                windows: vec![],
                error: Some(ProviderUsageError {
                    kind: kind.into(),
                    message,
                }),
            },
        }
    }

    pub async fn usage(
        &self,
        p: ProviderUsageParams,
    ) -> Result<ProviderUsageResult, DispatchError> {
        let cfg = self
            .require_kind(self.cfg(p.provider.as_deref())?, "usage")?
            .clone();
        let now = now_epoch();
        if let Some(hit) = self.cached_usage(&cfg.name, now, p.refresh == Some(true)) {
            return Ok(hit);
        }
        let (key, _source) = self.key(&cfg).await?;
        let result = match key.as_deref() {
            None => Self::usage_error(
                &cfg.name,
                self.warn_percent,
                "no_credential",
                "尚未登录：请运行 agentj auth login --device".into(),
                self.stale_usage(&cfg.name),
            ),
            Some(k) => match mu::fetch_usage(&cfg.base_url, Some(k)).await {
                Ok(w) => ProviderUsageResult {
                    provider: cfg.name.clone(),
                    fetched_at: now,
                    stale: false,
                    warn_percent: self.warn_percent,
                    windows: w.iter().map(map_window).collect(),
                    error: None,
                },
                Err(e) => Self::usage_error(
                    &cfg.name,
                    self.warn_percent,
                    usage_kind(&e),
                    usage_message(&e),
                    self.stale_usage(&cfg.name),
                ),
            },
        };
        self.usage_cache
            .lock()
            .unwrap()
            .insert(cfg.name.clone(), (result.clone(), now));
        Ok(result)
    }

    pub async fn auth_status(
        &self,
        p: AuthStatusParams,
    ) -> Result<AuthStatusResult, DispatchError> {
        let cfg = self.cfg(p.provider.as_deref())?.clone();
        // Pure read: reports credential presence, never rotates tokens (design D6/D8).
        let (key, source) = self.key_read_only(&cfg)?;
        let stored = self.store.get(&cfg.name).ok().flatten();
        let credential = match (&key, &stored) {
            // BUG-001: an `api_key_env` credential is effective on its own — it must be projected
            // as `api_key`/`env` even when no `auth.json` entry backs it (`stored == None`).
            (Some(k), _) if source == CredentialSource::Env => Some(CredentialInfo {
                kind: CredentialKind::ApiKey,
                source,
                key_hint: Some(redact(k)),
                expires_at: None,
                expiring_soon: false,
                org_id: None,
            }),
            (Some(k), Some(Credential::ApiKey { .. })) => Some(CredentialInfo {
                kind: CredentialKind::ApiKey,
                source,
                key_hint: Some(redact(k)),
                expires_at: None,
                expiring_soon: false,
                org_id: None,
            }),
            (
                _,
                Some(Credential::OAuth {
                    expires, org_id, ..
                }),
            ) => Some(CredentialInfo {
                kind: CredentialKind::OAuth,
                source,
                key_hint: None,
                expires_at: Some(*expires),
                expiring_soon: *expires - now_epoch() < EXPIRING_SOON_SECS,
                org_id: org_id.clone(),
            }),
            _ => None,
        };
        Ok(AuthStatusResult {
            provider: cfg.name.clone(),
            authenticated: key.is_some(),
            credential,
            models_count: self.models_count(&cfg.name),
        })
    }

    fn models_count(&self, provider: &str) -> u32 {
        self.registry
            .list_with_models()
            .into_iter()
            .find(|(n, _)| n == provider)
            .map(|(_, m)| m.len() as u32)
            .unwrap_or(0)
    }

    pub async fn model_list(&self, p: ModelListParams) -> Result<ModelListResult, DispatchError> {
        let cfg = self.cfg(p.provider.as_deref())?.clone();
        let now = now_epoch();
        if p.refresh != Some(true)
            && let Some((cached, at)) = self.model_cache.lock().unwrap().get(&cfg.name)
            && now - at < MODEL_TTL_SECS
        {
            return Ok(cached.clone());
        }
        let out = if cfg.kind != "opencode-go" {
            ModelListResult {
                provider: cfg.name.clone(),
                default_model: self.default_model(&cfg.name),
                source: ModelSource::Static,
                fetched_at: now,
                models: self
                    .static_models(&cfg)
                    .into_iter()
                    .map(|id| ModelEntry {
                        id,
                        verified: true,
                        format: cfg.kind.clone(),
                    })
                    .collect(),
            }
        } else {
            self.go_model_list(&cfg, now).await
        };
        self.model_cache
            .lock()
            .unwrap()
            .insert(cfg.name.clone(), (out.clone(), now));
        Ok(out)
    }

    fn default_model(&self, provider: &str) -> String {
        if self.default_model.is_empty() {
            provider.to_string()
        } else {
            self.default_model.clone()
        }
    }

    fn static_models(&self, cfg: &ProviderCfg) -> Vec<String> {
        self.registry
            .list_with_models()
            .into_iter()
            .find(|(n, _)| n == &cfg.name)
            .map(|(_, m)| m)
            .unwrap_or_default()
    }

    /// account → live ∩ overlay → overlay (design D5). Never returns an empty list.
    async fn go_model_list(&self, cfg: &ProviderCfg, now: i64) -> ModelListResult {
        let overlay = agentj_model::catalog::ModelCatalog::builtin();
        if let Ok((Some(key), _)) = self.key(cfg).await {
            let org = self
                .store
                .get(&cfg.name)
                .ok()
                .flatten()
                .and_then(|c| match c {
                    Credential::OAuth { org_id, .. } => org_id,
                    _ => None,
                });
            if let Ok(ids) = agentj_model::console_catalog::fetch_account_models(
                &self.console_base,
                &key,
                org.as_deref(),
            )
            .await
            {
                return ModelListResult {
                    provider: cfg.name.clone(),
                    default_model: self.default_model(&cfg.name),
                    source: ModelSource::Account,
                    fetched_at: now,
                    models: entries_of(&ids, &overlay),
                };
            }
        }
        if let Ok(live) = agentj_model::catalog_source::fetch_live_models(&cfg.base_url, None).await
        {
            let merged = agentj_model::catalog::ModelCatalog::from_overlay_and_live(
                overlay.clone(),
                Some(live),
            );
            return ModelListResult {
                provider: cfg.name.clone(),
                default_model: self.default_model(&cfg.name),
                source: ModelSource::Live,
                fetched_at: now,
                models: entries_of(&merged.ids(), &overlay),
            };
        }
        ModelListResult {
            provider: cfg.name.clone(),
            default_model: self.default_model(&cfg.name),
            source: ModelSource::Overlay,
            fetched_at: now,
            models: entries_of(&overlay.ids(), &overlay),
        }
    }

    pub async fn device_start(
        &self,
        p: AuthDeviceStartParams,
    ) -> Result<AuthDeviceStartResult, DispatchError> {
        let cfg = self
            .require_kind(self.cfg(p.provider.as_deref())?, "device_flow")?
            .clone();
        let code = agentj_auth::oauth::device_start(&self.console_base)
            .await
            .map_err(credential_error)?;
        Ok(AuthDeviceStartResult {
            provider: cfg.name,
            device_code: code.device_code,
            user_code: code.user_code,
            verification_uri: code.verification_uri,
            expires_in: code.expires_in,
            interval: code.interval,
        })
    }

    pub async fn device_poll(
        &self,
        p: AuthDevicePollParams,
    ) -> Result<AuthDevicePollResult, DispatchError> {
        let cfg = self
            .require_kind(self.cfg(p.provider.as_deref())?, "device_flow")?
            .clone();
        let outcome = agentj_auth::oauth::device_poll(&self.console_base, &p.device_code, 5)
            .await
            .map_err(credential_error)?;
        Ok(match outcome {
            agentj_auth::oauth::PollOutcome::Pending => {
                state(DeviceAuthState::Pending, None, None, None)
            }
            agentj_auth::oauth::PollOutcome::SlowDown { interval } => {
                state(DeviceAuthState::SlowDown, Some(interval), None, None)
            }
            agentj_auth::oauth::PollOutcome::Denied => {
                state(DeviceAuthState::Denied, None, None, None)
            }
            agentj_auth::oauth::PollOutcome::Expired => {
                state(DeviceAuthState::Expired, None, None, None)
            }
            agentj_auth::oauth::PollOutcome::Error(msg) => {
                state(DeviceAuthState::Error, None, Some(msg), None)
            }
            agentj_auth::oauth::PollOutcome::Authorized(t) => {
                self.authorized_login(&cfg, t).await?
            }
        })
    }

    /// Persist the credential the device flow minted and register `opencode-go` with it
    /// (design G3), so a completed login works without restarting the kernel.
    async fn authorized_login(
        &self,
        cfg: &ProviderCfg,
        t: agentj_auth::oauth::Tokens,
    ) -> Result<AuthDevicePollResult, DispatchError> {
        let access = t.access.clone();
        self.store
            .set(
                &cfg.name,
                Credential::OAuth {
                    access: t.access,
                    refresh: t.refresh,
                    expires: t.expires_at,
                    account_id: None,
                    org_id: t.org_id.clone(),
                },
            )
            .map_err(credential_error)?;
        let catalog = self.go_catalog_best_effort(cfg).await;
        activation::activate_opencode_go(
            &self.registry,
            cfg,
            &access,
            catalog,
            Some(activation::OAuthRefresh {
                store: self.store.clone(),
                console_base: self.console_base.clone(),
            }),
        )
        .map_err(activation_failed)?;
        Ok(state(
            DeviceAuthState::Authorized,
            None,
            None,
            Some(CredentialInfo {
                kind: CredentialKind::OAuth,
                source: CredentialSource::File,
                key_hint: None,
                expires_at: Some(t.expires_at),
                expiring_soon: false,
                org_id: t.org_id,
            }),
        ))
    }

    pub async fn set_api_key(
        &self,
        p: AuthSetApiKeyParams,
    ) -> Result<AuthSetApiKeyResult, DispatchError> {
        let cfg = self.cfg(p.provider.as_deref())?.clone();
        self.require_kind(&cfg, "api_key")?;
        let key = p.api_key.trim().to_string();
        if key.is_empty() {
            return Err(DispatchError::invalid_params_with_data(
                "api_key must not be blank",
                serde_json::json!({ "field": "api_key", "reason": "invalid_params" }),
            ));
        }
        // One upstream call decides all three outcomes (design D2/D4); the upstream body is
        // never echoed, only the kernel sentence for the decisive status.
        let account =
            agentj_model::console_catalog::fetch_account_models(&self.console_base, &key, None)
                .await;
        match account {
            Err(ModelError::Api { status: 401, .. }) => Ok(rejected(
                &cfg,
                "unauthorized",
                "API Key 无效或已被撤销（未保存）：请在 opencode.ai 控制台重新复制",
            )),
            Err(ModelError::Api { status: 403, .. }) => Ok(rejected(
                &cfg,
                "entitlement",
                "该工作区没有 OpenCode Go 订阅（未保存）",
            )),
            // 200 with account models ⇒ verified; timeout / 5xx / malformed / 200 without
            // go models ⇒ unverified (design D2) — the key is stored either way.
            Ok(ids) => self.save_link(&cfg, &key, p.label.clone(), Some(ids)).await,
            Err(_) => self.save_link(&cfg, &key, p.label.clone(), None).await,
        }
    }

    /// Persist the link, activate the provider and seed the catalog reuse window (design
    /// D2/D3/D7): `account` is `Some` only for a verified console catalog.
    async fn save_link(
        &self,
        cfg: &ProviderCfg,
        key: &str,
        label: Option<String>,
        account: Option<Vec<String>>,
    ) -> Result<AuthSetApiKeyResult, DispatchError> {
        let (catalog, source) = self.link_catalog(cfg, account).await;
        let now = now_epoch();
        self.store
            .set(
                &cfg.name,
                Credential::ApiKey {
                    key: key.to_string(),
                    label,
                    created_at: Some(now.to_string()),
                },
            )
            .map_err(credential_error)?;
        activation::activate_opencode_go(&self.registry, cfg, key, catalog.clone(), None)
            .map_err(activation_failed)?;
        let models = entries_of(&catalog.ids(), &catalog);
        let cached = ModelListResult {
            provider: cfg.name.clone(),
            default_model: self.default_model(&cfg.name),
            source,
            fetched_at: now,
            models: models.clone(),
        };
        self.model_cache
            .lock()
            .unwrap()
            .insert(cfg.name.clone(), (cached, now));
        Ok(link_result(cfg, key, source, models, now))
    }

    /// The catalog a link reports (design D2): the verified account ids, else the live probe,
    /// else the committed overlay. It never fabricates ids and is never empty.
    async fn link_catalog(
        &self,
        cfg: &ProviderCfg,
        account: Option<Vec<String>>,
    ) -> (ModelCatalog, ModelSource) {
        let overlay = ModelCatalog::builtin();
        if let Some(ids) = account {
            return (
                ModelCatalog::from_overlay_and_live(overlay, Some(ids)),
                ModelSource::Account,
            );
        }
        match agentj_model::catalog_source::fetch_live_models(&cfg.base_url, None).await {
            Ok(live) => (
                ModelCatalog::from_overlay_and_live(overlay, Some(live)),
                ModelSource::Live,
            ),
            Err(_) => (overlay, ModelSource::Overlay),
        }
    }

    /// The catalog an activation uses (design D3): the live ∩ overlay ids, else the committed
    /// overlay. It never fabricates an id and never fails the caller.
    async fn go_catalog_best_effort(&self, cfg: &ProviderCfg) -> ModelCatalog {
        self.link_catalog(cfg, None).await.0
    }

    /// Remove the stored credential and converge the registration with what still resolves
    /// (design D7): both per-provider caches are invalidated, then the `opencode-go` instance
    /// is rebuilt from a remaining env/flag key or unregistered when nothing is left.
    pub async fn clear_credential(
        &self,
        p: AuthClearCredentialParams,
    ) -> Result<AuthClearCredentialResult, DispatchError> {
        let cfg = self.cfg(p.provider.as_deref())?.clone();
        let cleared = self.store.remove(&cfg.name).map_err(credential_error)?;
        self.usage_cache.lock().unwrap().remove(&cfg.name);
        self.model_cache.lock().unwrap().remove(&cfg.name);
        activation::deactivate_opencode_go(&self.registry, &cfg, &self.store, None)
            .map_err(activation_failed)?;
        Ok(AuthClearCredentialResult {
            provider: cfg.name,
            cleared,
        })
    }
}

#[async_trait::async_trait]
impl ProviderService for CliProviderService {
    async fn model_list(&self, p: ModelListParams) -> Result<ModelListResult, DispatchError> {
        CliProviderService::model_list(self, p).await
    }
    async fn auth_status(&self, p: AuthStatusParams) -> Result<AuthStatusResult, DispatchError> {
        CliProviderService::auth_status(self, p).await
    }
    async fn usage(&self, p: ProviderUsageParams) -> Result<ProviderUsageResult, DispatchError> {
        CliProviderService::usage(self, p).await
    }
    async fn device_start(
        &self,
        p: AuthDeviceStartParams,
    ) -> Result<AuthDeviceStartResult, DispatchError> {
        CliProviderService::device_start(self, p).await
    }
    async fn device_poll(
        &self,
        p: AuthDevicePollParams,
    ) -> Result<AuthDevicePollResult, DispatchError> {
        CliProviderService::device_poll(self, p).await
    }
    async fn set_api_key(
        &self,
        p: AuthSetApiKeyParams,
    ) -> Result<AuthSetApiKeyResult, DispatchError> {
        CliProviderService::set_api_key(self, p).await
    }
    async fn clear_credential(
        &self,
        p: AuthClearCredentialParams,
    ) -> Result<AuthClearCredentialResult, DispatchError> {
        CliProviderService::clear_credential(self, p).await
    }
}

/// A `rejected` link (design D2): nothing was written or registered, so `effective_source`
/// stays `file` — a failed attempt leaves the resolution precedence untouched. `kind` is
/// one of the two emitted [`ApiKeyLinkError`] values and `message` is kernel-authored; the
/// upstream response body must never reach the client through either field.
fn rejected(cfg: &ProviderCfg, kind: &str, message: &str) -> AuthSetApiKeyResult {
    AuthSetApiKeyResult {
        provider: cfg.name.clone(),
        saved: false,
        outcome: ApiKeyOutcome::Rejected,
        error: Some(ApiKeyLinkError {
            kind: kind.into(),
            message: message.into(),
        }),
        credential: None,
        source: None,
        effective_source: CredentialSource::File,
        fetched_at: now_epoch(),
        models: vec![],
    }
}

/// The successful-link payload (design D2/D3): the redacted hint plus the catalog tier.
fn link_result(
    cfg: &ProviderCfg,
    key: &str,
    source: ModelSource,
    models: Vec<ModelEntry>,
    now: i64,
) -> AuthSetApiKeyResult {
    let effective = effective_source(cfg);
    AuthSetApiKeyResult {
        provider: cfg.name.clone(),
        saved: true,
        outcome: if source == ModelSource::Account {
            ApiKeyOutcome::Verified
        } else {
            ApiKeyOutcome::Unverified
        },
        error: None,
        credential: Some(CredentialInfo {
            kind: CredentialKind::ApiKey,
            source: effective,
            key_hint: Some(redact(key)),
            expires_at: None,
            expiring_soon: false,
            org_id: None,
        }),
        source: Some(source),
        effective_source: effective,
        fetched_at: now,
        models,
    }
}

/// D3/G7: a non-blank `api_key_env` shadows the entry just written, so the result reports
/// `env` instead of claiming the new credential is the effective one.
fn effective_source(cfg: &ProviderCfg) -> CredentialSource {
    if let Some(var) = cfg.api_key_env.as_deref()
        && let Ok(v) = std::env::var(var)
        && !v.trim().is_empty()
    {
        return CredentialSource::Env;
    }
    CredentialSource::File
}

/// A registration failure after the credential is already on disk (design D2): `-32603` with
/// a machine-readable reason; the save stays on disk, so a retry is meaningful.
fn activation_failed(reason: String) -> DispatchError {
    DispatchError::internal_with_data(
        "provider activation failed",
        serde_json::json!({ "reason": "activation_failed", "detail": reason }),
    )
}

fn state(
    state: DeviceAuthState,
    interval: Option<u64>,
    message: Option<String>,
    credential: Option<CredentialInfo>,
) -> AuthDevicePollResult {
    AuthDevicePollResult {
        state,
        interval,
        message,
        credential,
    }
}

fn credential_error(e: agentj_auth::credential::AuthError) -> DispatchError {
    DispatchError::internal_with_data(
        "CredentialError",
        serde_json::json!({ "reason": e.to_string() }),
    )
}

fn usage_kind(e: &mu::UsageError) -> &'static str {
    match e {
        mu::UsageError::NoCredential => "no_credential",
        mu::UsageError::Expired => "expired",
        mu::UsageError::Unauthorized => "unauthorized",
        mu::UsageError::Entitlement => "entitlement",
        mu::UsageError::Unavailable => "unavailable",
        mu::UsageError::Network(_) => "network",
    }
}

fn usage_message(e: &mu::UsageError) -> String {
    match e {
        mu::UsageError::NoCredential => "尚未登录：请运行 agentj auth login --device".into(),
        mu::UsageError::Expired => "订阅凭据已失效：请重新登录".into(),
        mu::UsageError::Unauthorized => "密钥无效或已被撤销：请重新登录".into(),
        mu::UsageError::Entitlement => "该工作区没有 OpenCode Go 订阅".into(),
        mu::UsageError::Unavailable => "用量暂时不可用（上游响应无法识别）".into(),
        mu::UsageError::Network(reason) => format!("网络不可达：{reason}"),
    }
}

fn map_window(w: &mu::UsageWindow) -> UsageWindow {
    UsageWindow {
        kind: match w.kind {
            mu::UsageWindowKind::Rolling => UsageWindowKind::Rolling,
            mu::UsageWindowKind::Weekly => UsageWindowKind::Weekly,
            mu::UsageWindowKind::Monthly => UsageWindowKind::Monthly,
        },
        status: match w.status {
            mu::UsageStatus::Ok => UsageWindowStatus::Ok,
            mu::UsageStatus::RateLimited => UsageWindowStatus::RateLimited,
            mu::UsageStatus::Unknown => UsageWindowStatus::Unknown,
        },
        percent: w.percent,
        resets_at: w.resets_at.clone(),
    }
}

fn format_name(f: agentj_model::catalog::WireFormat) -> String {
    match f {
        agentj_model::catalog::WireFormat::OpenAiCompat => "openai-compat",
        agentj_model::catalog::WireFormat::Responses => "responses",
        agentj_model::catalog::WireFormat::Anthropic => "anthropic",
    }
    .into()
}

/// Maps ids through the overlay: a hit keeps its format/verified; a miss is synthesized as
/// `openai-compat` + `verified: false` (phase-1 D3 — never guess a route from the id).
fn entries_of(ids: &[String], overlay: &agentj_model::catalog::ModelCatalog) -> Vec<ModelEntry> {
    ids.iter()
        .map(|id| match overlay.lookup(id) {
            Some(e) => ModelEntry {
                id: id.clone(),
                verified: e.verified,
                format: format_name(e.format),
            },
            None => ModelEntry {
                id: id.clone(),
                verified: false,
                format: "openai-compat".into(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ModelError;
    use agentj_core::module::ModelProvider;
    use agentj_core::types::{ChatMessage, ChatResponse, ModelEvent, ModelOptions, ToolDefinition};
    use futures::stream::BoxStream;

    /// Registry stub reporting a fixed model list.
    struct StubProvider(Vec<String>);

    #[async_trait::async_trait]
    impl ModelProvider for StubProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            Ok(Box::pin(futures::stream::empty()))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            Err(ModelError::Stream("stub".into()))
        }
        fn models(&self) -> Vec<String> {
            self.0.clone()
        }
    }

    fn empty_registry() -> Arc<ProviderRegistry> {
        Arc::new(ProviderRegistry::new())
    }

    fn registry_with(names: &[&str]) -> Arc<ProviderRegistry> {
        let reg = ProviderRegistry::new();
        for n in names {
            reg.register(
                (*n).to_string(),
                Arc::new(StubProvider(vec![format!("{n}-model")])),
            );
        }
        Arc::new(reg)
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

    async fn mount_usage(server: &wiremock::MockServer, status: u16, body: serde_json::Value) {
        wiremock::Mock::given(wiremock::matchers::path("/v1/usage"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    fn svc(store: AuthStore, base: &str) -> CliProviderService {
        CliProviderService::new(
            store,
            vec![ProviderCfg {
                name: "opencode-go".into(),
                kind: "opencode-go".into(),
                base_url: base.into(),
                api_key_env: None,
            }],
            "opencode-go".into(),
            base.into(),
            80,
            empty_registry(),
        )
    }

    fn go_cfg(base: &str) -> ProviderCfg {
        ProviderCfg {
            name: "opencode-go".into(),
            kind: "opencode-go".into(),
            base_url: base.into(),
            api_key_env: None,
        }
    }

    /// A service over `cfgs` whose provider base and console base are both `console`, plus
    /// the temp dir holding its (initially absent) `auth.json`.
    fn console_service(
        cfgs: Vec<ProviderCfg>,
        default: &str,
        console: &str,
    ) -> (CliProviderService, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let svc = CliProviderService::new(
            store,
            cfgs,
            default.into(),
            console.into(),
            80,
            empty_registry(),
        );
        (svc, dir)
    }

    fn svc_with_console(console: &str) -> (CliProviderService, tempfile::TempDir) {
        console_service(vec![go_cfg(console)], "opencode-go", console)
    }

    /// The same service with `api_key_env` pointing at a (test-local) variable, so a stored
    /// link and an env key can be observed shadowing each other.
    fn svc_with_console_and_env(
        console: &str,
        env_var: &str,
    ) -> (CliProviderService, tempfile::TempDir) {
        console_service(
            vec![ProviderCfg {
                api_key_env: Some(env_var.into()),
                ..go_cfg(console)
            }],
            "opencode-go",
            console,
        )
    }

    /// Permission bits of `path`, for the `auth.json` mode assertion.
    fn file_mode(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn params(api_key: &str, provider: Option<&str>) -> AuthSetApiKeyParams {
        AuthSetApiKeyParams {
            provider: provider.map(str::to_string),
            api_key: api_key.into(),
            label: None,
        }
    }

    #[tokio::test]
    async fn usage_is_cached_and_refresh_bypasses_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        mount_usage(
            &server,
            200,
            serde_json::json!({"usage":{
                "rolling":{"status":"ok","percent":4,"resetsAt":"2026-08-13T16:27:38Z"},
                "weekly":{"status":"ok","percent":3,"resetsAt":"2026-08-17T00:00:00Z"},
                "monthly":{"status":"ok","percent":0,"resetsAt":"x"}}}),
        )
        .await;
        let s = svc(
            store_with_api_key(dir.path(), "sk-test-1234"),
            &server.uri(),
        );

        let first = s
            .usage(ProviderUsageParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(
            (first.provider.as_str(), first.windows.len(), first.stale),
            ("opencode-go", 3, false)
        );
        assert_eq!(first.warn_percent, 80);
        let _ = s
            .usage(ProviderUsageParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "cache hit must not refetch"
        );
        let _ = s
            .usage(ProviderUsageParams {
                provider: None,
                refresh: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "refresh bypasses the cache"
        );
    }

    #[tokio::test]
    async fn usage_maps_domain_errors_and_never_calls_upstream_without_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        mount_usage(
            &server,
            403,
            serde_json::json!({"type":"error","error":{"type":"EntitlementError","message":"m"}}),
        )
        .await;
        let s = svc(store_with_api_key(dir.path(), "sk-k"), &server.uri());
        let r = s
            .usage(ProviderUsageParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(r.error.unwrap().kind, "entitlement");

        let empty = tempfile::tempdir().unwrap();
        let s = svc(
            AuthStore::with_path(empty.path().join("auth.json")),
            &server.uri(),
        );
        let before = server.received_requests().await.unwrap().len();
        let r = s
            .usage(ProviderUsageParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(r.error.unwrap().kind, "no_credential");
        assert_eq!(server.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn unusable_provider_names_are_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let s = svc(
            AuthStore::with_path(dir.path().join("auth.json")),
            "https://example.invalid",
        );
        let e = s
            .usage(ProviderUsageParams {
                provider: Some("nope".into()),
                refresh: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.data.as_ref().unwrap()["reason"].as_str().unwrap()),
            (-32602, "unknown_provider")
        );

        // A registered provider that does not implement the capability.
        let dir2 = tempfile::tempdir().unwrap();
        let s2 = CliProviderService::new(
            AuthStore::with_path(dir2.path().join("auth.json")),
            vec![ProviderCfg {
                name: "deepseek".into(),
                kind: "openai-compat".into(),
                base_url: "https://example.invalid".into(),
                api_key_env: None,
            }],
            "deepseek".into(),
            "https://opencode.ai/console".into(),
            80,
            empty_registry(),
        );
        let e = s2
            .usage(ProviderUsageParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.data.as_ref().unwrap()["reason"].as_str().unwrap()),
            (-32602, "unsupported_provider")
        );
    }

    #[tokio::test]
    async fn blank_key_is_invalid_params_without_upstream_call() {
        let server = wiremock::MockServer::start().await;
        let (svc, dir) = svc_with_console(&server.uri());
        let err = svc.set_api_key(params("   ", None)).await.unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data.unwrap()["reason"], "invalid_params");
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(!dir.path().join("auth.json").exists());
    }

    #[tokio::test]
    async fn rejected_key_writes_nothing_and_calls_upstream_once() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 401, serde_json::json!({"_tag":"Unauthorized"})).await;
        let (svc, dir) = svc_with_console(&server.uri());
        let out = svc.set_api_key(params("sk-bad", None)).await.unwrap();
        assert!(!out.saved);
        assert!(matches!(out.outcome, ApiKeyOutcome::Rejected));
        let err = out.error.unwrap();
        assert_eq!(err.kind, "unauthorized");
        assert!(
            !err.message.contains("Unauthorized") && !err.message.contains("_tag"),
            "no upstream body"
        );
        assert!(!dir.path().join("auth.json").exists());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_provider_reports_the_effective_names_sorted() {
        let server = wiremock::MockServer::start().await;
        let (svc, _dir) = console_service(
            vec![
                go_cfg("https://example.invalid"),
                ProviderCfg {
                    name: "deepseek-chat".into(),
                    kind: "openai-compat".into(),
                    base_url: "https://example.invalid".into(),
                    api_key_env: None,
                },
            ],
            "opencode-go",
            &server.uri(),
        );
        let err = svc
            .set_api_key(params("sk-k", Some("nope")))
            .await
            .unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.data.unwrap()["registered"],
            serde_json::json!(["deepseek-chat", "opencode-go"])
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn non_go_provider_is_unsupported_for_api_key_linking() {
        let server = wiremock::MockServer::start().await;
        let (svc, _dir) = console_service(
            vec![ProviderCfg {
                name: "deepseek-chat".into(),
                kind: "openai-compat".into(),
                base_url: "https://example.invalid".into(),
                api_key_env: None,
            }],
            "deepseek-chat",
            &server.uri(),
        );
        let err = svc
            .set_api_key(params("sk-k", Some("deepseek-chat")))
            .await
            .unwrap_err();
        let data = err.data.unwrap();
        assert_eq!(err.code, -32602);
        assert_eq!(data["reason"], "unsupported_provider");
        assert_eq!(data["capability"], "api_key");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn forbidden_key_is_rejected_with_entitlement_and_writes_nothing() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 403, serde_json::json!({"error":"no_subscription"})).await;
        let (svc, dir) = svc_with_console(&server.uri());
        let out = svc
            .set_api_key(params("sk-nosub", Some("opencode-go")))
            .await
            .unwrap();
        assert!(!out.saved);
        assert!(matches!(out.outcome, ApiKeyOutcome::Rejected));
        assert_eq!(out.error.unwrap().kind, "entitlement");
        assert!(!dir.path().join("auth.json").exists());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    /// `params` with a stored label and the default provider (the plan's `Some("web")`
    /// stands for the client-supplied label, not a provider name).
    fn labelled_params(api_key: &str, label: &str) -> AuthSetApiKeyParams {
        AuthSetApiKeyParams {
            provider: None,
            api_key: api_key.into(),
            label: Some(label.into()),
        }
    }

    #[tokio::test]
    async fn verified_link_saves_caches_and_activates() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 200, v2_body(&["kimi-k3", "minimax-m3"])).await;
        let (svc, dir) = svc_with_console(&server.uri());
        let out = svc
            .set_api_key(labelled_params("sk-good", "web"))
            .await
            .unwrap();
        assert!(out.saved && matches!(out.outcome, ApiKeyOutcome::Verified));
        assert_eq!(out.source, Some(ModelSource::Account));
        assert_eq!(
            out.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["opencode-go/kimi-k3", "opencode-go/minimax-m3"]
        );
        assert_eq!(file_mode(&dir.path().join("auth.json")), 0o600);
        assert!(!serde_json::to_string(&out).unwrap().contains("sk-good"));
        assert!(svc.registry.get("opencode-go").is_some());
        assert!(matches!(
            svc.store.get("opencode-go").unwrap().unwrap(),
            Credential::ApiKey { key, label: Some(l), .. } if key == "sk-good" && l == "web"
        ));
    }

    #[tokio::test]
    async fn unverified_after_500_still_saves_and_never_invents_catalog() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 500, serde_json::json!({})).await;
        mount_live_models(&server, 200, &["kimi-k3"]).await;
        let (svc, dir) = svc_with_console(&server.uri());
        let out = svc.set_api_key(params("sk-maybe", None)).await.unwrap();
        assert!(out.saved && matches!(out.outcome, ApiKeyOutcome::Unverified));
        assert_eq!(out.source, Some(ModelSource::Live));
        assert_eq!(out.models.len(), 1);
        assert!(dir.path().join("auth.json").exists());
    }

    #[tokio::test]
    async fn env_key_shadows_the_stored_credential() {
        // SAFETY: unique test-local variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_SHADOW_KEY", "env-key") };
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 200, v2_body(&["kimi-k3"])).await;
        let (svc, _dir) = svc_with_console_and_env(&server.uri(), "AGENTJ_TEST_SHADOW_KEY");
        let out = svc.set_api_key(params("sk-good", None)).await.unwrap();
        assert_eq!(out.effective_source, CredentialSource::Env);
        assert!(out.saved);
        unsafe { std::env::remove_var("AGENTJ_TEST_SHADOW_KEY") };
    }

    #[tokio::test]
    async fn verified_link_caches_the_account_catalog_and_reports_a_hint() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 200, v2_body(&["kimi-k3", "minimax-m3"])).await;
        let (svc, _dir) = svc_with_console(&server.uri());
        let out = svc.set_api_key(params("sk-good", None)).await.unwrap();
        let calls = server.received_requests().await.unwrap().len();
        let cached = svc
            .model_list(ModelListParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(cached.source, ModelSource::Account);
        assert_eq!(ids(&cached.models), ids(&out.models));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            calls,
            "a link seeds the 60s catalog reuse window"
        );

        let status = svc
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap();
        assert!(status.authenticated);
        let c = status.credential.unwrap();
        assert_eq!(
            (c.kind, c.source, c.key_hint.as_deref()),
            (
                CredentialKind::ApiKey,
                CredentialSource::File,
                Some("…good")
            )
        );
    }

    #[tokio::test]
    async fn unverified_falls_back_to_the_overlay_when_live_also_fails() {
        let server = wiremock::MockServer::start().await;
        mount_v2_config(&server, 500, serde_json::json!({})).await;
        mount_live_models(&server, 500, &[]).await;
        let (svc, dir) = svc_with_console(&server.uri());
        let out = svc.set_api_key(params("sk-maybe", None)).await.unwrap();
        assert!(out.saved && matches!(out.outcome, ApiKeyOutcome::Unverified));
        assert_eq!(out.source, Some(ModelSource::Overlay));
        assert!(!out.models.is_empty(), "the overlay catalog is never empty");
        assert!(dir.path().join("auth.json").exists());
    }

    #[tokio::test]
    async fn omitted_provider_resolves_to_the_configured_default() {
        // The configured default is a non-go provider, so an omitted `provider` must reach
        // *its* kind check rather than fail as an unknown name.
        let server = wiremock::MockServer::start().await;
        let (svc, _dir) = console_service(
            vec![
                go_cfg("https://example.invalid"),
                ProviderCfg {
                    name: "deepseek-chat".into(),
                    kind: "openai-compat".into(),
                    base_url: "https://example.invalid".into(),
                    api_key_env: None,
                },
            ],
            "deepseek-chat",
            &server.uri(),
        );
        let err = svc.set_api_key(params("sk-k", None)).await.unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(err.data.unwrap()["reason"], "unsupported_provider");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn auth_status_uses_real_source_and_redact_format() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        let s = svc(
            store_with_api_key(dir.path(), "sk-test-1234"),
            &server.uri(),
        );
        let r = s
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap();
        assert!(r.authenticated);
        let c = r.credential.clone().unwrap();
        assert_eq!(
            (c.kind, c.source, c.key_hint.as_deref()),
            (
                CredentialKind::ApiKey,
                CredentialSource::File,
                Some("…1234")
            )
        );
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "status must not touch the network"
        );
        assert!(
            !serde_json::to_string(&r).unwrap().contains("sk-test-1234"),
            "full key leaked"
        );

        let empty = tempfile::tempdir().unwrap();
        let s = svc(
            AuthStore::with_path(empty.path().join("auth.json")),
            &server.uri(),
        );
        let r = s
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap();
        assert!(!r.authenticated && r.credential.is_none());
    }

    #[tokio::test]
    async fn auth_status_reports_oauth_lifecycle_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let expires = agentj_auth::oauth::now_epoch() + 60;
        store
            .set(
                "opencode-go",
                Credential::try_from_value(&serde_json::json!({
                    "type":"oauth","access":"a","refresh":"r","expires":expires,"org_id":"org_1"
                }))
                .unwrap(),
            )
            .unwrap();
        let s = svc(store, "https://example.invalid");
        let c = s
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap()
            .credential
            .unwrap();
        assert_eq!(c.kind, CredentialKind::OAuth);
        assert!(c.key_hint.is_none());
        assert_eq!(c.expires_at, Some(expires));
        assert_eq!(c.org_id.as_deref(), Some("org_1"));
        assert!(c.expiring_soon, "60s of life is inside the 120s window");
    }

    #[tokio::test]
    async fn env_key_wins_over_the_stored_credential() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        // SAFETY: single-threaded test-local env mutation with a unique variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_SUB_KEY", "env-key") };
        let s = CliProviderService::new(
            store_with_api_key(dir.path(), "sk-file"),
            vec![ProviderCfg {
                name: "opencode-go".into(),
                kind: "opencode-go".into(),
                base_url: server.uri(),
                api_key_env: Some("AGENTJ_TEST_SUB_KEY".into()),
            }],
            "opencode-go".into(),
            server.uri(),
            80,
            empty_registry(),
        );
        let r = s
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap();
        assert_eq!(r.credential.unwrap().source, CredentialSource::Env);
    }

    /// BUG-001: an env-only credential (`api_key_env` set, no `auth.json` entry) must still be
    /// projected as `kind: api_key` / `source: env` / redacted `key_hint`; omitting it makes the
    /// client fall back to a generic "authenticated" state and lose the `kind` / `source` hint.
    #[tokio::test]
    async fn auth_status_reports_env_credential_without_stored_entry() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        // SAFETY: single-threaded test-local env mutation with a unique variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_ENV_ONLY_KEY", "sk-envonly-9876") };
        let s = CliProviderService::new(
            // No `auth.json` entry at all: the credential comes solely from the env var.
            AuthStore::with_path(dir.path().join("auth.json")),
            vec![ProviderCfg {
                name: "opencode-go".into(),
                kind: "opencode-go".into(),
                base_url: server.uri(),
                api_key_env: Some("AGENTJ_TEST_ENV_ONLY_KEY".into()),
            }],
            "opencode-go".into(),
            server.uri(),
            80,
            empty_registry(),
        );
        let r = s
            .auth_status(AuthStatusParams { provider: None })
            .await
            .unwrap();
        assert!(r.authenticated);
        let c = r.credential.expect("env-only credential must be reported");
        assert_eq!(
            (c.kind, c.source, c.key_hint.as_deref()),
            (CredentialKind::ApiKey, CredentialSource::Env, Some("…9876"))
        );
        assert!(!c.expiring_soon);
        unsafe { std::env::remove_var("AGENTJ_TEST_ENV_ONLY_KEY") };
    }

    fn ids(entries: &[ModelEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.id.as_str()).collect()
    }

    fn v2_body(ids: &[&str]) -> serde_json::Value {
        let models: serde_json::Map<String, serde_json::Value> = ids
            .iter()
            .map(|id| ((*id).to_string(), serde_json::json!({})))
            .collect();
        serde_json::json!({"providers":{
            "opencode":{"name":"opencode","settings":{"baseURL":"https://opencode.ai/zen/go"},
                        "models":models},
            "anthropic":{"name":"Anthropic","settings":{"baseURL":"https://api.anthropic.com"},
                         "models":{"claude-x":{}}}}})
    }

    async fn mount_v2_config(server: &wiremock::MockServer, status: u16, body: serde_json::Value) {
        wiremock::Mock::given(wiremock::matchers::path("/api/v2/config"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_live_models(server: &wiremock::MockServer, status: u16, ids: &[&str]) {
        let body = serde_json::json!({
            "object":"list",
            "data": ids.iter().map(|i| serde_json::json!({"id": i})).collect::<Vec<_>>(),
        });
        wiremock::Mock::given(wiremock::matchers::path("/v1/models"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn model_list_prefers_account_then_live_then_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_api_key(dir.path(), "sk-k");

        let account_server = wiremock::MockServer::start().await;
        mount_v2_config(&account_server, 200, v2_body(&["kimi-k3", "brand-new-id"])).await;
        let s = svc(store.clone(), &account_server.uri());
        let account = s
            .model_list(ModelListParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(account.source, ModelSource::Account);
        assert_eq!(
            ids(&account.models),
            ["opencode-go/brand-new-id", "opencode-go/kimi-k3"]
        );
        let unknown = account
            .models
            .iter()
            .find(|m| m.id.ends_with("brand-new-id"))
            .unwrap();
        assert!(
            !unknown.verified && unknown.format == "openai-compat",
            "routing is never guessed"
        );
        let before = account_server.received_requests().await.unwrap().len();
        let _ = s
            .model_list(ModelListParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(
            account_server.received_requests().await.unwrap().len(),
            before,
            "60s reuse window"
        );
        let _ = s
            .model_list(ModelListParams {
                provider: None,
                refresh: Some(true),
            })
            .await
            .unwrap();
        assert!(
            account_server.received_requests().await.unwrap().len() > before,
            "refresh bypasses it"
        );

        let live_server = wiremock::MockServer::start().await;
        mount_v2_config(&live_server, 500, serde_json::json!({})).await;
        mount_live_models(&live_server, 200, &["kimi-k3"]).await;
        let s = svc(store.clone(), &live_server.uri());
        let live = s
            .model_list(ModelListParams {
                provider: None,
                refresh: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(live.source, ModelSource::Live);
        assert!(!live.models.is_empty());

        let dead_server = wiremock::MockServer::start().await;
        mount_v2_config(&dead_server, 500, serde_json::json!({})).await;
        mount_live_models(&dead_server, 500, &[]).await;
        let s = svc(store, &dead_server.uri());
        let overlay = s
            .model_list(ModelListParams {
                provider: None,
                refresh: Some(true),
            })
            .await
            .unwrap();
        assert_eq!(overlay.source, ModelSource::Overlay);
        assert!(
            !overlay.models.is_empty(),
            "the overlay catalog is never empty"
        );
    }

    #[tokio::test]
    async fn non_go_providers_are_static_and_offline() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        let s = CliProviderService::new(
            AuthStore::with_path(dir.path().join("auth.json")),
            vec![ProviderCfg {
                name: "deepseek".into(),
                kind: "openai-compat".into(),
                base_url: server.uri(),
                api_key_env: None,
            }],
            "deepseek".into(),
            "https://opencode.ai/console".into(),
            80,
            registry_with(&["deepseek"]),
        );

        let r = s
            .model_list(ModelListParams {
                provider: None,
                refresh: None,
            })
            .await
            .unwrap();
        assert_eq!(
            (r.source, r.models.first().map(|m| m.format.as_str())),
            (ModelSource::Static, Some("openai-compat"))
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    async fn mount_device_code(server: &wiremock::MockServer, user_code: &str) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/code"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code":"dc","user_code":user_code,
                "verification_uri_complete":"/console/device?user_code=x",
                "expires_in":600,"interval":5 })),
            )
            .mount(server)
            .await;
    }

    async fn mount_device_token(
        server: &wiremock::MockServer,
        status: u16,
        body: serde_json::Value,
    ) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_device_token_once(
        server: &wiremock::MockServer,
        status: u16,
        body: serde_json::Value,
    ) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .up_to_n_times(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn device_poll_writes_the_credential_only_on_authorized() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        mount_device_code(&server, "ABCD-1234").await;

        let s = svc(
            AuthStore::with_path(dir.path().join("auth.json")),
            &server.uri(),
        );
        let start = s
            .device_start(AuthDeviceStartParams { provider: None })
            .await
            .unwrap();
        assert_eq!((start.user_code.as_str(), start.interval), ("ABCD-1234", 5));

        // wiremock matches mocks in registration order and reuses them forever, so the first
        // (pending) mount MUST be capped; otherwise the authorized mount is never reached.
        mount_device_token_once(
            &server,
            400,
            serde_json::json!({"error":"authorization_pending"}),
        )
        .await;
        let pending = s
            .device_poll(AuthDevicePollParams {
                provider: None,
                device_code: start.device_code.clone(),
            })
            .await
            .unwrap();
        assert_eq!(pending.state, DeviceAuthState::Pending);
        assert!(
            s.store.get("opencode-go").unwrap().is_none(),
            "pending must not write credentials"
        );

        mount_device_token(
            &server,
            200,
            serde_json::json!({
                "access_token":"a","refresh_token":"r","expires_in":600,"org_id":"org_1"}),
        )
        .await;
        let done = s
            .device_poll(AuthDevicePollParams {
                provider: None,
                device_code: start.device_code,
            })
            .await
            .unwrap();
        assert_eq!(done.state, DeviceAuthState::Authorized);
        assert_eq!(done.credential.unwrap().kind, CredentialKind::OAuth);
        assert!(matches!(
            s.store.get("opencode-go").unwrap().unwrap(),
            Credential::OAuth { .. }
        ));
    }

    #[tokio::test]
    async fn device_start_reports_unsupported_providers_and_transport_failures() {
        let dir = tempfile::tempdir().unwrap();
        let s = CliProviderService::new(
            AuthStore::with_path(dir.path().join("auth.json")),
            vec![ProviderCfg {
                name: "deepseek".into(),
                kind: "openai-compat".into(),
                base_url: "https://example.invalid".into(),
                api_key_env: None,
            }],
            "deepseek".into(),
            "https://opencode.ai/console".into(),
            80,
            empty_registry(),
        );
        let e = s
            .device_start(AuthDeviceStartParams { provider: None })
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.data.as_ref().unwrap()["reason"].as_str().unwrap()),
            (-32602, "unsupported_provider")
        );

        // An unreachable loopback port.
        let dead: u16 = 1;
        let s = svc(
            AuthStore::with_path(dir.path().join("auth.json")),
            &format!("http://127.0.0.1:{dead}"),
        );
        assert_eq!(
            s.device_start(AuthDeviceStartParams { provider: None })
                .await
                .unwrap_err()
                .code,
            -32603
        );
        assert!(
            !s.store.path().exists(),
            "no credential may be written on transport failure"
        );
    }

    /// A provider base that refuses connections, so no upstream call is observable.
    const DEAD_BASE: &str = "http://127.0.0.1:9";

    fn empty_store() -> AuthStore {
        let dir = tempfile::tempdir().unwrap();
        AuthStore::with_path(dir.path().join("auth.json"))
    }

    /// A store pre-seeded with an `api_key` entry for `opencode-go`.
    fn pre_seeded(dir: &tempfile::TempDir, key: &str) -> AuthStore {
        let store = AuthStore::with_path(dir.path().join("auth.json"));
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

    /// A service over `base` plus the registry it was built with, so a clear can be observed
    /// through both the credential store and the registration.
    fn svc_with_store_and_registry(
        store: AuthStore,
        base: &str,
    ) -> (CliProviderService, Arc<ProviderRegistry>) {
        let reg = empty_registry();
        let svc = CliProviderService::new(
            store,
            vec![go_cfg(base)],
            "opencode-go".into(),
            base.into(),
            80,
            reg.clone(),
        );
        (svc, reg)
    }

    fn clear_params(provider: Option<&str>) -> AuthClearCredentialParams {
        AuthClearCredentialParams {
            provider: provider.map(str::to_string),
        }
    }

    fn default_params() -> AuthStatusParams {
        AuthStatusParams { provider: None }
    }

    /// Register a stub under the opencode-go name without any upstream call.
    fn activate_seeded_opencode_go(reg: &ProviderRegistry) {
        reg.register(
            "opencode-go".into(),
            Arc::new(StubProvider(vec!["opencode-go/kimi-k3".into()])),
        );
    }

    #[tokio::test]
    async fn clear_credential_unregisters_when_no_key_remains() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, reg) = svc_with_store_and_registry(pre_seeded(&dir, "sk-x"), DEAD_BASE);
        activate_seeded_opencode_go(&reg); // explicit pre-seed, no upstream call
        let out = svc
            .clear_credential(clear_params(Some("opencode-go")))
            .await
            .unwrap();
        assert!(out.cleared);
        assert!(!dir.path().join("auth.json").exists());
        assert!(reg.get("opencode-go").is_none());
        assert!(
            !svc.auth_status(default_params())
                .await
                .unwrap()
                .authenticated
        );
    }

    #[tokio::test]
    async fn clear_credential_absent_entry_reports_false() {
        let (svc, _reg) = svc_with_store_and_registry(empty_store(), DEAD_BASE);
        assert!(
            !svc.clear_credential(clear_params(None))
                .await
                .unwrap()
                .cleared
        );
    }

    /// A service over the mock console base plus the temp dir and registry it was built with,
    /// so a device-code login is observable through both the store and the registration.
    fn svc_with_device_flow(
        console: &str,
    ) -> (CliProviderService, tempfile::TempDir, Arc<ProviderRegistry>) {
        let reg = empty_registry();
        let dir = tempfile::tempdir().unwrap();
        let svc = CliProviderService::new(
            AuthStore::with_path(dir.path().join("auth.json")),
            vec![go_cfg(console)],
            "opencode-go".into(),
            console.into(),
            80,
            reg.clone(),
        );
        (svc, dir, reg)
    }

    fn poll_params(device_code: &str) -> AuthDevicePollParams {
        AuthDevicePollParams {
            provider: None,
            device_code: device_code.into(),
        }
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
    async fn authorized_device_poll_also_registers_the_provider() {
        let server = wiremock::MockServer::start().await;
        mount_token_success(&server, "access-1").await;
        let (svc, dir, reg) = svc_with_device_flow(&server.uri());
        let out = svc.device_poll(poll_params("dev-code")).await.unwrap();
        assert_eq!(out.state, DeviceAuthState::Authorized);
        assert!(dir.path().join("auth.json").exists());
        assert!(reg.get("opencode-go").is_some(), "no restart needed");
    }

    #[tokio::test]
    async fn pending_device_poll_leaves_the_registry_untouched() {
        let server = wiremock::MockServer::start().await;
        mount_device_token(
            &server,
            400,
            serde_json::json!({"error":"authorization_pending"}),
        )
        .await;
        let (svc, _dir, reg) = svc_with_device_flow(&server.uri());
        let out = svc.device_poll(poll_params("dev-code")).await.unwrap();
        assert_eq!(out.state, DeviceAuthState::Pending);
        assert!(
            reg.get("opencode-go").is_none(),
            "pending must not activate"
        );
    }
}
