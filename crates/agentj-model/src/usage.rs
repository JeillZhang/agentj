//! Defensive `GET /v1/usage` client for the opencode-go subscription (design D6). @since 0.27.0
//!
//! The upstream endpoint is undocumented, reshaped once within an hour of launch and sends no
//! cache headers. Parsing therefore only reads the three known window keys, ignores unknown
//! keys/fields, degrades a non-integer `percent` to `None`, and drops the `percent == 0`
//! placeholder `resetsAt` instead of inventing data.

use crate::http;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

/// The three known quota windows, in reporting order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageWindowKind {
    Rolling,
    Weekly,
    Monthly,
}

/// Window status; unknown upstream values degrade to [`UsageStatus::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageStatus {
    Ok,
    RateLimited,
    Unknown,
}

/// One quota window. `percent` is the *used* percentage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageWindow {
    pub kind: UsageWindowKind,
    pub status: UsageStatus,
    pub percent: Option<u8>,
    pub resets_at: Option<String>,
}

/// Domain failure of a usage query (mapped to protocol `error.kind` by the CLI service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    NoCredential,
    Expired,
    Unauthorized,
    Entitlement,
    Unavailable,
    Network(String),
}

/// Fetch the quota windows for `key`. Never contacts the server without a non-empty key.
pub async fn fetch_usage(base_url: &str, key: Option<&str>) -> Result<Vec<UsageWindow>, UsageError> {
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        return Err(UsageError::NoCredential);
    };
    let url = format!("{}/v1/usage", base_url.trim_end_matches('/'));
    let resp = http::client()
        .get(&url)
        .bearer_auth(key)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| UsageError::Network(e.to_string()))?;
    match resp.status().as_u16() {
        200 => {
            let body = resp
                .text()
                .await
                .map_err(|e| UsageError::Network(e.to_string()))?;
            let v: serde_json::Value =
                serde_json::from_str(&body).map_err(|_| UsageError::Unavailable)?;
            window_list(&v).ok_or(UsageError::Unavailable)
        }
        401 => Err(UsageError::Unauthorized),
        403 => Err(UsageError::Entitlement),
        _ => Err(UsageError::Unavailable),
    }
}

/// Defensive parse (design D6): only the three known keys are read, unknown keys/fields are
/// ignored, a missing/non-integer percent degrades to `None`, and only `percent == 0` drops
/// the upstream placeholder reset timestamp.
fn window_list(v: &serde_json::Value) -> Option<Vec<UsageWindow>> {
    let usage = v.get("usage")?;
    let known = [
        ("rolling", UsageWindowKind::Rolling),
        ("weekly", UsageWindowKind::Weekly),
        ("monthly", UsageWindowKind::Monthly),
    ];
    let mut out = Vec::new();
    for (name, kind) in known {
        let Some(w) = usage.get(name) else { continue };
        let percent = w
            .get("percent")
            .and_then(|p| p.as_u64())
            .map(|p| p.min(100) as u8);
        let status = match w.get("status").and_then(|s| s.as_str()) {
            Some("ok") => UsageStatus::Ok,
            Some("rate-limited") => UsageStatus::RateLimited,
            _ => UsageStatus::Unknown,
        };
        let resets_at = match percent {
            Some(0) => None,
            _ => w.get("resetsAt").and_then(|r| match r {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            }),
        };
        out.push(UsageWindow {
            kind,
            status,
            percent,
            resets_at,
        });
    }
    if out.is_empty() { None } else { Some(out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mount(server: &wiremock::MockServer, status: u16, body: serde_json::Value) {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/usage"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn parses_three_windows_and_drops_the_zero_percent_placeholder() {
        let server = wiremock::MockServer::start().await;
        mount(
            &server,
            200,
            serde_json::json!({"usage":{
                "rolling":{"status":"ok","percent":4,"resetsAt":"2026-08-13T16:27:38.287Z"},
                "weekly":{"status":"ok","percent":3,"resetsAt":"2026-08-17T00:00:00.287Z"},
                "monthly":{"status":"ok","percent":0,"resetsAt":"2026-09-13T06:06:01.287Z"}}}),
        )
        .await;
        let w = fetch_usage(&server.uri(), Some("k")).await.unwrap();
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].kind, w[0].percent), (UsageWindowKind::Rolling, Some(4)));
        assert_eq!(w[2].percent, Some(0));
        assert!(
            w[2].resets_at.is_none(),
            "zero-percent placeholder must be dropped"
        );

        let reqs = server.received_requests().await.unwrap();
        assert!(reqs[0]
            .headers
            .get("user-agent")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("agentj/"));
        assert!(reqs[0]
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Bearer "));
        assert!(reqs[0].headers.get("x-opencode-session").is_none());
    }

    #[tokio::test]
    async fn tolerates_shape_drift() {
        let server = wiremock::MockServer::start().await;
        mount(
            &server,
            200,
            serde_json::json!({"usage":{
                "rolling":{"status":"throttled","percent":"9","resetsAt":123},
                "biweekly":{"status":"ok","percent":1},
                "weekly":{"status":"rate-limited","percent":100,"resetsAt":"x"}}}),
        )
        .await;
        let w = fetch_usage(&server.uri(), Some("k")).await.unwrap();
        assert_eq!(w.len(), 2, "unknown window key ignored");
        assert_eq!(w[0].status, UsageStatus::Unknown);
        assert_eq!(w[0].percent, None, "non-integer percent degrades to unknown");
        assert_eq!(
            w[0].resets_at.as_deref(),
            Some("123"),
            "non-string reset kept verbatim"
        );
        assert_eq!(w[1].status, UsageStatus::RateLimited);
    }

    #[tokio::test]
    async fn maps_failures_without_folding_403_into_401() {
        for (status, want) in [
            (401u16, UsageError::Unauthorized),
            (403, UsageError::Entitlement),
            (503, UsageError::Unavailable),
        ] {
            let server = wiremock::MockServer::start().await;
            mount(
                &server,
                status,
                serde_json::json!({"type":"error","error":{"type":"X","message":"m"}}),
            )
            .await;
            assert_eq!(fetch_usage(&server.uri(), Some("k")).await.unwrap_err(), want);
        }
        let server = wiremock::MockServer::start().await;
        mount(&server, 200, serde_json::json!({"nope":true})).await;
        assert_eq!(
            fetch_usage(&server.uri(), Some("k")).await.unwrap_err(),
            UsageError::Unavailable
        );
    }

    #[tokio::test]
    async fn missing_key_never_contacts_the_server() {
        let server = wiremock::MockServer::start().await;
        assert_eq!(
            fetch_usage(&server.uri(), None).await.unwrap_err(),
            UsageError::NoCredential
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
