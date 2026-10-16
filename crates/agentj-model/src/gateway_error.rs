//! Gateway error-envelope parsing (design D8).
//!
//! Upstream errors are `{"type":"error","error":{"type":T,"message":M}}`. We render
//! them as `"T: M"` so quota/auth failures stay distinguishable, and fall back to the
//! raw body so a message is always non-empty.

use agentj_core::error::ModelError;

/// Map a non-2xx upstream response to [`ModelError::Api`], appending a re-login hint for
/// 401/403 (design D8, shared by all three subscription transports). @since 0.29.4
pub(crate) fn api_error(status: u16, body: String) -> ModelError {
    let parsed = parse_gateway_error(status, &body);
    let message = if status == 401 || status == 403 {
        format!("{parsed}（请运行 agentj auth login 重新获取凭据）")
    } else {
        parsed
    };
    ModelError::Api { status, message }
}

/// Parse an upstream error body into a human-readable message.
pub fn parse_gateway_error(_status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            Some((
                v["error"]["type"].as_str()?.to_string(),
                v["error"]["message"].as_str()?.to_string(),
            ))
        })
        .map(|(t, m)| format!("{t}: {m}"))
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_error_message_carries_error_type() {
        let body =
            r#"{"type":"error","error":{"type":"GoUsageLimitError","message":"monthly limit"}}"#;
        assert_eq!(
            parse_gateway_error(429, body),
            "GoUsageLimitError: monthly limit"
        );
    }

    #[test]
    fn non_envelope_body_falls_back_to_raw_text() {
        assert_eq!(parse_gateway_error(500, "boom"), "boom");
    }
}
