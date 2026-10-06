use serde::{Deserialize, Serialize};

/// A stored credential. Tag-discriminated on `type`; the `OAuth` shape is frozen now so
/// phase 2 does not change the file format (design D2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    ApiKey {
        key: String,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        created_at: Option<String>,
    },
    OAuth {
        access: String,
        refresh: String,
        expires: i64,
        #[serde(default)]
        account_id: Option<String>,
        #[serde(default)]
        org_id: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("credential file not readable: {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("insecure credential file permissions {mode:o} (expected 600): {path}")]
    InsecurePermissions { path: String, mode: u32 },
    #[error("credential file parse failed: {path}: {reason}")]
    Malformed { path: String, reason: String },
    #[error("unsupported credential type '{kind}': requires agentj phase 2 or newer")]
    Unsupported { kind: String },
    #[error("no home directory: cannot locate credential store")]
    NoHomeDir,
}

pub type AuthFile = std::collections::BTreeMap<String, Credential>;

impl Credential {
    /// The secret usable as a bearer token. OAuth is rejected with a phase-2 hint.
    pub fn key(&self) -> Result<&str, AuthError> {
        match self {
            Credential::ApiKey { key, .. } => Ok(key),
            Credential::OAuth { .. } => Err(AuthError::Unsupported {
                kind: "oauth".into(),
            }),
        }
    }

    /// Tag-discriminated parse. Unknown tags are `Unsupported` (version hint), not
    /// `Malformed`, so a newer agentj written file fails with an actionable message.
    pub fn try_from_value(v: &serde_json::Value) -> Result<Self, AuthError> {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("api_key") => {
                serde_json::from_value(v.clone()).map_err(|e| AuthError::Malformed {
                    path: "<credential>".into(),
                    reason: e.to_string(),
                })
            }
            Some("oauth") => Err(AuthError::Unsupported {
                kind: "oauth".into(),
            }),
            Some(other) => Err(AuthError::Unsupported {
                kind: other.to_string(),
            }),
            None => Err(AuthError::Malformed {
                path: "<credential>".into(),
                reason: "missing credential type".into(),
            }),
        }
    }
}

/// Parse the whole store; every entry goes through `Credential::try_from_value`.
pub fn parse_auth_file(text: &str, path: &str) -> Result<AuthFile, AuthError> {
    let raw: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_str(text)
        .map_err(|e| AuthError::Malformed {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
    raw.into_iter()
        .map(|(k, v)| Credential::try_from_value(&v).map(|c| (k, c)))
        .collect()
}

/// Render a key as `…<last 4>`; never reveals more than the tail.
pub fn redact(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_credential_parses_via_parse_auth_file() {
        let body = r#"{"opencode-go":{"type":"api_key","key":"sk-x","label":"ws"}}"#;
        let file = parse_auth_file(body, "<test>").unwrap();
        assert_eq!(file.get("opencode-go").unwrap().key().unwrap(), "sk-x");
    }

    #[test]
    fn oauth_and_unknown_tags_report_unsupported() {
        let oauth = serde_json::json!({"type":"oauth","access":"a","refresh":"r","expires":123});
        assert!(
            matches!(Credential::try_from_value(&oauth), Err(AuthError::Unsupported { ref kind }) if kind == "oauth")
        );
        let weird = serde_json::json!({"type":"perpetual"});
        assert!(
            matches!(Credential::try_from_value(&weird), Err(AuthError::Unsupported { ref kind }) if kind == "perpetual")
        );
    }

    #[test]
    fn missing_type_is_malformed() {
        assert!(matches!(
            Credential::try_from_value(&serde_json::json!({"key":"sk-x"})),
            Err(AuthError::Malformed { .. })
        ));
    }

    #[test]
    fn redact_keeps_last_four_only() {
        assert_eq!(redact("sk-1234567890"), "…7890");
    }
}
