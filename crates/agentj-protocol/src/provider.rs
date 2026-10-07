//! `model/list` + `auth/status` wire types for the opencode-go subscription.
//! @since 0.17.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `model/list` params. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelListParams {
    /// Provider name; omitted means the configured default provider. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Bypass the 60s in-process reuse window. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<bool>,
}

/// `model/list` result — catalog plus the tier it came from. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelListResult {
    pub provider: String,
    pub default_model: String,
    pub source: ModelSource,
    /// Unix epoch seconds when the catalog was fetched. @since 0.17.0
    pub fetched_at: i64,
    pub models: Vec<ModelEntry>,
}

/// One catalog entry. `verified: false` means the route was synthesized, not known.
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelEntry {
    pub id: String,
    pub verified: bool,
    pub format: String,
}

/// Catalog tier (account → live → overlay; static for non-go providers).
/// `Unknown` tolerates forward values; the schema lists only the known tiers.
/// @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Account,
    Live,
    Overlay,
    Static,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// `auth/status` params. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthStatusParams {
    /// Provider name; omitted means the configured default provider. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// `auth/status` result. Never carries a full key/token — see [`CredentialInfo`].
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthStatusResult {
    pub provider: String,
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialInfo>,
    pub models_count: u32,
}

/// Redacted credential projection. `key_hint` is the api_key's last four characters
/// (`…1234`); OAuth credentials are identified by `org_id`/`expires_at` instead.
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CredentialInfo {
    pub kind: CredentialKind,
    pub source: CredentialSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub expiring_soon: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
}

/// Credential kind. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    // The explicit rename is required because `rename_all = "snake_case"` would render
    // `OAuth` as `o_auth`; the wire value must be `oauth` (matches the `type:"oauth"` file tag).
    #[serde(rename = "oauth")]
    OAuth,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Flag,
    Env,
    File,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// `provider/usage` params. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderUsageParams {
    /// Provider name; omitted means the configured default provider. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Bypass the 300s in-process cache. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<bool>,
}

/// `provider/usage` result. Domain failures travel in `error` (success response).
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderUsageResult {
    pub provider: String,
    /// Unix epoch seconds when the snapshot was fetched. @since 0.17.0
    pub fetched_at: i64,
    /// `true` when `windows` is a reused earlier snapshot (error path). @since 0.17.0
    pub stale: bool,
    /// Client-side warning threshold in percent (1..=100). @since 0.17.0
    pub warn_percent: u8,
    pub windows: Vec<UsageWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderUsageError>,
}

/// One quota window. `percent` is the *used* percentage. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsageWindow {
    pub kind: UsageWindowKind,
    pub status: UsageWindowStatus,
    /// `None` when upstream's value was absent or not an integer. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    /// Omitted when `percent == 0` (upstream placeholder is dropped). @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

/// Quota window kind. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindowKind {
    Rolling,
    Weekly,
    Monthly,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Window status. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum UsageWindowStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "rate-limited")]
    RateLimited,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Domain failure carried inside a successful `provider/usage` response.
/// `kind ∈ {no_credential, expired, unauthorized, entitlement, unavailable, network}`.
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderUsageError {
    pub kind: String,
    pub message: String,
}

/// `auth/deviceStart` params. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthDeviceStartParams {
    /// Provider name; omitted means the configured default provider. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// `auth/deviceStart` result (RFC 8628 device authorization response). @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthDeviceStartResult {
    pub provider: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// `auth/devicePoll` params. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthDevicePollParams {
    /// Provider name; omitted means the configured default provider. @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub device_code: String,
}

/// `auth/devicePoll` result — the poll state, never a raw token on failure states.
/// @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthDevicePollResult {
    pub state: DeviceAuthState,
    /// Next poll interval in seconds (present for `slow_down`). @since 0.17.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialInfo>,
}

