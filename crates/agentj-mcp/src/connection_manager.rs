//! McpConnectionManager — long-lived MCP connection lifecycle (design §MCP 连接生命周期).
//! T8 TDD: tests land first; the implementation is added in the follow-up commit.

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::module::{ToolRegistry, ToolSource};
    use std::sync::Arc;
    use std::time::Duration;

    fn manager(registry: Arc<ToolRegistry>, idle: Duration) -> Arc<McpConnectionManager> {
        let events = Arc::new(EventBus::new());
        Arc::new(McpConnectionManager::new(
            registry,
            idle,
            Duration::from_millis(500),
            2,
            events,
        ))
    }

    fn echo_script() -> String {
        format!("{}/tests/fixtures/mcp_echo.sh", env!("CARGO_MANIFEST_DIR"))
    }

    #[tokio::test]
    async fn connect_registers_tool_with_source() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        assert_eq!(mgr.state("srv1"), Some(ConnectionState::Ready));
        let tools = registry.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].source, ToolSource::Mcp { server_id: "srv1".into() });
    }

    #[tokio::test]
    async fn execute_round_trips_tool_call() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        let out = mgr.execute("srv1", "echo", serde_json::json!({"text": "hello"})).await.unwrap();
        assert_eq!(out.as_str().unwrap(), "echo: hello");
    }

    #[tokio::test]
    async fn registered_tool_executes_through_manager() {
        // Blocking-item fix: McpSharedTool::execute must delegate to the manager so the
        // AgentLoop path (ctx.tools.get(name) + spawn_blocking(tool.execute)) works for
        // long-lived MCP tools — registered tools must NOT be dead ends.
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        let echo = registry.get("echo").expect("echo tool registered");
        let out = echo.execute(serde_json::json!({"text": "hello"})).unwrap();
        assert_eq!(out.as_str().unwrap(), "echo: hello");
    }

    #[tokio::test]
    async fn crash_mid_call_triggers_unavailable_and_reconnect() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        let crash = format!("{}/tests/fixtures/mcp_crash_after_2.sh", env!("CARGO_MANIFEST_DIR"));
        mgr.connect("srv1".into(), crash).await.unwrap();
        // the fixture serves exactly 2 tool calls, then exits; calls 1-2 succeed, call 3
        // hits a dead server → ServerUnavailable → reconnect (fresh process) → tool works again
        assert!(mgr.execute("srv1", "echo", serde_json::json!({"text": "a"})).await.is_ok());
        assert!(mgr.execute("srv1", "echo", serde_json::json!({"text": "b"})).await.is_ok());
        let result = mgr.execute("srv1", "echo", serde_json::json!({"text": "c"})).await;
        assert!(matches!(result, Err(ToolError::ServerUnavailable { server_id, .. }) if server_id == "srv1"));
        let out = mgr.execute("srv1", "echo", serde_json::json!({"text": "d"})).await;
        assert!(out.is_ok(), "tool must work after reconnect, got: {out:?}");
    }

    #[tokio::test]
    async fn release_to_zero_with_idle_timeout_closes_and_unregisters() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_millis(50));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        assert_eq!(registry.tools().len(), 1);
        mgr.acquire("srv1").unwrap(); // refcount 2
        mgr.release("srv1");          // 1
        mgr.release("srv1");          // 0 → idle timer starts
        tokio::time::sleep(Duration::from_millis(150)).await;
        mgr.reap_idle().await;
        assert_eq!(mgr.state("srv1"), Some(ConnectionState::Closed));
        assert!(registry.tools().is_empty(), "ghost tools must be unregistered on close");
    }

    #[tokio::test]
    async fn reconnect_exhaustion_lands_in_failed() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        // /nonexistent script → every reconnect attempt fails; after max_reconnect → Failed
        mgr.connect("srv1".into(), "/nonexistent/mcp_server.sh".into()).await.unwrap_err();
        assert_eq!(mgr.state("srv1"), Some(ConnectionState::Failed));
    }
}
