//! Gateway error-envelope parsing (design D8).
//!
//! Upstream errors are `{"type":"error","error":{"type":T,"message":M}}`. We render
//! them as `"T: M"` so quota/auth failures stay distinguishable, and fall back to the
//! raw body so a message is always non-empty.

use agentj_core::error::ModelError;

/// Map a non-2xx upstream response to a typed error, appending a re-login hint for
/// 401/403 (design D8, shared by all three subscription transports). A context-window
/// rejection becomes [`ModelError::TokenLimitExceeded`] so the kernel can retry smaller.
/// @since 0.29.4
pub(crate) fn api_error(status: u16, body: String) -> ModelError {
    if let Some((used, max)) = context_overflow(status, &body) {
        return ModelError::TokenLimitExceeded { used, max };
    }
    let parsed = parse_gateway_error(status, &body);
    let message = if status == 401 || status == 403 {
        format!("{parsed}（请运行 agentj auth login 重新获取凭据）")
    } else {
        parsed
    };
    ModelError::Api { status, message }
}

/// `(used, max)` when the upstream rejected the request for exceeding the model's context
/// window; `0` where the wording carries no number. The wording is the discriminator, so a
/// 413 without any of the needles (a generic payload-too-large) stays a plain API error.
/// @since 0.29.7
pub fn context_overflow(status: u16, body: &str) -> Option<(usize, usize)> {
    if !matches!(status, 400 | 413) {
        return None;
    }
    let message = parse_gateway_error(status, body).to_ascii_lowercase();
    let overflow = [
        "context_length_exceeded",
        "maximum context length",
        // A max_tokens-vs-window validation error also says "context window", and shrinking
        // the request would not fix it — so that phrasing alone must not match.
        "reduce the length of the messages",
    ]
    .iter()
    .any(|needle| message.contains(needle));
    if !overflow {
        return None;
    }
    Some((
        number_after(&message, "resulted in").unwrap_or(0),
        number_after(&message, "maximum context length is").unwrap_or(0),
    ))
}

/// First integer following `anchor`, digit-group commas ignored (`1048576`, `1,048,576`).
fn number_after(haystack: &str, anchor: &str) -> Option<usize> {
    let rest = &haystack[haystack.find(anchor)? + anchor.len()..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    digits.parse().ok()
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

    /// The upstream wording this was written against (opencode-go, one-shot 400).
    const OVERFLOW_BODY: &str = concat!(
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"Upstream request failed: "#,
        "[context_length_exceeded] This model's maximum context length is 1048576 tokens. ",
        "However, your messages resulted in 3904559 tokens. Please reduce the length of the messages.\"}}",
    );

    #[test]
    fn context_overflow_maps_to_the_typed_error() {
        assert_eq!(
            api_error(400, OVERFLOW_BODY.to_string()).to_string(),
            "context length exceeded: 3904559/1048576 tokens"
        );
    }

    #[test]
    fn unrelated_400_stays_a_plain_api_error() {
        let body =
            r#"{"error":{"type":"invalid_request_error","message":"`temperature` is deprecated"}}"#;
        assert!(matches!(
            api_error(400, body.to_string()),
            ModelError::Api { status: 400, .. }
        ));
    }

    #[test]
    fn unparsable_overflow_reports_unknown_sizes() {
        let body =
            r#"{"error":{"type":"invalid_request_error","message":"context_length_exceeded"}}"#;
        assert!(matches!(
            api_error(400, body.to_string()),
            ModelError::TokenLimitExceeded { used: 0, max: 0 }
        ));
    }

    #[test]
    fn digit_grouped_sizes_are_parsed() {
        let body = concat!(
            r#"{"error":{"type":"invalid_request_error","message":"[context_length_exceeded] "#,
            "maximum context length is 1,048,576 tokens, your messages resulted in 3,904,559 tokens\"}}",
        );
        assert_eq!(
            api_error(400, body.to_string()).to_string(),
            "context length exceeded: 3904559/1048576 tokens"
        );
    }

    /// "context window" alone is a max_tokens-vs-window validation error: shrinking the
    /// request does not fix it, and the retry would replace the real message with `0/0`.
    #[test]
    fn context_window_validation_error_stays_a_plain_api_error() {
        let body = r#"{"error":{"type":"invalid_request_error","message":"max_tokens is too large for this model's context window"}}"#;
        assert!(matches!(
            api_error(400, body.to_string()),
            ModelError::Api { status: 400, .. }
        ));
    }
}
