//! opencode-go OAuth device-authorization client (RFC 8628, design D7). @since 0.27.0
//!
//! Endpoints live under the Console base (`[model].console_base_url`,
//! default `https://opencode.ai/console`); the token endpoint is shared by the
//! device-code exchange and the refresh exchange.

use crate::credential::AuthError;

/// Console origin hosting the device-flow endpoints.
pub const OPENCODE_CONSOLE_BASE: &str = "https://opencode.ai/console";
/// Public client id used by the official CLI; no client secret exists.
pub const OPENCODE_CLI_CLIENT_ID: &str = "opencode-cli";
const UA: &str = concat!("agentj/", env!("CARGO_PKG_VERSION"));

/// Device-authorization response (RFC 8628 §3.2) with a resolved absolute URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// A rotated token set. `expires_at` is a Unix epoch second derived from `expires_in`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: i64,
    pub org_id: Option<String>,
}

/// Outcome of one token-endpoint exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// Client should keep polling at the current interval.
    Pending,
    /// Client should back off; `interval` is the caller's interval bumped by 5s.
    SlowDown { interval: u64 },
    Authorized(Tokens),
    Expired,
    Denied,
    /// Unrecognized upstream error string, surfaced verbatim.
    Error(String),
}

pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .build()
        .expect("http client")
}

pub(crate) fn token_url(console_base: &str) -> String {
    format!("{}/auth/device/token", console_base.trim_end_matches('/'))
}

/// `verification_uri_complete` is a path relative to the console origin.
fn verification_uri(console_base: &str, v: &serde_json::Value) -> String {
    let raw = v["verification_uri_complete"]
        .as_str()
        .unwrap_or_default();
    if raw.starts_with("http") {
        raw.to_string()
    } else {
        format!(
            "{}{}",
            console_base.trim_end_matches('/'),
            if raw.starts_with('/') {
                raw.to_string()
            } else {
                format!("/{raw}")
            }
        )
    }
}

/// `POST <console>/auth/device/code` — start a device authorization.
pub async fn device_start(console_base: &str) -> Result<DeviceCode, AuthError> {
    let url = format!("{}/auth/device/code", console_base.trim_end_matches('/'));
    let resp = client()
        .post(&url)
        .form(&[
            ("client_id", OPENCODE_CLI_CLIENT_ID),
            ("supports_org_scope", "true"),
        ])
        .send()
        .await
        .map_err(|e| AuthError::Network {
            reason: e.to_string(),
        })?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(AuthError::Http {
            status,
            message: body,
        });
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| AuthError::Malformed {
        path: url,
        reason: e.to_string(),
    })?;
    Ok(DeviceCode {
        device_code: v["device_code"].as_str().unwrap_or_default().to_string(),
        user_code: v["user_code"].as_str().unwrap_or_default().to_string(),
        verification_uri: verification_uri(console_base, &v),
        expires_in: v["expires_in"].as_u64().unwrap_or(600),
        interval: v["interval"].as_u64().unwrap_or(5),
    })
}

/// Parse a token-endpoint body into a poll outcome (shared by poll and refresh).
///
/// A `200` yields [`PollOutcome::Authorized`]. Recognized 4xx OAuth error codes map to their
/// states; an unrecognized non-empty error string is surfaced verbatim as
/// [`PollOutcome::Error`] (design D3: domain failures are result data), while a body with no
/// error string stays an [`AuthError::Http`] (transport-level failure).
fn parse_token_body(status: u16, body: &str) -> Result<PollOutcome, AuthError> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if status == 200 {
        return Ok(PollOutcome::Authorized(Tokens {
            access: v["access_token"].as_str().unwrap_or_default().to_string(),
            refresh: v["refresh_token"].as_str().unwrap_or_default().to_string(),
            expires_at: now_epoch() + v["expires_in"].as_i64().unwrap_or(600),
            org_id: v["org_id"].as_str().map(str::to_string),
        }));
    }
    match v["error"].as_str().unwrap_or("") {
        "authorization_pending" => Ok(PollOutcome::Pending),
        // Placeholder value; `device_poll` replaces it with the caller's interval + 5s.
        "slow_down" => Ok(PollOutcome::SlowDown { interval: 10 }),
        "access_denied" => Ok(PollOutcome::Denied),
        "expired_token" => Ok(PollOutcome::Expired),
        "" => Err(AuthError::Http {
            status,
            message: body.to_string(),
        }),
        other => Ok(PollOutcome::Error(other.to_string())),
    }
}

