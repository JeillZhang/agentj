use crate::types::SessionId;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("module init failed: {0}")]
    Init(String),
    #[error("module shutdown failed: {0}")]
    Shutdown(String),
    #[error("handler error: {0}")]
    Handler(String),
    #[error("session error: {0}")]
    Session(#[from] SessionError),
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session '{0}' not found")]
    NotFound(SessionId),
    #[error("session '{0}' is inactive (archived or deleted)")]
    InactiveSession(SessionId),
    #[error("session persistence failed: {0}")]
    PersistenceFailed(String), // v0.1 contract, reserved for v0.2 disk backend (design #16)
}

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("module '{0}' not found")]
    ModuleNotFound(String),
    #[error("module '{0}' initialization failed: {1}")]
    ModuleInitFailed(String, #[source] anyhow::Error),
    #[error("module '{0}' shutdown failed: {1}")]
    ModuleShutdownFailed(String, #[source] anyhow::Error),
    #[error("message routing failed for {msg:?}: {reason}")]
    MessageRouteFailed { msg: String, reason: String },
    #[error("shutdown timeout after {0:?}")]
    ShutdownTimeout(Duration),
    #[error("IPC error: {0}")]
    Ipc(#[from] IpcError),
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("rate limited, retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },
    #[error("context length exceeded: {used}/{max} tokens")]
    TokenLimitExceeded { used: usize, max: usize },
    #[error("unknown model '{model}' for provider '{provider}'")]
    UnknownModel { provider: String, model: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("stream error: {0}")]
    Stream(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("tool '{name}' not found")]
    ToolNotFound { name: String },
    #[error("tool '{name}' execution failed: {reason}")]
    ExecutionFailed { name: String, reason: String },
    #[error("tool '{name}' permission denied: {reason}")]
    PermissionDenied { name: String, reason: String },
    #[error("tool timeout after {0:?}")]
    Timeout(Duration),
    // v0.1 contract-first (design error section): MCP server crash/disconnect mid-call.
    // Consumed by the v0.2 McpConnectionManager; defined now so the contract is stable.
    #[error("MCP server '{server_id}' unavailable: {reason}")]
    ServerUnavailable { server_id: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_error_display() {
        let err = KernelError::ModuleNotFound("test".into());
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn kernel_error_shutdown_failed_display() {
        let err = KernelError::ModuleShutdownFailed("m".into(), anyhow::anyhow!("boom"));
        assert!(err.to_string().contains("m"));
    }

    #[test]
    fn model_error_network_carries_message() {
        let err = ModelError::Network("conn refused".into());
        assert!(matches!(err, ModelError::Network(_)));
    }

    #[test]
    fn model_error_unknown_model_display() {
        let err = ModelError::UnknownModel {
            provider: "opencode-go".into(),
            model: "opencode-go/nope".into(),
        };
        assert!(err.to_string().contains("opencode-go/nope"));
        assert!(
            matches!(err, ModelError::UnknownModel { provider, .. } if provider == "opencode-go")
        );
    }

    #[test]
    fn tool_error_server_unavailable_display() {
        let err = ToolError::ServerUnavailable {
            server_id: "srv1".into(),
            reason: "crashed".into(),
        };
        assert!(err.to_string().contains("srv1"));
        assert!(
            matches!(err, ToolError::ServerUnavailable { server_id, .. } if server_id == "srv1")
        );
    }

    #[test]
    fn ipc_from_io_error() {
        let io: std::io::Error = std::io::ErrorKind::BrokenPipe.into();
        let err = IpcError::from(io);
        assert!(matches!(err, IpcError::Io(_)));
    }
}
