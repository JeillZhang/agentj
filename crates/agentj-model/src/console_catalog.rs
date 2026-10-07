//! Account-level opencode-go catalog (`GET /api/v2/config`, design D5). @since 0.27.0
//!
//! The Console config endpoint is workspace-scoped: any key can read it, and it reflects the
//! models actually selectable for that workspace (including workspace-disabled differences).
//! It is the highest-priority `model/list` tier; every failure is an `Err` so the caller can
//! fall back to live/overlay — this never returns an empty `Ok`.

use crate::catalog_source::namespaced;
use crate::http;
use agentj_core::error::ModelError;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);
const GO_BASE_MARKER: &str = "/zen/go";

/// Fetch the account-scoped model ids (namespaced, sorted, de-duplicated).
pub async fn fetch_account_models(
    console_base: &str,
    key: &str,
    org_id: Option<&str>,
) -> Result<Vec<String>, ModelError> {
    let url = format!("{}/api/v2/config", console_base.trim_end_matches('/'));
    let mut req = http::client().get(&url).bearer_auth(key).timeout(TIMEOUT);
    if let Some(org) = org_id {
        req = req.header("x-org-id", org);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    if resp.status().as_u16() != 200 {
        return Err(ModelError::Api {
            status: resp.status().as_u16(),
            message: resp.text().await.unwrap_or_default(),
        });
    }
    let body = resp
        .text()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ModelError::Stream(e.to_string()))?;
    let mut ids: Vec<String> = v["providers"]
        .as_object()
        .map(|providers| {
            providers
                .values()
                .filter(|p| {
                    p["settings"]["baseURL"]
                        .as_str()
                        .unwrap_or_default()
                        .contains(GO_BASE_MARKER)
                })
                .flat_map(|p| {
                    p["models"]
                        .as_object()
                        .map(|m| m.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                })
                .map(|id| namespaced(&id))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        Err(ModelError::Stream(
            "account catalog has no opencode-go models".into(),
        ))
    } else {
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_body() -> serde_json::Value {
        serde_json::json!({"providers":{
            "opencode":{"name":"opencode","settings":{"baseURL":"https://opencode.ai/zen/go"},
                        "models":{"kimi-k3":{},"minimax-m3":{}}},
            "anthropic":{"name":"Anthropic","settings":{"baseURL":"https://api.anthropic.com"},
                         "models":{"claude-x":{}}}}})
    }

    #[tokio::test]
    async fn picks_the_go_provider_and_namespaces_ids() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v2/config"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(v2_body()))
            .mount(&server)
            .await;
        let ids = fetch_account_models(&server.uri(), "k", Some("org_1"))
            .await
            .unwrap();
        assert_eq!(ids, vec!["opencode-go/kimi-k3", "opencode-go/minimax-m3"]);
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs[0].headers.get("x-org-id").unwrap(), "org_1");
    }

    #[tokio::test]
    async fn errors_instead_of_returning_an_empty_catalog() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"providers":{}})),
            )
            .mount(&server)
            .await;
        assert!(
            fetch_account_models(&server.uri(), "k", None)
                .await
                .is_err()
        );
        assert!(
            server.received_requests().await.unwrap()[0]
                .headers
                .get("x-org-id")
                .is_none()
        );

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(
            fetch_account_models(&server.uri(), "k", None)
                .await
                .is_err()
        );
    }
}
