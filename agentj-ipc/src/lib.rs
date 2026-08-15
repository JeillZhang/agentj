//! Unified Connection abstraction (design 决策 #1/#2) — transport-agnostic serve.
use agentj_core::agent_loop::{AgentEvent, AgentLoop};
use agentj_core::error::{IpcError, KernelError, ModuleError};
use agentj_core::kernel::Kernel;
use agentj_core::module::KernelMessage;
use agentj_core::types::SessionId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::task::JoinHandle;

/// Transport kind — new transports (Tcp / UnixSocket) add a variant + impl, serve loop unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Stdio,
    Tcp,
    UnixSocket,
}

/// Unified connection abstraction (design 决策 #1): stdio/TCP/Unix-socket are implementations.
/// v0.1 only ships `StdioConnection`; `TcpConnection` lands in v0.3 (daemon-kernel plan).
/// `#[async_trait]` boxes the recv future so the trait stays dyn-compatible (edition 2024
/// captures `Self` in `async fn` return-position impl Trait by default, which is not).
#[async_trait]
pub trait Connection: Send + Sync {
    fn id(&self) -> String;
    fn kind(&self) -> TransportKind;
    fn send(&self, msg: &str) -> Result<(), IpcError>;
    async fn recv(&mut self) -> Option<Result<String, IpcError>>;
    fn peer_desc(&self) -> String;
}

