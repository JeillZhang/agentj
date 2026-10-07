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
}
