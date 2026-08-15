// T12 RED: test only — no McpClient/McpTransport/McpTool implementation yet.
#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::module::ToolRegistry;
    use std::sync::Arc;

    #[tokio::test]
    async fn mcp_client_discovers_and_executes_tool() {
        let tools = Arc::new(ToolRegistry::new());
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp_echo.sh");
        McpClient::connect_and_register(script, &tools).await.unwrap();

        let echo_tool = tools.get("echo").expect("echo tool should be registered");
        let result = echo_tool.execute(serde_json::json!({"text": "hello"})).unwrap();
        assert_eq!(result.as_str().unwrap(), "echo: hello");
    }
}