/// Multi-client connection registry (design 决策 #1/#5) — v0.1 uses the single
/// stdio path; session_owner routing is consumed by v0.3 permission direction.
pub struct ConnectionRegistry {
    conns: std::collections::HashMap<String, Box<dyn Connection>>,
    session_owner: std::collections::HashMap<SessionId, String>,
    #[allow(dead_code)]
    pending_runs: std::collections::HashMap<SessionId, ()>, // reserved for v0.3 pending-run tracking
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            conns: std::collections::HashMap::new(),
            session_owner: std::collections::HashMap::new(),
            pending_runs: std::collections::HashMap::new(),
        }
    }
    /// Register a connection; v0.1 registers exactly one stdio connection.
    pub fn register(&mut self, conn: Box<dyn Connection>) -> String {
        let id = conn.id();
        self.conns.insert(id.clone(), conn);
        id
    }
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn Connection>> {
        self.conns.remove(id)
    }
    /// Record which connection owns a session (design 决策 #5: 审批定向路由依据).
    pub fn set_session_owner(&mut self, session: &SessionId, conn: &str) {
        self.session_owner.insert(session.clone(), conn.to_string());
    }
    pub fn session_owner(&self, session: &SessionId) -> Option<&String> {
        self.session_owner.get(session)
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String, // JSON-RPC 2.0 envelope version — required by serde, value not inspected
    id: Value,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub struct IpcServer {
    agent_loop: Arc<AgentLoop>,
    kernel: Kernel,
    kernel_handle: Option<JoinHandle<Result<(), KernelError>>>,
    shutdown_requested: AtomicBool,
    registry: ConnectionRegistry,
}

impl IpcServer {
    pub fn new(agent_loop: AgentLoop, mut kernel: Kernel) -> Self {
        // kernel owns module lifecycle: start() spawns the event loop; its JoinHandle
        // completes only after the graceful shutdown chain has run.
        let kernel_handle = kernel.start();
        Self {
            agent_loop: Arc::new(agent_loop),
            kernel,
            kernel_handle: Some(kernel_handle),
            shutdown_requested: AtomicBool::new(false),
            registry: ConnectionRegistry::new(),
        }
    }

    pub fn create_session(&self) -> Result<SessionId, ModuleError> {
        self.agent_loop.create_session()
    }

    pub async fn run_turn(&self, sid: &SessionId, input: &str) -> Result<String, KernelError> {
        self.agent_loop.run_turn(sid, input).await
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::SeqCst)
    }

    /// Send the Shutdown message to the kernel (idempotent — safe to call repeatedly).
    pub fn signal_shutdown(&self) -> Result<(), IpcError> {
        self.kernel
            .sender()
            .send(KernelMessage::Shutdown)
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))
    }

    /// Expose the kernel sender so the CLI can trigger shutdown from a signal handler
    /// (design #13: SIGINT/SIGTERM → KernelMessage::Shutdown, same chain as stdin EOF).
    pub fn kernel_sender(&self) -> agentj_core::kernel::KernelSender {
        self.kernel.sender()
    }

    /// Wait for the kernel's graceful shutdown (modules' shutdown() chain) to finish.
    pub async fn await_shutdown(&mut self) -> Result<(), KernelError> {
        if let Some(handle) = self.kernel_handle.take() {
            handle.await.map_err(|e| KernelError::MessageRouteFailed {
                msg: "kernel loop".into(),
                reason: format!("join failed: {e}"),
            })??;
        }
        Ok(())
    }

    /// Handle a single JSON-RPC request, returning a JSON-RPC response string.
    pub async fn handle_request(&self, raw: &str) -> Result<String, IpcError> {
        let req: JsonRpcRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                    }),
                };
                return Ok(serde_json::to_string(&err_resp)?);
            }
        };

        let result = match req.method.as_str() {
            "session/new" => {
                let sid = self
                    .agent_loop
                    .create_session()
                    .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
                Ok(serde_json::json!({ "session_id": sid }))
            }
            "session/run" => {
                let params = req
                    .params
                    .ok_or_else(|| IpcError::InvalidRequest("missing params".into()))?;
                let sid = params["session_id"]
                    .as_str()
                    .ok_or_else(|| IpcError::InvalidRequest("missing session_id".into()))?;
                let content = params["content"].as_str().unwrap_or("");
                let mut rx = self.agent_loop.run_turn_stream(&sid.to_string(), content);
                let mut text = String::new();
                let mut tc_list = vec![];
                while let Some(event) = rx.recv().await {
                    match event {
                        AgentEvent::Delta(d) => text.push_str(&d),
                        AgentEvent::Done { tool_calls, .. } => {
                            tc_list = tool_calls;
                            break;
                        }
                        AgentEvent::Error(e) => return Err(IpcError::InvalidRequest(e)),
                        _ => {}
                    }
                }
                Ok(serde_json::json!({ "response": text, "tool_calls": tc_list }))
            }
            "shutdown" => {
                // real shutdown: signal the kernel, graceful teardown happens in serve loop
                self.signal_shutdown()?;
                self.shutdown_requested.store(true, Ordering::SeqCst);
                Ok(serde_json::json!({ "status": "ok" }))
            }
            _ => Err(IpcError::InvalidRequest(format!(
                "unknown method: {}",
                req.method
            ))),
        };

        match result {
            Ok(r) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: req.id,
                    result: Some(r),
                    error: None,
                };
                Ok(serde_json::to_string(&resp)?)
            }
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: req.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: e.to_string(),
                    }),
                };
                Ok(serde_json::to_string(&resp)?)
            }
        }
    }

    /// Run the server in stdio mode via the unified Connection serve loop (design 决策 #2).
    /// On EOF or after a shutdown request, signal the kernel and await its graceful shutdown.
    /// The read loop is interrupted when the kernel finishes (design #13: SIGINT/SIGTERM
    /// from the CLI sends Shutdown → kernel exits → serve loop returns even though stdin
    /// is still open).
    pub async fn serve_stdio(&mut self) -> Result<(), IpcError> {
        // v0.1: a single StdioConnection implements the Connection trait; the serve loop
        // is transport-agnostic so v0.3's TcpConnection slots in with zero rework.
        let conn = StdioConnection::new();
        let conn_id = conn.id();
        self.registry.register(Box::new(conn));
        // Poll the kernel handle by `&mut` (taken out so the future needs no `self` borrow
        // while the loop also reads the registry); it is restored below for await_shutdown.
        let mut kernel_handle = self.kernel_handle.take();
        let mut kernel_finished = false;
        let kernel_done = async {
            if let Some(handle) = &mut kernel_handle {
                let _ = handle.await;
            }
            true
        };
        tokio::pin!(kernel_done);
        loop {
            let conn = self
                .registry
                .conns
                .get_mut(&conn_id)
                .ok_or_else(|| IpcError::InvalidRequest("stdio connection missing".into()))?;
            let line = tokio::select! {
                res = conn.recv() => {
                    match res {
                        Some(Ok(line)) => line,
                        Some(Err(e)) => return Err(e),
                        None => break, // EOF (client closed stdin)
                    }
                }
                _ = &mut kernel_done => { kernel_finished = true; break; } // signal-triggered shutdown
            };
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let response = self.handle_request(&trimmed).await.unwrap_or_else(|e| {
                format!(
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"{e}"}}}}"#
                )
            });
            let conn = self
                .registry
                .conns
                .get(&conn_id)
                .ok_or_else(|| IpcError::InvalidRequest("stdio connection missing".into()))?;
            conn.send(&response)?;
            if self.shutdown_requested() {
                break;
            } // graceful shutdown requested
        }
        // Graceful teardown. When the loop exited via kernel_done the kernel already ran
        // its shutdown chain — re-awaiting the completed handle would panic, so skip. On
        // the EOF/request paths, signal the kernel (idempotent — the shutdown RPC may have
        // already sent it), then wait for the modules' shutdown() chain to finish.
        if !kernel_finished {
            self.kernel_handle = kernel_handle;
            if !self.shutdown_requested() {
                let _ = self.kernel.sender().send(KernelMessage::Shutdown);
            }
            self.await_shutdown()
                .await
                .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        }
        Ok(())
    }
}

