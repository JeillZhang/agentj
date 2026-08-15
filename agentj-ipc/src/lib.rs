//! agentj-ipc — JSON-RPC 2.0 over stdio via a unified Connection abstraction.
//! T13 RED state: tests only; IpcServer is not implemented yet.

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::agent_loop::AgentLoop;
    use agentj_core::kernel::Kernel;
    use agentj_core::module::*;
    use agentj_core::types::*;
    use agentj_context::ContextPipeline;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

    struct MockConfig;
    impl ConfigProvider for MockConfig {
        fn get_str(&self, _k: &str) -> Option<String> { None }
        fn get_int(&self, _k: &str) -> Option<i64> { None }
        fn providers(&self) -> Vec<(String, String)> { vec![] }
    }

    fn stub_agent_loop() -> AgentLoop {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register("mock".into(), Arc::new(MockModelProvider::new(vec![
            Ok(ModelEvent::Delta("hello".into())),
            Ok(ModelEvent::Done),
        ])));
        let ctx = KernelContext {
            session: Arc::new(InMemoryStore::new()),
            tools,
            providers,
            config: Arc::new(MockConfig),
        };
        AgentLoop::new(ctx, Arc::new(ContextPipeline::new()), "You are helpful.".into())
    }

    #[tokio::test]
    async fn session_new_returns_valid_id() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["result"]["session_id"].is_string());
        assert!(!v["result"]["session_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn session_run_streams_delta() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["response"], "hello");
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_object());
        assert_eq!(v["error"]["code"], -32601); // Method not found
    }

    #[tokio::test]
    async fn shutdown_runs_kernel_graceful_shutdown() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request(r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["status"], "ok");

        // the kernel loop must actually terminate (graceful shutdown ran)
        server.await_shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn invalid_json_returns_parse_error() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request("not json").await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32700); // Parse error
    }

    #[tokio::test]
    async fn kernel_completion_is_observable_for_signal_interrupt() {
        // design #13: the CLI needs to interrupt serve_stdio's blocked read_line when
        // SIGINT/SIGTERM arrives — signal_shutdown + awaiting the kernel handle must
        // terminate the loop. Run serve_stdio with a stdin that stays open, signal
        // shutdown from another task, and assert serve_stdio returns.
        use tokio::io::AsyncWriteExt;
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let tx = server.kernel_sender();
        let signal = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.send(KernelMessage::Shutdown).unwrap();
        });
        // serve_stdio reads stdin (blocked) until the kernel finishes; feed a
        // request first so the loop is live, then the signal task triggers exit.
        let run = tokio::spawn(async move {
            server.serve_stdio().await.unwrap();
        });
        signal.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), run).await
            .expect("serve_stdio must exit after kernel shutdown").unwrap();
    }
}
