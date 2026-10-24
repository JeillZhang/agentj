//! Single-flight OAuth access-token refresh (design D7). @since 0.27.0
//!
//! Refresh tokens are **single-use**: reusing a rotated one revokes the whole subscription
//! session. Rotation therefore happens at most once per observed state, guarded by an
//! exclusive `auth.json.lock` file and a re-read after the lock is acquired.

use crate::credential::{AuthError, Credential};
use crate::oauth::{PollOutcome, device_refresh, now_epoch};
use crate::resolve::{KeySource, ResolvedKey};
use crate::store::AuthStore;
use std::time::{Duration, Instant};

/// Refresh when fewer than this many seconds of access-token life remain.
pub const ACCESS_SAFETY_MARGIN_SECS: i64 = 60;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_STEP: Duration = Duration::from_millis(25);

/// Exclusive rotation lock (`auth.json` → `auth.json.lock`), released on drop.
struct FileLock(std::path::PathBuf);

impl FileLock {
    async fn acquire(store: &AuthStore) -> Result<Self, AuthError> {
        let path = store.path().with_extension("json.lock");
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(FileLock(path)),
                Err(_) if start.elapsed() < LOCK_TIMEOUT => tokio::time::sleep(LOCK_STEP).await,
                Err(e) => {
                    return Err(AuthError::Io {
                        path: path.display().to_string(),
                        reason: e.to_string(),
                    });
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A still-usable key, or `None` when the credential must be rotated first.
fn usable(cred: &Credential, now: i64) -> Option<String> {
    match cred {
        Credential::ApiKey { key, .. } if !key.is_empty() => Some(key.clone()),
        Credential::OAuth {
            access, expires, ..
        } if !access.is_empty() && *expires - now > ACCESS_SAFETY_MARGIN_SECS => {
            Some(access.clone())
        }
        _ => None,
    }
}

/// Resolve a usable access key for `provider`, rotating an expired OAuth credential once.
///
/// Never returns an empty key and never replays a stale refresh token.
pub async fn ensure_access(
    store: &AuthStore,
    provider: &str,
    console_base: &str,
) -> Result<Option<ResolvedKey>, AuthError> {
    let Some(cred) = store.get(provider)? else {
        return Ok(None);
    };
    if let Some(key) = usable(&cred, now_epoch()) {
        return Ok(Some(file_key(key)));
    }
    let _lock = FileLock::acquire(store).await?;
    rotate_locked(store, provider, console_base).await
}

/// Rotate while holding the lock: re-read first, since another process may have won.
async fn rotate_locked(
    store: &AuthStore,
    provider: &str,
    console_base: &str,
) -> Result<Option<ResolvedKey>, AuthError> {
    // Re-read after acquiring: another process may have rotated already.
    let Some(cred) = store.get(provider)? else {
        return Ok(None);
    };
    if let Some(key) = usable(&cred, now_epoch()) {
        return Ok(Some(file_key(key)));
    }
    let Credential::OAuth { refresh, .. } = cred else {
        return Ok(None);
    };
    if refresh.is_empty() {
        return Err(AuthError::RefreshRejected {
            reason: "empty refresh token".into(),
        });
    }
    match device_refresh(console_base, &refresh).await? {
        PollOutcome::Authorized(t) => {
            store.set(
                provider,
                Credential::OAuth {
                    access: t.access.clone(),
                    refresh: t.refresh,
                    expires: t.expires_at,
                    account_id: None,
                    org_id: t.org_id,
                },
            )?;
            Ok(Some(file_key(t.access)))
        }
        other => Err(AuthError::RefreshRejected {
            reason: format!("{other:?}"),
        }),
    }
}

/// A key that came out of the credential file.
fn file_key(key: String) -> ResolvedKey {
    ResolvedKey {
        key,
        source: KeySource::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_store(dir: &std::path::Path, expires: i64) -> AuthStore {
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

    #[tokio::test]
    async fn fresh_credentials_are_returned_without_http() {
        let dir = tempfile::tempdir().unwrap();
        let server = wiremock::MockServer::start().await;
        let store = oauth_store(dir.path(), crate::oauth::now_epoch() + 3600);
        assert_eq!(
            ensure_access(&store, "opencode-go", &server.uri())
                .await
                .unwrap()
                .unwrap()
                .key,
            "old-access"
        );
        assert!(server.received_requests().await.unwrap().is_empty());

        let api_key_store = AuthStore::with_path(dir.path().join("api.json"));
        api_key_store
            .set(
                "p",
                Credential::ApiKey {
                    key: "sk-k".into(),
                    label: None,
                    created_at: None,
                },
            )
            .unwrap();
        assert_eq!(
            ensure_access(&api_key_store, "p", &server.uri())
                .await
                .unwrap()
                .unwrap()
                .key,
            "sk-k"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn expired_credential_rotates_exactly_once_under_concurrency() {
        let dir = tempfile::tempdir().unwrap();
        let store = oauth_store(dir.path(), crate::oauth::now_epoch() - 1);
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token":"new-access","refresh_token":"new-refresh","expires_in":600,"org_id":"org_1" })))
            .mount(&server)
            .await;

        let base = server.uri();
        let (a, b) = tokio::join!(
            ensure_access(&store, "opencode-go", &base),
            ensure_access(&store, "opencode-go", &base)
        );
        assert_eq!(a.unwrap().unwrap().key, "new-access");
        assert_eq!(b.unwrap().unwrap().key, "new-access");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "refresh must be single-flight"
        );
        match store.get("opencode-go").unwrap().unwrap() {
            Credential::OAuth {
                access, refresh, ..
            } => {
                assert_eq!(
                    (access.as_str(), refresh.as_str()),
                    ("new-access", "new-refresh")
                )
            }
            other => panic!("expected oauth, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_refresh_keeps_the_stored_credential() {
        let dir = tempfile::tempdir().unwrap();
        let store = oauth_store(dir.path(), crate::oauth::now_epoch() - 1);
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(
                wiremock::ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"})),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            ensure_access(&store, "opencode-go", &server.uri())
                .await
                .unwrap_err(),
            AuthError::Http { status: 401, .. }
        ));
        match store.get("opencode-go").unwrap().unwrap() {
            Credential::OAuth { refresh, .. } => assert_eq!(refresh, "old-refresh"),
            other => panic!("expected oauth, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_refresh_token_is_rejected_without_http() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        store
            .set(
                "opencode-go",
                Credential::try_from_value(&serde_json::json!({
                    "type":"oauth","access":"","refresh":"","expires":1
                }))
                .unwrap(),
            )
            .unwrap();
        let server = wiremock::MockServer::start().await;
        assert!(matches!(
            ensure_access(&store, "opencode-go", &server.uri())
                .await
                .unwrap_err(),
            AuthError::RefreshRejected { .. }
        ));
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
