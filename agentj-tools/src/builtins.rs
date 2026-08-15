// In agentj-tools/src/builtins.rs (tests module)
#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ToolError;
    use agentj_core::module::Tool;
    use std::sync::Arc;

    #[test]
    fn read_tool_reads_file() -> Result<(), ToolError> {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world")?;

        let tool = ReadTool;
        let result = tool.execute(serde_json::json!({"path": file_path.to_str().unwrap()}))?;
        assert_eq!(result.as_str().unwrap(), "hello world");
        Ok(())
    }

    #[test]
    fn bash_tool_timeout_detected() {
        let tool = Arc::new(BashTool);
        let started = std::time::Instant::now();
        let result = execute_with_timeout(
            &tool,
            serde_json::json!({"command": "sleep 5"}),
            std::time::Duration::from_millis(100),
        );
        assert!(matches!(result, Err(ToolError::Timeout(_))));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
