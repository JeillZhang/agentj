//! Shared upstream-send policy for the three subscription wire formats (0.29.4).
//!
//! Two upstream behaviours are handled in one place so `anthropic` / `openai_compat` /
//! `responses` cannot drift:
//!
//! 1. `429` → [`ModelError::RateLimited`] carrying the server's `Retry-After` hint, so the
//!    `chat_stream` backoff loop can honour it (design error 模型层).
//! 2. `400` whose message says `temperature` is unsupported/deprecated by the model (e.g.
//!    opencode-go `claude-haiku-5-5`, `gpt-6-luna`) → retry **once** with the field removed.
//!    The design keeps sending the 0.7 default to models that accept it; this only adapts
//!    the models that reject it.

use agentj_core::error::ModelError;
use reqwest::{RequestBuilder, Response};
use serde_json::Value;
use std::time::Duration;

/// True when a `400` body blames `temperature` for the rejection.
pub(crate) fn temperature_unsupported(status: u16, body: &str) -> bool {
    if status != 400 {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("temperature")
        && (lower.contains("deprecated")
            || lower.contains("not supported")
            || lower.contains("unsupported"))
}

/// A copy of `body` with the top-level `temperature` field removed.
pub(crate) fn without_temperature(body: &Value) -> Value {
    let mut out = body.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.remove("temperature");
    }
    out
}

fn rate_limited(response: &Response) -> ModelError {
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    ModelError::RateLimited { retry_after }
}

/// POST `body` through `post` and return the successful response.
///
/// `post` must attach the transport's own headers and JSON body; it is called once, or
/// twice when the first attempt is rejected for `temperature`. All non-2xx responses are
/// mapped here, so callers only handle the `200` case. @since 0.29.4
pub(crate) async fn send<F>(body: &Value, post: F) -> Result<Response, ModelError>
where
    F: Fn(&Value) -> RequestBuilder,
{
    let response = post(body)
        .send()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    resolve(response, body, &post).await
}

/// Classify `response`; retry without `temperature` when the upstream blames it.
async fn resolve<F>(response: Response, body: &Value, post: &F) -> Result<Response, ModelError>
where
    F: Fn(&Value) -> RequestBuilder,
{
    let status = response.status().as_u16();
    if status == 200 {
        return Ok(response);
    }
    if status == 429 {
        return Err(rate_limited(&response));
    }
    let text = response.text().await.unwrap_or_default();
    if !temperature_unsupported(status, &text) {
        return Err(crate::gateway_error::api_error(status, text));
    }
    let retry = post(&without_temperature(body))
        .send()
        .await
        .map_err(|e| ModelError::Network(e.to_string()))?;
    let retry_status = retry.status().as_u16();
    if retry_status == 200 {
        return Ok(retry);
    }
    if retry_status == 429 {
        return Err(rate_limited(&retry));
    }
    let retry_text = retry.text().await.unwrap_or_default();
    Err(crate::gateway_error::api_error(retry_status, retry_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_temperature_rejection_shapes() {
        assert!(temperature_unsupported(
            400,
            "{\"error\":{\"message\":\"`temperature` is deprecated for this model.\"}}"
        ));
        assert!(temperature_unsupported(
            400,
            "Unsupported parameter: 'temperature' is not supported with this model."
        ));
        assert!(!temperature_unsupported(400, "invalid model"));
        assert!(!temperature_unsupported(500, "temperature exploded"));
        assert!(!temperature_unsupported(
            400,
            "context length exceeded (temperature fine)"
        ));
    }

    #[test]
    fn without_temperature_keeps_other_fields() {
        let body = json!({"model": "m", "temperature": 0.7, "stream": true});
        let stripped = without_temperature(&body);
        assert!(stripped.get("temperature").is_none());
        assert_eq!(stripped["model"], "m");
        assert_eq!(stripped["stream"], true);
        // The original body is left untouched.
        assert_eq!(body["temperature"], 0.7);
    }
}