/// One device-code poll (`grant_type=urn:ietf:params:oauth:grant-type:device_code`).
/// `interval` is the caller's current interval; `slow_down` returns `interval + 5`.
pub async fn device_poll(
    console_base: &str,
    device_code: &str,
    interval: u64,
) -> Result<PollOutcome, AuthError> {
    let resp = client()
        .post(token_url(console_base))
        .form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("device_code", device_code),
            ("client_id", OPENCODE_CLI_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|e| AuthError::Network {
            reason: e.to_string(),
        })?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    match parse_token_body(status, &body)? {
        PollOutcome::SlowDown { .. } => Ok(PollOutcome::SlowDown {
            interval: interval + 5,
        }),
        other => Ok(other),
    }
}

/// Exchange a refresh token for a rotated token set. `401`/`403` are surfaced as
/// [`AuthError::Http`] so callers can map them to a re-login prompt.
pub async fn device_refresh(console_base: &str, refresh: &str) -> Result<PollOutcome, AuthError> {
    let resp = client()
        .post(token_url(console_base))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", OPENCODE_CLI_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|e| AuthError::Network {
            reason: e.to_string(),
        })?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status == 401 || status == 403 {
        return Err(AuthError::Http {
            status,
            message: body,
        });
    }
    parse_token_body(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn device_start_posts_form_and_parses_the_payload() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/auth/device/code"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code":"dc","user_code":"ABCD-1234",
                "verification_uri_complete":"/console/device?user_code=ABCD-1234",
                "expires_in":600,"interval":5 })))
            .mount(&server)
            .await;

        let code = device_start(&server.uri()).await.unwrap();
        assert_eq!(
            (code.user_code.as_str(), code.expires_in, code.interval),
            ("ABCD-1234", 600, 5)
        );
        assert_eq!(
            code.verification_uri,
            format!("{}/console/device?user_code=ABCD-1234", server.uri())
        );

        let reqs = server.received_requests().await.unwrap();
        let body = String::from_utf8(reqs[0].body.clone()).unwrap();
        assert!(
            body.contains("client_id=opencode-cli") && body.contains("supports_org_scope=true"),
            "{body}"
        );
        assert_eq!(
            reqs[0].headers.get("content-type").unwrap(),
            "application/x-www-form-urlencoded"
        );
        assert!(reqs[0]
            .headers
            .get("user-agent")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("agentj/"));
    }

    #[tokio::test]
    async fn device_start_maps_http_errors() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        assert!(matches!(
            device_start(&server.uri()).await.unwrap_err(),
            AuthError::Http { status: 500, .. }
        ));
    }

    async fn mount_token(server: &wiremock::MockServer, status: u16, body: serde_json::Value) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn poll_maps_every_documented_state() {
        let server = wiremock::MockServer::start().await;
        mount_token(&server, 400, serde_json::json!({"error":"authorization_pending"})).await;
        assert_eq!(
            device_poll(&server.uri(), "dc", 5).await.unwrap(),
            PollOutcome::Pending
        );

        let server = wiremock::MockServer::start().await;
        mount_token(&server, 400, serde_json::json!({"error":"slow_down"})).await;
        assert_eq!(
            device_poll(&server.uri(), "dc", 5).await.unwrap(),
            PollOutcome::SlowDown { interval: 10 }
        );

        let server = wiremock::MockServer::start().await;
        mount_token(
            &server,
            200,
            serde_json::json!({"access_token":"a","refresh_token":"r","expires_in":600,"org_id":"org_1"}),
        )
        .await;
        match device_poll(&server.uri(), "dc", 5).await.unwrap() {
            PollOutcome::Authorized(t) => {
                assert_eq!(
                    (t.access.as_str(), t.refresh.as_str(), t.org_id.as_deref()),
                    ("a", "r", Some("org_1"))
                );
                assert!(t.expires_at > now_epoch());
            }
            other => panic!("expected Authorized, got {other:?}"),
        }

        for (raw, want) in [
            ("access_denied", PollOutcome::Denied),
            ("expired_token", PollOutcome::Expired),
            ("teapot", PollOutcome::Error("teapot".into())),
        ] {
            let server = wiremock::MockServer::start().await;
            mount_token(&server, 400, serde_json::json!({"error": raw})).await;
            assert_eq!(device_poll(&server.uri(), "dc", 5).await.unwrap(), want);
        }
    }

    #[tokio::test]
    async fn refresh_uses_the_refresh_grant_and_rotates() {
        let server = wiremock::MockServer::start().await;
        mount_token(
            &server,
            200,
            serde_json::json!({"access_token":"new","refresh_token":"rotated","expires_in":600}),
        )
        .await;
        match device_refresh(&server.uri(), "old-refresh").await.unwrap() {
            PollOutcome::Authorized(t) => {
                assert_eq!((t.access.as_str(), t.refresh.as_str()), ("new", "rotated"))
            }
            other => panic!("expected Authorized, got {other:?}"),
        }
        let body = String::from_utf8(server.received_requests().await.unwrap()[0].body.clone()).unwrap();
        assert!(
            body.contains("grant_type=refresh_token") && body.contains("refresh_token=old-refresh"),
            "{body}"
        );
    }
}
