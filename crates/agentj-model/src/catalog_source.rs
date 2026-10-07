//! Live opencode-go model discovery (design D3).
//!
//! `GET <base_url>/v1/models` is anonymously readable; a failure is surfaced to the
//! caller so it can fall back to the committed overlay without blocking startup.

use agentj_core::error::ModelError;

/// Namespace prefix for every id this catalog source emits.
///
/// The upstream `GET <base_url>/v1/models` answers with bare ids (`kimi-k3`), while the
/// committed overlay, `gen_opencode_catalog::overrides()` and `default_model` all key on
/// `opencode-go/<id>`. Normalizing here keeps the whole pipeline (`from_overlay_and_live`,
/// `catalog_gen::build_overlay`, `auth status`, `kernel/status`) on one namespace.
const NAMESPACE: &str = "opencode-go/";

/// Fetch the live model id list, sorted and de-duplicated, namespaced as `opencode-go/<id>`.
pub async fn fetch_live_models(
    base_url: &str,
    key: Option<&str>,
) -> Result<Vec<String>, ModelError> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let mut req = crate::http::client()
        .get(&url)
        .timeout(std::time::Duration::from_secs(5));
    if let Some(k) = key {
        req = req.header("Authorization", format!("Bearer {k}"));
    }
    let res = req
        .send()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    if status != 200 {
        return Err(ModelError::Api {
            status,
            message: res.text().await.unwrap_or_default(),
        });
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .map(namespaced)
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Prefix a bare upstream id; ids that already carry the namespace are returned unchanged.
pub(crate) fn namespaced(id: &str) -> String {
    if id.starts_with(NAMESPACE) {
        id.to_string()
    } else {
        format!("{NAMESPACE}{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn live_models_are_sorted_and_deduped() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"object":"list","data":[{"id":"opencode-go/b"},{"id":"opencode-go/a"}]}"#,
            ))
            .mount(&server)
            .await;
        assert_eq!(
            fetch_live_models(&server.uri(), None).await.unwrap(),
            vec!["opencode-go/a", "opencode-go/b"]
        );
    }

    #[tokio::test]
    async fn bare_upstream_ids_are_namespaced() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"object":"list","data":[{"id":"kimi-k3"},{"id":"opencode-go/grok-4.5"}]}"#,
            ))
            .mount(&server)
            .await;
        assert_eq!(
            fetch_live_models(&server.uri(), None).await.unwrap(),
            vec!["opencode-go/grok-4.5", "opencode-go/kimi-k3"]
        );
    }

    #[tokio::test]
    async fn unauthorized_surfaces_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&server)
            .await;
        assert!(matches!(
            fetch_live_models(&server.uri(), None).await,
            Err(ModelError::Api { status: 401, .. })
        ));
    }

    #[tokio::test]
    async fn closed_port_is_a_network_error() {
        let err = fetch_live_models("http://127.0.0.1:9", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ModelError::Network(_)));
    }
}
