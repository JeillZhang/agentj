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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    OAuth,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Where the credential came from. `Unknown` tolerates forward values. @since 0.17.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Flag,
    Env,
    File,
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
}