/// Device-flow poll state. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAuthState {
    Pending,
    SlowDown,
    Authorized,
    Expired,
    Denied,
    Error,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_list_result_roundtrip() {
        let r = ModelListResult {
            provider: "opencode-go".into(),
            default_model: "opencode-go/kimi-k3".into(),
            source: ModelSource::Account,
            fetched_at: 1_760_000_000,
            models: vec![ModelEntry {
                id: "opencode-go/kimi-k3".into(),
                verified: true,
                format: "anthropic".into(),
            }],
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["source"], "account");
        assert_eq!(v["models"][0]["verified"], true);
        assert_eq!(v["fetched_at"], 1_760_000_000i64);
        let back: ModelListResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.models[0].id, "opencode-go/kimi-k3");
    }

    #[test]
    fn unknown_model_source_is_tolerated() {
        let v = serde_json::json!({"provider":"p","default_model":"m","source":"quantum-tier",
            "fetched_at":1,"models":[]});
        let r: ModelListResult = serde_json::from_value(v).unwrap();
        assert!(matches!(r.source, ModelSource::Unknown));
        // Forward-compat convention (AGENTS.md): the Unknown unit variant tolerates the wire
        // value and reserializes to the canonical literal "unknown" (raw string is not kept).
        assert_eq!(serde_json::to_value(&r).unwrap()["source"], "unknown");
    }

    #[test]
    fn auth_status_result_carries_only_a_hint() {
        let r = AuthStatusResult {
            provider: "opencode-go".into(),
            authenticated: true,
            credential: Some(CredentialInfo {
                kind: CredentialKind::ApiKey,
                source: CredentialSource::File,
                key_hint: Some("…cdef".into()),
                expires_at: None,
                expiring_soon: false,
                org_id: None,
            }),
            models_count: 44,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["credential"]["key_hint"], "…cdef");
        assert_eq!(v["credential"]["kind"], "api_key");
        assert!(!v.to_string().contains("sk-"), "no full key may appear");
    }

    #[test]
    fn credential_kind_oauth_uses_the_oauth_wire_value() {
        // Why: `rename_all = "snake_case"` would render `OAuth` as `o_auth`; the wire value
        // must be `oauth` (plan T16 AC; spec §8.24/§9.1; matches the `type:"oauth"` file tag).
        assert_eq!(
            serde_json::to_value(CredentialKind::OAuth).unwrap(),
            "oauth"
        );
        let back: CredentialKind = serde_json::from_value(serde_json::json!("oauth")).unwrap();
        assert!(matches!(back, CredentialKind::OAuth));
        assert_eq!(
            serde_json::to_value(CredentialKind::ApiKey).unwrap(),
            "api_key"
        );
    }

    #[test]
    fn zero_percent_omits_reset_and_states_use_snake_case() {
        let r = ProviderUsageResult {
            provider: "opencode-go".into(),
            fetched_at: 1,
            stale: false,
            warn_percent: 80,
            windows: vec![
                UsageWindow {
                    kind: UsageWindowKind::Rolling,
                    status: UsageWindowStatus::Ok,
                    percent: Some(4),
                    resets_at: Some("2026-08-13T16:27:38.287Z".into()),
                },
                UsageWindow {
                    kind: UsageWindowKind::Monthly,
                    status: UsageWindowStatus::Ok,
                    percent: Some(0),
                    resets_at: None,
                },
            ],
            error: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["windows"][1]["kind"], "monthly");
        assert!(v["windows"][1].get("resets_at").is_none());
        let slow = AuthDevicePollResult {
            state: DeviceAuthState::SlowDown,
            interval: Some(10),
            message: None,
            credential: None,
        };
        assert_eq!(serde_json::to_value(&slow).unwrap()["state"], "slow_down");
    }

    #[test]
    fn unknown_window_status_is_tolerated() {
        let v = serde_json::json!({"provider":"p","fetched_at":1,"stale":false,"warn_percent":80,
            "windows":[{"kind":"rolling","status":"throttled","percent":9}]});
        let r: ProviderUsageResult = serde_json::from_value(v).unwrap();
        assert!(matches!(r.windows[0].status, UsageWindowStatus::Unknown));
        assert_eq!(r.windows[0].percent, Some(9));
    }
}