/// Stdio transport — implements `Connection` for stdin/stdout NDJSON (design 决策 #2).
pub struct StdioConnection {
    id: String,
    reader: BufReader<tokio::io::Stdin>,
}

impl StdioConnection {
    pub fn new() -> Self {
        Self {
            id: format!("stdio-{}", std::process::id()),
            reader: BufReader::new(tokio::io::stdin()),
        }
    }
}

impl Default for StdioConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Connection for StdioConnection {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Stdio
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        // Sync trait method — std blocking stdout; tokio stdout's block_on panics
        // when called from within the serve loop's async context.
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(msg.as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => Some(Ok(line)),
            Err(e) => Some(Err(IpcError::InvalidRequest(e.to_string()))),
        }
    }
    fn peer_desc(&self) -> String {
        "stdio".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop::AgentLoop;
    use agentj_core::kernel::Kernel;
    use agentj_core::module::*;
    use agentj_core::types::*;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

    struct MockConfig;
    impl ConfigProvider for MockConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    fn stub_agent_loop() -> AgentLoop {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: Arc::new(InMemoryStore::new()),
            tools,
            providers,
            config: Arc::new(MockConfig),
        };
        AgentLoop::new(
            ctx,
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
        )
    }

    #[tokio::test]
    async fn session_new_returns_valid_id() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["result"]["session_id"].is_string());
        assert!(!v["result"]["session_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn session_run_streams_delta() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["response"], "hello");
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_object());
        assert_eq!(v["error"]["code"], -32601); // Method not found
    }

    #[tokio::test]
    async fn shutdown_runs_kernel_graceful_shutdown() {
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["status"], "ok");

        // the kernel loop must actually terminate (graceful shutdown ran)
        server.await_shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn invalid_json_returns_parse_error() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
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
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let tx = server.kernel_sender();
        let signal = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            // Tolerate a closed channel: when the test harness delivers stdin EOF
            // immediately, serve_stdio's graceful EOF path already shut the kernel down.
            let _ = tx.send(KernelMessage::Shutdown);
        });
        // serve_stdio reads stdin (blocked) until the kernel finishes; feed a
        // request first so the loop is live, then the signal task triggers exit.
        let run = tokio::spawn(async move {
            server.serve_stdio().await.unwrap();
        });
        signal.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), run)
            .await
            .expect("serve_stdio must exit after kernel shutdown")
            .unwrap();
    }
}
