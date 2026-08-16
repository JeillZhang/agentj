//! Permission error definitions (design 错误处理).
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("permission rule invalid for tool '{tool}': {reason}")]
    InvalidRule { tool: String, reason: String },

    #[error("permission request '{request_id}' timed out after {timeout:?}")]
    RequestTimeout { request_id: String, timeout: Duration },

    #[error("permission request '{request_id}' cancelled: {reason}")]
    RequestCancelled { request_id: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_rule_error_message() {
        let e = PermissionError::InvalidRule {
            tool: "bash".into(),
            reason: "bad glob".into(),
        };
        assert!(e.to_string().contains("bash"));
        assert!(e.to_string().contains("bad glob"));
    }

    #[test]
    fn request_timeout_error_message() {
        let e = PermissionError::RequestTimeout {
            request_id: "r1".into(),
            timeout: std::time::Duration::from_secs(60),
        };
        assert!(e.to_string().contains("r1"));
    }

    #[test]
    fn request_cancelled_error_message() {
        let e = PermissionError::RequestCancelled {
            request_id: "r1".into(),
            reason: "session terminated".into(),
        };
        assert!(e.to_string().contains("r1"));
    }
}
