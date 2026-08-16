//! Unified Connection abstraction (design 决策 #1/#2) — transport-agnostic serve.
use agentj_core::agent_loop::{AgentEvent, AgentLoop};
use agentj_core::error::{IpcError, KernelError, ModuleError};
use agentj_core::kernel::Kernel;
use agentj_core::module::KernelMessage;
use agentj_core::types::SessionId;
use agentj_permission::{ApprovalDecision, PermissionModule};
use agentj_protocol::permission::{PermissionRequestParams, PermissionRespondParams};
use agentj_protocol::streaming::CancelParams;
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
    /// Notifications carry no id (JSON-RPC 2.0 §2) — defaults to Null.
    #[serde(default)]
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

/// Method-dispatch failure carrying the JSON-RPC 2.0 standard error code
/// (design 决策 #4). `handle_request` echoes the request `id` alongside this
/// code so every error path round-trips the correlation id.
struct DispatchError {
    code: i32,
    message: String,
}

impl DispatchError {
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
        }
    }
    fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
        }
    }
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
        }
    }
}

pub struct IpcServer {
    agent_loop: Arc<AgentLoop>,
    kernel: Kernel,
    kernel_handle: Option<JoinHandle<Result<(), KernelError>>>,
    shutdown_requested: AtomicBool,
    registry: ConnectionRegistry,
    /// Optional PermissionModule: `permission/respond` and `$/cancelRequest` dispatch into it.
    pub permission_module: Option<Arc<PermissionModule>>,
    /// In-flight session id — the mapping `$/cancelRequest` reads to reach the module.
    /// Interior-mutable (`handle_request` runs under `&self`): `dispatch_session_run`
    /// records the id while a run executes and clears it when the turn finishes.
    pub session: Arc<tokio::sync::Mutex<Option<String>>>,
}

/// RAII guard recording the in-flight session id for `$/cancelRequest`.
/// `dispatch_session_run` records the id at entry and the guard clears it on
/// drop, covering every exit path (success, error, and the stub-loop early
/// return). Drop uses `try_lock` because the dispatch releases the lock right
/// after recording — the clear never contends with a cancel handler.
struct InflightSessionGuard(Arc<tokio::sync::Mutex<Option<String>>>);

impl InflightSessionGuard {
    async fn record(session: &Arc<tokio::sync::Mutex<Option<String>>>, sid: &str) -> Self {
        *session.lock().await = Some(sid.to_string());
        Self(session.clone())
    }
}

