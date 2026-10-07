//! Credential precedence resolution: `--api-key` flag > `api_key_env` (non-empty) > file.

use crate::credential::AuthError;
use crate::store::AuthStore;

/// Where a resolved key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Flag,
    Env,
    File,
}

/// A non-empty key and its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKey {
    pub key: String,
    pub source: KeySource,
}

/// Resolve the usable key for `provider`, or `None` when nothing is available.
///
/// Never returns an empty key: an unset/blank env var falls through to the file.
pub fn resolve_credential(
    provider: &str,
    flag: Option<&str>,
    api_key_env: Option<&str>,
    store: &AuthStore,
) -> Result<Option<ResolvedKey>, AuthError> {
    if let Some(k) = flag.filter(|k| !k.trim().is_empty()) {
        return Ok(Some(ResolvedKey {
            key: k.trim().to_string(),
            source: KeySource::Flag,
        }));
    }
    if let Some(name) = api_key_env.filter(|n| !n.is_empty())
        && let Ok(v) = std::env::var(name)
        && !v.trim().is_empty()
    {
        return Ok(Some(ResolvedKey {
            key: v.trim().to_string(),
            source: KeySource::Env,
        }));
    }
    Ok(match store.get(provider)? {
        Some(cred) => Some(ResolvedKey {
            key: cred.key()?.to_string(),
            source: KeySource::File,
        }),
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::Credential;
    use crate::store::AuthStore;

    fn seeded_store(dir: &std::path::Path) -> AuthStore {
        let s = AuthStore::with_path(dir.join("auth.json"));
        s.set(
            "opencode-go",
            Credential::ApiKey {
                key: "K".into(),
                label: None,
                created_at: None,
            },
        )
        .unwrap();
        s
    }

    #[test]
    fn flag_wins_over_env_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = seeded_store(dir.path());
        // SAFETY: single-threaded test-local env mutation; unique variable name.
        unsafe { std::env::set_var("AGENTJ_TEST_KEY", "E") };
        let r = resolve_credential("opencode-go", Some("F"), Some("AGENTJ_TEST_KEY"), &s)
            .unwrap()
            .unwrap();
        assert_eq!((r.key.as_str(), r.source), ("F", KeySource::Flag));
    }

    #[test]
    fn empty_env_name_falls_through_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = seeded_store(dir.path());
        let r = resolve_credential("opencode-go", None, Some(""), &s)
            .unwrap()
            .unwrap();
        assert_eq!((r.key.as_str(), r.source), ("K", KeySource::File));
    }

    #[test]
    fn nothing_resolvable_is_none_not_empty_string() {
        let dir = tempfile::tempdir().unwrap();
        let s = AuthStore::with_path(dir.path().join("auth.json"));
        assert!(
            resolve_credential("opencode-go", None, Some("AGENTJ_UNSET_KEY"), &s)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn oauth_file_credential_resolves_with_file_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        store
            .set(
                "opencode-go",
                Credential::try_from_value(&serde_json::json!({
                    "type":"oauth","access":"a-tok","refresh":"r-tok","expires":42
                }))
                .unwrap(),
            )
            .unwrap();
        let got = resolve_credential("opencode-go", None, None, &store)
            .unwrap()
            .unwrap();
        assert_eq!((got.key.as_str(), got.source), ("a-tok", KeySource::File));
        let cloned = store.clone();
        assert_eq!(cloned.path(), store.path());
    }
}
