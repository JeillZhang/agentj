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
        let err = ModelError::UnknownModel { provider: "opencode-go".into(), model: "opencode-go/nope".into() };
        assert!(err.to_string().contains("opencode-go/nope"));
        assert!(matches!(err, ModelError::UnknownModel { provider, .. } if provider == "opencode-go"));
    }

    #[test]
    fn tool_error_server_unavailable_display() {
        let err = ToolError::ServerUnavailable { server_id: "srv1".into(), reason: "crashed".into() };
        assert!(err.to_string().contains("srv1"));
        assert!(matches!(err, ToolError::ServerUnavailable { server_id, .. } if server_id == "srv1"));
    }

    #[test]
    fn ipc_from_io_error() {
        let io: std::io::Error = std::io::ErrorKind::BrokenPipe.into();
        let err = IpcError::from(io);
        assert!(matches!(err, IpcError::Io(_)));
    }
}