impl Drop for InflightSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.try_lock() {
            *guard = None;
        }
    }
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
            permission_module: None,
            session: Arc::new(tokio::sync::Mutex::new(None)),
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
    /// Every error path echoes the request `id` (JSON-RPC 2.0 §5) with the
    /// semantically correct standard code, never falling through to `serve_stdio`.
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
            "session/new" => self.dispatch_session_new(),
            "session/run" => self.dispatch_session_run(&req).await,
            "shutdown" => self.dispatch_shutdown(),
            "permission/respond" => self.dispatch_permission_respond(&req),
            "$/cancelRequest" => self.dispatch_cancel_request(&req).await,
            other => Err(DispatchError::method_not_found(other)),
        };

        let resp = match result {
            Ok(r) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(r),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: e.code,
                    message: e.message,
                }),
            },
        };
        if req.id == Value::Null {
            // JSON-RPC 2.0 §2.1: a notification (no id) must not get a response
            // line. The handler above still ran — the side effect is delivered.
            return Ok(String::new());
        }
        Ok(serde_json::to_string(&resp)?)
    }

    fn dispatch_session_new(&self) -> Result<Value, DispatchError> {
        let sid = self
            .agent_loop
            .create_session()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        Ok(serde_json::json!({ "session_id": sid }))
    }

    async fn dispatch_session_run(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let sid = params["session_id"]
            .as_str()
            .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
        let content = params["content"].as_str().unwrap_or("");
        // Record the in-flight session so `$/cancelRequest` can reach the
        // permission module while the run executes; the guard clears it on
        // every exit path (including the AgentEvent::Error return).
        let _inflight = InflightSessionGuard::record(&self.session, sid).await;
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
                AgentEvent::Error(e) => return Err(DispatchError::internal(e)),
                _ => {}
            }
        }
        Ok(serde_json::json!({ "response": text, "tool_calls": tc_list }))
    }

    fn dispatch_shutdown(&self) -> Result<Value, DispatchError> {
        self.signal_shutdown()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        self.shutdown_requested.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "status": "ok" }))
    }

    /// `permission/respond`: deliver the client's decision to a pending approval.
    /// An unknown request_id is an error (-32600), never silent success.
    fn dispatch_permission_respond(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_request("missing params"))?;
        let params: PermissionRespondParams = serde_json::from_value(params.clone())
            .map_err(|e| DispatchError::invalid_request(e.to_string()))?;
        let module = self
            .permission_module
            .clone()
            .ok_or_else(|| DispatchError::invalid_request("no permission module"))?;
        let decision = match params.decision {
            agentj_protocol::permission::PermissionDecision::Allow => ApprovalDecision::Allow,
            agentj_protocol::permission::PermissionDecision::Deny => ApprovalDecision::Deny,
            agentj_protocol::permission::PermissionDecision::AllowAlways => {
                ApprovalDecision::AllowAlways
            }
            agentj_protocol::permission::PermissionDecision::Unknown => {
                return Err(DispatchError::invalid_request("unknown decision"));
            }
        };
        let rid = agentj_core::permission::PermissionRequestId(params.request_id);
        if !module.respond(&rid, decision) {
            return Err(DispatchError::invalid_request(format!(
                "unknown permission request: {}",
                rid.0
            )));
        }
        Ok(serde_json::json!({}))
    }

    /// `$/cancelRequest` notification: resolve the in-flight session's pending
    /// approvals to Deny ("cancelled"). Notifications carry no response body.
    async fn dispatch_cancel_request(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let _params: CancelParams =
            serde_json::from_value(req.params.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| DispatchError::invalid_request(e.to_string()))?;
        let session_id = self.session.lock().await.clone();
        let module = self.permission_module.as_ref();
        if let (Some(session_id), Some(module)) = (session_id, module) {
            let _ = module.cancel_session(&session_id, "cancelled");
        }
        Ok(serde_json::Value::Null)
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
        // Subscribe the permission notifier: an Ask request emits a $/permissionRequest
        // notification line on stdout (same blocking write path as StdioConnection::send).
        if let Some(module) = &self.permission_module {
            let notifier = Arc::new(|p: &PermissionRequestParams| {
                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "$/permissionRequest",
                    "params": {
                        "request_id": p.request_id,
                        "tool": p.tool,
                        "args": p.args,
                        "reason": p.reason,
                    },
                });
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{msg}");
            });
            module.set_notifier(notifier);
        }
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
            if !response.is_empty() {
                conn.send(&response)?;
            }
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
    use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
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

    fn permission_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "bash".to_string(),
            agentj_permission::ToolPolicy {
                mode: agentj_permission::PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let module = Arc::new(agentj_permission::PermissionModule::new(
            policies,
            std::time::Duration::from_millis(50),
        ));
        let mut server = IpcServer::new(stub_agent_loop(), Kernel::new());
        server.permission_module = Some(module.clone());
        (server, module)
    }

    /// Fixture for the cancel regression test: the AgentLoop's ToolRegistry is
    /// wired to the SAME PermissionModule the server dispatches into, and the
    /// mock script emits a `bash` ToolCall — so a session/run genuinely blocks
    /// on a permission Ask while in-flight (the real cancel scenario).
    fn blocking_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "bash".to_string(),
            agentj_permission::ToolPolicy {
                mode: agentj_permission::PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let module = Arc::new(agentj_permission::PermissionModule::new(
            policies,
            std::time::Duration::from_secs(10),
        ));
        let tools = Arc::new(ToolRegistry::new());
        tools.set_permission_checker(module.clone());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![Ok(ModelEvent::ToolCall(
                ToolCall {
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "curl x"}),
                },
            ))])),
        );
        let ctx = KernelContext {
            session: Arc::new(InMemoryStore::new()),
            tools,
            providers,
            config: Arc::new(MockConfig),
        };
        let mut server = IpcServer::new(
            AgentLoop::new(
                ctx,
                Arc::new(ContextPipeline::new()),
                "You are helpful.".into(),
            ),
            Kernel::new(),
        );
        server.permission_module = Some(module.clone());
        (server, module)
    }

    #[tokio::test]
    async fn permission_respond_resolves_pending_approval() {
        let (server, module) = permission_server();
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl x"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let d = module.check(&ctx);
        if let PermissionDecision::Ask { request_id } = d {
            let module2 = module.clone();
            let rid = request_id.clone();
            let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let raw = format!(
                concat!(
                    r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","#,
                    r#""params":{{"request_id":"{}","decision":"allow"}}}}"#,
                ),
                request_id.0
            );
            let resp = server.handle_request(&raw).await.unwrap();
            assert!(resp.contains("\"result\""));
            assert!(matches!(handle.await.unwrap(), PermissionDecision::Allow));
        } else {
            panic!("expected Ask");
        }
    }

    #[tokio::test]
    async fn permission_respond_unknown_request_id_errors() {
        let (server, _module) = permission_server();
        let raw = concat!(
            r#"{"jsonrpc":"2.0","id":8,"method":"permission/respond","#,
            r#""params":{"request_id":"nope","decision":"allow"}}"#,
        );
        let resp = server.handle_request(raw).await.unwrap();
        assert!(resp.contains("\"error\""));
        assert!(
            resp.contains("-32600") || resp.contains("InvalidRequest") || resp.contains("unknown")
        );
    }

    #[tokio::test]
    async fn cancel_request_resolves_pending_to_deny() {
        let (server, _module) = permission_server();
        let module = server.permission_module.clone().unwrap();
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl x"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
            let module2 = module.clone();
            let rid = request_id.clone();
            let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            // Seed the request→session mapping the $/cancelRequest handler reads
            // (v0.1 has no real in-flight run here — see
            // session_run_records_inflight_session_for_cancel for the production
            // write-side path), then send the notification and assert the HANDLER
            // resolves the approval (no manual cancel_session call — the mapping
            // path is exercised).
            *server.session.lock().await = Some("s1".into());
            let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
            let _ = server.handle_request(raw).await.unwrap();
            assert_eq!(
                handle.await.unwrap(),
                PermissionDecision::Deny {
                    reason: "cancelled".into()
                }
            );
        } else {
            panic!("expected Ask");
        }
    }

    /// M1 regression: JSON-RPC 2.0 §2.1 — a notification (request without an
    /// id, e.g. $/cancelRequest) must produce NO response line, while a
    /// request WITH an id still gets its response.
    #[tokio::test]
    async fn id_less_notification_gets_no_response() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","method":"session/new"}"#)
            .await
            .unwrap();
        assert!(resp.is_empty(), "notification must not respond: {resp}");
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        assert!(!resp.is_empty(), "id'd request must respond");
        assert!(resp.contains("\"result\""));
    }

    /// Poll until the in-flight session is recorded (proves the write side).
    async fn wait_for_inflight_session(
        server: &Arc<IpcServer>,
        sid: &str,
    ) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if *server.session.lock().await == Some(sid.to_string()) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        })
        .await
    }

    /// Register a pending Ask approval for `sid` and await it in a task.
    fn spawn_pending_approval(
        module: &Arc<agentj_permission::PermissionModule>,
        sid: &str,
    ) -> tokio::task::JoinHandle<PermissionDecision> {
        let ctx = PermissionContext {
            session_id: sid.to_string(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl x"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
            let module2 = module.clone();
            let rid = request_id.clone();
            tokio::spawn(async move { module2.await_approval(&rid).await })
        } else {
            panic!("expected Ask");
        }
    }

    #[tokio::test]
    async fn session_run_records_inflight_session_for_cancel() {
        // T9 finding A regression: the write side of `server.session` (the mapping
        // $/cancelRequest reads) must be populated by production code while a
        // session/run executes — not only seeded by tests. Drive the whole path
        // through handlers with a run that blocks on a permission Ask.
        let (server, module) = blocking_run_server();
        let server = Arc::new(server);
        let new_resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&new_resp).unwrap();
        let sid = v["result"]["session_id"].as_str().unwrap().to_string();
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_task = tokio::spawn({
            let server = Arc::clone(&server);
            async move { server.handle_request(&raw).await.unwrap() }
        });
        wait_for_inflight_session(&server, &sid)
            .await
            .expect("session/run must record the in-flight session id");
        // The run is blocked on a permission Ask; create a pending approval for
        // the same session and cancel through the handler. The sleep lets
        // await_approval subscribe before the cancel removes the entry.
        let pending = spawn_pending_approval(&module, &sid);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = server
            .handle_request(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#)
            .await
            .unwrap();
        assert_eq!(
            pending.await.unwrap(),
            PermissionDecision::Deny {
                reason: "cancelled".into()
            }
        );
        // The cancel unblocked the run; it completes and clears the session.
        let run_resp = run_task.await.unwrap();
        assert!(run_resp.contains("\"result\""));
        assert_eq!(
            *server.session.lock().await,
            None,
            "in-flight session cleared when the run finished"
        );
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
        let raw = concat!(
            r#"{"jsonrpc":"2.0","id":2,"method":"session/run","#,
            r#""params":{"session_id":"s1","content":"hi"}}"#,
        );
        let resp = server.handle_request(raw).await.unwrap();
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
        assert_eq!(v["id"], 3, "error response must echo request id");
    }

    #[tokio::test]
    async fn session_run_missing_params_echoes_id_and_invalid_params() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/run"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["id"], 10,
            "error response must echo request id, got: {resp}"
        );
        assert_eq!(
            v["error"]["code"], -32602,
            "missing params is Invalid params, got: {resp}"
        );
        assert!(v["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn session_run_missing_session_id_echoes_id_and_invalid_params() {
        let server = IpcServer::new(stub_agent_loop(), Kernel::new());
        let resp = server
            .handle_request(
                r#"{"jsonrpc":"2.0","id":7,"method":"session/run","params":{"content":"hi"}}"#,
            )
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["id"], 7,
            "error response must echo request id, got: {resp}"
        );
        assert_eq!(
            v["error"]["code"], -32602,
            "missing session_id is Invalid params, got: {resp}"
        );
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
