//! Unified Connection abstraction (design 决策 #1/#2) — transport-agnostic serve.
pub mod tcp;

use agentj_core::agent_loop::{AgentEvent, AgentLoop};
use agentj_core::error::{IpcError, KernelError, ModuleError};
use agentj_core::kernel::Kernel;
use agentj_core::module::KernelMessage;
use agentj_core::types::SessionId;
use agentj_permission::{ApprovalDecision, PermissionModule};
use agentj_protocol::base::error_codes;
use agentj_protocol::lifecycle::InitializeParams;
use agentj_protocol::permission::{PermissionRequestParams, PermissionRespondParams};
use agentj_protocol::session::{SessionIdParams, SessionSubscribeParams};
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
    pending_runs: std::collections::HashSet<SessionId>,
    subscribers: std::collections::HashMap<SessionId, std::collections::HashSet<String>>,
    /// Connections that passed `initialize` auth on a token-gated server.
    authorized: std::collections::HashSet<String>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            conns: std::collections::HashMap::new(),
            session_owner: std::collections::HashMap::new(),
            pending_runs: std::collections::HashSet::new(),
            subscribers: std::collections::HashMap::new(),
            authorized: std::collections::HashSet::new(),
        }
    }
    /// Register a connection; v0.1 registers exactly one stdio connection.
    pub fn register(&mut self, conn: Box<dyn Connection>) -> String {
        let id = conn.id();
        self.conns.insert(id.clone(), conn);
        id
    }
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn Connection>> {
        self.authorized.remove(id);
        self.session_owner.retain(|_, owner| owner != id);
        for subs in self.subscribers.values_mut() {
            subs.remove(id);
        }
        self.subscribers.retain(|_, s| !s.is_empty());
        self.conns.remove(id)
    }
    /// Record which connection owns a session (design 决策 #5: 审批定向路由依据).
    pub fn set_session_owner(&mut self, session: &SessionId, conn: &str) {
        self.session_owner.insert(session.clone(), conn.to_string());
    }
    pub fn session_owner(&self, session: &SessionId) -> Option<&String> {
        self.session_owner.get(session)
    }
    /// Record that a connection passed `initialize` auth (token-gated mode).
    pub fn mark_authorized(&mut self, conn_id: &str) {
        self.authorized.insert(conn_id.to_string());
    }
    pub fn is_authorized(&self, conn_id: &str) -> bool {
        self.authorized.contains(conn_id)
    }
    /// Route a message to one connection; a vanished peer is not an error
    /// (design 错误处理: drop the response instead of failing the caller).
    pub fn send_to(&self, conn_id: &str, msg: &str) -> Result<(), IpcError> {
        match self.conns.get(conn_id) {
            Some(conn) => conn.send(msg),
            None => Ok(()),
        }
    }
    /// Subscribe a connection to a session's broadcast channel (决策 #4).
    pub fn subscribe(&mut self, session: &SessionId, conn_id: &str) {
        self.subscribers
            .entry(session.clone())
            .or_default()
            .insert(conn_id.to_string());
    }
    pub fn unsubscribe(&mut self, session: &SessionId, conn_id: &str) {
        if let Some(subs) = self.subscribers.get_mut(session) {
            subs.remove(conn_id);
            if subs.is_empty() {
                self.subscribers.remove(session);
            }
        }
    }
    /// Broadcast to every subscriber of a session (决策 #4); the owner must be
    /// registered as a subscriber by the dispatch layer to also receive it.
    pub fn broadcast_to_session(&self, session: &SessionId, msg: &str) {
        if let Some(subs) = self.subscribers.get(session) {
            for conn_id in subs {
                let _ = self.send_to(conn_id, msg);
            }
        }
    }
    /// Mutual exclusion for concurrent runs on one session (决策 #7):
    /// returns false while a run is in flight (`-32805 SessionBusy`).
    pub fn try_acquire_run(&mut self, session: &SessionId) -> bool {
        self.pending_runs.insert(session.clone())
    }
    pub fn release_run(&mut self, session: &SessionId) {
        self.pending_runs.remove(session);
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
    /// Shared registry: multi-client routing mutates it from `&self` handlers
    /// and the T8 sync notifier. Sync lock (send/recv paths never await while
    /// held — serve_stdio detaches a connection before its await points).
    registry: Arc<std::sync::RwLock<ConnectionRegistry>>,
    /// Shared access token for authorized clients (daemon-kernel --token);
    /// checked by `initialize` dispatch when Some.
    token: std::sync::Mutex<Option<String>>,
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

/// RAII guard releasing the per-session run mutex (决策 #7): acquired at
/// dispatch entry; Drop covers every exit path so a crashed run never leaves
/// `-32805 SessionBusy` stuck on.
struct RunGuard {
    registry: Arc<std::sync::RwLock<ConnectionRegistry>>,
    sid: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.registry.write() {
            reg.release_run(&self.sid);
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
            registry: Arc::new(std::sync::RwLock::new(ConnectionRegistry::new())),
            token: std::sync::Mutex::new(None),
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

    /// Configure the access token required by `initialize`; None disables auth.
    pub fn set_token(&self, token: Option<String>) {
        if let Ok(mut t) = self.token.lock() {
            *t = token;
        }
    }

    /// Send the Shutdown message to the kernel (idempotent — safe to call repeatedly).
    /// Also marks shutdown as requested so transport accept-loops polling
    /// `shutdown_requested()` observe the signal (TCP last-disconnect path).
    pub fn signal_shutdown(&self) -> Result<(), IpcError> {
        self.shutdown_requested.store(true, Ordering::SeqCst);
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

    /// Handle a single JSON-RPC request on the stdio sentinel connection
    /// ("stdio") — v0.1 single-client entry, unchanged behavior.
    pub async fn handle_request(&self, raw: &str) -> Result<String, IpcError> {
        self.handle_request_from(raw, "stdio").await
    }

    /// Handle a single JSON-RPC request from connection `conn_id`. Every error
    /// path echoes the request `id` (JSON-RPC 2.0 §5) with the semantically
    /// correct standard code, never falling through to the serve loop.
    pub async fn handle_request_from(&self, raw: &str, conn_id: &str) -> Result<String, IpcError> {
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

        let result = if let Some(err) = self.auth_gate(&req, conn_id) {
            Err(err)
        } else {
            match req.method.as_str() {
                "initialize" => self.dispatch_initialize(&req, conn_id),
                "session/new" => self.dispatch_session_new(conn_id),
                "session/run" => self.dispatch_session_run(&req, conn_id).await,
                "session/subscribe" => self.dispatch_subscribe(&req, conn_id),
                "session/unsubscribe" => self.dispatch_unsubscribe(&req, conn_id),
                "shutdown" => self.dispatch_shutdown(),
                "permission/respond" => self.dispatch_permission_respond(&req, conn_id).await,
                "$/cancelRequest" => self.dispatch_cancel_request(&req).await,
                other => Err(DispatchError::method_not_found(other)),
            }
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

    /// Token-mode auth gate: when a token is configured, only connections
    /// that already passed `initialize` may issue business methods. The
    /// `initialize` method itself always reaches `dispatch_initialize` (which
    /// performs the token check and marks the connection authorized). stdio
    /// serves with no token, so this gate is inert on that path.
    fn auth_gate(&self, req: &JsonRpcRequest, conn_id: &str) -> Option<DispatchError> {
        if req.method.as_str() == "initialize" {
            return None;
        }
        let token_configured = self.token.lock().ok().map(|t| t.is_some()).unwrap_or(false);
        if !token_configured {
            return None;
        }
        let authorized = self
            .registry
            .read()
            .ok()
            .map(|r| r.is_authorized(conn_id))
            .unwrap_or(false);
        if !authorized {
            return Some(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
            });
        }
        None
    }

    fn dispatch_initialize(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: InitializeParams =
            serde_json::from_value(req.params.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(expected) = self.token.lock()
            && let Some(expected) = expected.as_deref()
            && params.token.as_deref().unwrap_or("") != expected
        {
            return Err(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
            });
        }
        // Successful auth: the connection may now issue business methods on a
        // token-gated server (the non-initialize gate checks this registry).
        if let Ok(mut reg) = self.registry.write() {
            reg.mark_authorized(conn_id);
        }
        let version = env!("CARGO_PKG_VERSION");
        Ok(serde_json::json!({
            "protocol_version": version,
            "capabilities": { "multi_client": true },
            "server_info": { "name": "agentj", "version": version },
            "connection_id": conn_id,
        }))
    }

    fn dispatch_subscribe(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: SessionSubscribeParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(mut reg) = self.registry.write() {
            reg.subscribe(&params.session_id, conn_id);
        }
        Ok(serde_json::json!({}))
    }

    fn dispatch_unsubscribe(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(mut reg) = self.registry.write() {
            reg.unsubscribe(&params.session_id, conn_id);
        }
        Ok(serde_json::json!({}))
    }

    fn dispatch_session_new(&self, conn_id: &str) -> Result<Value, DispatchError> {
        let sid = self
            .agent_loop
            .create_session()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid, conn_id);
        }
        Ok(serde_json::json!({ "session_id": sid }))
    }

    async fn dispatch_session_run(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let sid = params["session_id"]
            .as_str()
            .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
        let content = params["content"].as_str().unwrap_or("");
        // Per-session run mutex (决策 #7): a concurrent run on the same session
        // is rejected with -32805 SessionBusy instead of interleaving streams.
        let acquired = self
            .registry
            .write()
            .map(|mut r| r.try_acquire_run(&sid.to_string()))
            .unwrap_or(false);
        if !acquired {
            return Err(DispatchError {
                code: error_codes::SESSION_BUSY,
                message: "SessionBusy".into(),
            });
        }
        let _run_guard = RunGuard {
            registry: Arc::clone(&self.registry),
            sid: sid.to_string(),
        };
        // Record the in-flight session so `$/cancelRequest` can reach the
        // permission module while the run executes; the guard clears it on
        // every exit path (including the AgentEvent::Error return).
        let _inflight = InflightSessionGuard::record(&self.session, sid).await;
        // The run initiator owns the session for approval routing (决策 #5)
        // and observes $/progress without an explicit subscribe. Re-recording
        // here also covers clients that skip session/new (hard-coded ids).
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid.to_string(), conn_id);
            reg.subscribe(&sid.to_string(), conn_id);
        }
        let progress_token = params.get("progress_token").cloned();
        let mut rx = self.agent_loop.run_turn_stream(&sid.to_string(), content);
        let mut text = String::new();
        let mut tc_list = vec![];
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(d) => {
                    text.push_str(&d);
                    let progress = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "$/progress",
                        "params": {
                            "token": progress_token.clone(),
                            "value": { "kind": "text", "delta": d },
                        },
                    })
                    .to_string();
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &progress);
                    }
                }
                AgentEvent::Done { tool_calls, .. } => {
                    tc_list = tool_calls;
                    break;
                }
                AgentEvent::Error(e) => return Err(DispatchError::internal(e)),
                _ => {}
            }
        }
        Ok(serde_json::json!({ "content": text, "tool_calls": tc_list }))
    }

    fn dispatch_shutdown(&self) -> Result<Value, DispatchError> {
        self.signal_shutdown()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        self.shutdown_requested.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "status": "ok" }))
    }

    /// `permission/respond`: deliver the client's decision to a pending approval.
    /// An unknown request_id is an error (-32600), never silent success.
    async fn dispatch_permission_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        // Owner gate (决策 #5): while a session has an in-flight run, only the
        // connection owning that session may resolve its approvals.
        let inflight = self.session.lock().await.clone();
        if let Some(sid) = inflight {
            let is_owner = self
                .registry
                .read()
                .ok()
                .and_then(|r| r.session_owner(&sid).cloned())
                .map(|owner| owner == conn_id)
                .unwrap_or(false);
            if !is_owner {
                return Err(DispatchError {
                    code: error_codes::NOT_AUTHORIZED,
                    message: "NotAuthorized".into(),
                });
            }
        }
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

    /// Transport-agnostic per-connection request loop (design 决策 #2): read
    /// NDJSON lines, dispatch each through `handle_request_from` with the
    /// connection's real id, write responses back on the same connection. The
    /// connection is registered for owner routing / subscription broadcasts
    /// and unregistered on every exit path (EOF, error, requested shutdown).
    pub async fn serve_connection(
        &self,
        conn: Box<dyn Connection>,
        _token: Option<&str>,
    ) -> Result<(), IpcError> {
        let conn_id = conn.id();
        if let Ok(mut reg) = self.registry.write() {
            reg.register(conn);
        }
        let serve_result = self.serve_connection_requests(&conn_id).await;
        if let Ok(mut reg) = self.registry.write() {
            reg.unregister(&conn_id);
        }
        serve_result
    }

    /// Per-connection request loop body. Like `serve_stdio`, the connection is
    /// detached from the registry while `recv` awaits (the std RwLock guard
    /// must not be held across an await) and re-inserted before each dispatch
    /// so handlers and the permission notifier can reach it via the registry.
    async fn serve_connection_requests(&self, conn_id: &str) -> Result<(), IpcError> {
        loop {
            let mut conn = self
                .registry
                .write()
                .map_err(|_| IpcError::InvalidRequest("connection registry poisoned".into()))?
                .conns
                .remove(conn_id)
                .ok_or_else(|| IpcError::InvalidRequest(format!("connection {conn_id} missing")))?;
            let Some(line) = conn.recv().await else {
                break; // EOF — connection dropped here; unregistered by the caller
            };
            // Re-insert before dispatch so handlers/notifier can reach it.
            if let Ok(mut reg) = self.registry.write() {
                reg.conns.insert(conn_id.to_string(), conn);
            }
            let trimmed = match line {
                Ok(l) => l.trim().to_string(),
                Err(e) => return Err(e),
            };
            if trimmed.is_empty() {
                continue;
            }
            let response = self
                .handle_request_from(&trimmed, conn_id)
                .await
                .unwrap_or_else(|e| {
                    format!(
                        r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"{e}"}}}}"#
                    )
                });
            if !response.is_empty() {
                let mut delivered = false;
                if let Ok(reg) = self.registry.read()
                    && let Some(conn) = reg.conns.get(conn_id)
                {
                    conn.send(&response)?;
                    delivered = true;
                }
                if !delivered {
                    return Err(IpcError::InvalidRequest(format!(
                        "connection {conn_id} missing"
                    )));
                }
            }
            if self.shutdown_requested() {
                break;
            }
        }
        Ok(())
    }

    /// Route $/permissionRequest notifications to the session owner through
    /// the registry (决策 #5). stdio falls back to stdout because its sentinel
    /// connection IS stdout. Attach once per transport entry point.
    pub(crate) fn attach_permission_notifier(&self) {
        if let Some(module) = &self.permission_module {
            let registry = Arc::clone(&self.registry);
            let notifier = Arc::new(move |p: &PermissionRequestParams, session_id: &str| {
                let key = session_id.to_string();
                let owner = registry
                    .read()
                    .ok()
                    .and_then(|r| r.session_owner(&key).cloned());
                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "$/permissionRequest",
                    "params": {
                        "request_id": p.request_id,
                        "tool": p.tool,
                        "args": p.args,
                        "reason": p.reason,
                        "origin_client": owner.clone(),
                    },
                });
                if let Some(owner_id) = owner
                    && let Ok(r) = registry.read()
                {
                    let _ = r.send_to(&owner_id, &msg.to_string());
                }
            });
            module.set_notifier(notifier);
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
        if let Ok(mut reg) = self.registry.write() {
            reg.register(Box::new(conn));
        }
        // Subscribe the permission notifier: $/permissionRequest is routed to
        // the session owner through the registry (决策 #5).
        self.attach_permission_notifier();
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
            // Detach the connection so recv's await never runs under the registry
            // lock: std guards are !Send and must not be held across await points.
            let mut conn = self
                .registry
                .write()
                .map_err(|_| IpcError::InvalidRequest("connection registry poisoned".into()))?
                .conns
                .remove(&conn_id)
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
            if let Ok(mut reg) = self.registry.write() {
                reg.conns.insert(conn_id.clone(), conn);
            }
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let response = self.handle_request(&trimmed).await.unwrap_or_else(|e| {
                format!(
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"{e}"}}}}"#
                )
            });
            if !response.is_empty() {
                let mut delivered = false;
                if let Ok(reg) = self.registry.read()
                    && let Some(conn) = reg.conns.get(&conn_id)
                {
                    conn.send(&response)?;
                    delivered = true;
                }
                if !delivered {
                    return Err(IpcError::InvalidRequest("stdio connection missing".into()));
                }
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
            // Sentinel id matches handle_request_from's stdio conn_id so
            // owner-directed routing (send_to) reaches the stdout writer.
            id: "stdio".into(),
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

/// Crate-internal fixtures shared by lib and tcp test modules.
#[cfg(test)]
pub(crate) mod test_support {
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop::AgentLoop;
    use agentj_core::module::*;
    use agentj_core::types::*;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

    pub(crate) struct MockConfig;
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

    pub(crate) fn stub_agent_loop() -> AgentLoop {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{MockConfig, stub_agent_loop};
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop::AgentLoop;
    use agentj_core::kernel::Kernel;
    use agentj_core::module::*;
    use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
    use agentj_core::types::*;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

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
        // when: session/run responds, then: result carries `content` per SessionRunResult
        assert_eq!(v["result"]["content"], "hello");
        assert!(v["result"].get("response").is_none());
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

    type SharedLog = Arc<std::sync::Mutex<Vec<String>>>;

    /// T5 fixture: `send` appends to a log the test keeps a handle to (the
    /// boxed conn inside the registry cannot be downcast, so logs live outside).
    struct FakeConn {
        id: String,
        log: SharedLog,
    }

    fn fake_conn(id: &str) -> (FakeConn, SharedLog) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            FakeConn {
                id: id.to_string(),
                log: std::sync::Arc::clone(&log),
            },
            log,
        )
    }

    #[async_trait]
    impl Connection for FakeConn {
        fn id(&self) -> String {
            self.id.clone()
        }
        fn kind(&self) -> TransportKind {
            TransportKind::Stdio
        }
        fn send(&self, msg: &str) -> Result<(), IpcError> {
            if let Ok(mut log) = self.log.lock() {
                log.push(msg.to_string());
            }
            Ok(())
        }
        async fn recv(&mut self) -> Option<Result<String, IpcError>> {
            None
        }
        fn peer_desc(&self) -> String {
            self.id.clone()
        }
    }

    /// Test connection driven by an mpsc channel: feed lines via `tx`, read
    /// sends from `log`, drop `tx` to EOF (recv then returns None).
    struct ScriptedConn {
        id: String,
        rx: tokio::sync::mpsc::Receiver<String>,
        log: SharedLog,
    }

    fn scripted_conn(id: &str) -> (ScriptedConn, SharedLog, tokio::sync::mpsc::Sender<String>) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        (
            ScriptedConn {
                id: id.to_string(),
                rx,
                log: std::sync::Arc::clone(&log),
            },
            log,
            tx,
        )
    }

    #[async_trait]
    impl Connection for ScriptedConn {
        fn id(&self) -> String {
            self.id.clone()
        }
        fn kind(&self) -> TransportKind {
            TransportKind::Tcp
        }
        fn send(&self, msg: &str) -> Result<(), IpcError> {
            if let Ok(mut log) = self.log.lock() {
                log.push(msg.to_string());
            }
            Ok(())
        }
        async fn recv(&mut self) -> Option<Result<String, IpcError>> {
            self.rx.recv().await.map(Ok)
        }
        fn peer_desc(&self) -> String {
            self.id.clone()
        }
    }

    #[test]
    fn registry_routes_broadcast_to_subscribers_only() {
        let mut reg = ConnectionRegistry::new();
        let (conn_a, log_a) = fake_conn("bcast-a");
        let (conn_b, log_b) = fake_conn("bcast-b");
        let a = reg.register(Box::new(conn_a));
        let _b = reg.register(Box::new(conn_b));
        reg.subscribe(&"s1".to_string(), &a);
        reg.broadcast_to_session(&"s1".to_string(), "hello");
        assert_eq!(
            log_a.lock().unwrap().len(),
            1,
            "subscriber a must receive the broadcast"
        );
        assert_eq!(
            log_b.lock().unwrap().len(),
            0,
            "non-subscriber b must NOT receive it"
        );
    }

    #[test]
    fn registry_unregister_releases_owner_and_subscriptions() {
        let mut reg = ConnectionRegistry::new();
        let (conn_a, log_a) = fake_conn("own-a");
        let a = reg.register(Box::new(conn_a));
        reg.set_session_owner(&"s1".to_string(), &a);
        reg.subscribe(&"s1".to_string(), &a);
        reg.unregister(&a);
        assert!(
            reg.session_owner(&"s1".to_string()).is_none(),
            "owner released on disconnect"
        );
        // broadcast after unregister is a no-op (no panic, no delivery)
        reg.broadcast_to_session(&"s1".to_string(), "hello");
        assert_eq!(log_a.lock().unwrap().len(), 0);
    }

    #[test]
    fn registry_run_mutex_is_exclusive() {
        let mut reg = ConnectionRegistry::new();
        assert!(reg.try_acquire_run(&"s1".to_string()));
        assert!(
            !reg.try_acquire_run(&"s1".to_string()),
            "second run must be rejected (busy)"
        );
        reg.release_run(&"s1".to_string());
        assert!(reg.try_acquire_run(&"s1".to_string()));
    }

    /// Connection-id → sent-message log shared between register_fake_conn and
    /// drain_sent (the registry owns the boxed conn; logs live outside it).
    static FAKE_LOGS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, SharedLog>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    fn register_fake_conn(server: &Arc<IpcServer>) -> String {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let id = format!(
            "fake-{}",
            SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        let (conn, log) = fake_conn(&id);
        if let Ok(mut reg) = server.registry.write() {
            reg.register(Box::new(conn));
        }
        if let Ok(mut logs) = FAKE_LOGS.lock() {
            logs.insert(id.clone(), log);
        }
        id
    }

    fn drain_sent(_server: &Arc<IpcServer>, id: &str) -> Vec<String> {
        FAKE_LOGS
            .lock()
            .ok()
            .and_then(|logs| logs.get(id).cloned())
            .map(|log| log.lock().unwrap_or_else(|p| p.into_inner()).clone())
            .unwrap_or_default()
    }

    fn extract_sid(resp: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(resp).unwrap();
        v["result"]["session_id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn initialize_returns_connection_id_and_multi_client() {
        let server = Arc::new(IpcServer::new(stub_agent_loop(), Kernel::new()));
        let conn_id = register_fake_conn(&server);
        let resp = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#,
                &conn_id,
            )
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["connection_id"], conn_id);
        assert_eq!(v["result"]["capabilities"]["multi_client"], true);
    }

    #[tokio::test]
    async fn initialize_with_wrong_token_is_rejected() {
        let server = Arc::new(IpcServer::new(stub_agent_loop(), Kernel::new()));
        server.set_token(Some("secret".into()));
        let conn_id = register_fake_conn(&server);
        let resp = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0","token":"wrong"}}"#,
                &conn_id,
            )
            .await
            .unwrap();
        assert!(
            resp.contains(r#""code":-32806"#),
            "wrong token must be NotAuthorized: {resp}"
        );
    }

    #[tokio::test]
    async fn non_owner_permission_respond_is_rejected() {
        let (server, _module) = blocking_run_server();
        let server = Arc::new(server);
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        let new_resp = server
            .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_task = tokio::spawn({
            let s = Arc::clone(&server);
            let conn = a.clone();
            async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
        });
        wait_for_inflight_session(&server, &sid)
            .await
            .expect("run must record in-flight session");
        let resp = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":9,"method":"permission/respond","params":{"request_id":"req-x","decision":"allow"}}"#,
                &b,
            )
            .await
            .unwrap();
        assert!(
            resp.contains(r#""code":-32806"#),
            "non-owner respond must be NotAuthorized: {resp}"
        );
        let _ = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":10,"method":"$/cancelRequest","params":{"id":5}}"#,
                &a,
            )
            .await
            .unwrap();
        let _ = run_task.await;
    }

    #[tokio::test]
    async fn concurrent_run_on_same_session_is_rejected() {
        // stub mock finishes instantly — the blocking fixture keeps run1 in
        // flight deterministically while run2 arrives.
        let (server, _module) = blocking_run_server();
        let server = Arc::new(server);
        let a = register_fake_conn(&server);
        let new_resp = server
            .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_task = tokio::spawn({
            let s = Arc::clone(&server);
            let conn = a.clone();
            async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
        });
        wait_for_inflight_session(&server, &sid)
            .await
            .expect("run1 must be in flight");
        let run2 = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"again"}}}}"#
        );
        let r2 = server.handle_request_from(&run2, &a).await.unwrap();
        assert!(
            r2.contains(r#""code":-32805"#),
            "second run must be SessionBusy: {r2}"
        );
        let _ = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":10,"method":"$/cancelRequest","params":{"id":5}}"#,
                &a,
            )
            .await
            .unwrap();
        let _ = run_task.await;
    }

    #[tokio::test]
    async fn subscriber_receives_progress_broadcast() {
        let server = Arc::new(IpcServer::new(stub_agent_loop(), Kernel::new()));
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        let new_resp = server
            .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        server
            .handle_request_from(
                &format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
                ),
                &b,
            )
            .await
            .unwrap();
        server
            .handle_request_from(
                &format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
                ),
                &a,
            )
            .await
            .unwrap();
        let b_sent = drain_sent(&server, &b);
        assert!(
            b_sent
                .iter()
                .any(|m| m.contains("$/progress") && m.contains("hello")),
            "subscriber B must receive $/progress deltas: {b_sent:?}"
        );
    }

    #[tokio::test]
    async fn permission_request_is_routed_to_owner_only() {
        let (server, _module) = blocking_run_server();
        let server = Arc::new(server);
        server.attach_permission_notifier();
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        let new_resp = server
            .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_task = tokio::spawn({
            let s = Arc::clone(&server);
            let conn = a.clone();
            async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
        });
        wait_for_inflight_session(&server, &sid)
            .await
            .expect("run must be in flight");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let a_sent = drain_sent(&server, &a);
        let b_sent = drain_sent(&server, &b);
        assert!(
            a_sent
                .iter()
                .any(|m| m.contains("$/permissionRequest") && m.contains("\"origin_client\":\"")),
            "owner A must receive $/permissionRequest with origin_client: {a_sent:?}"
        );
        assert!(
            !b_sent.iter().any(|m| m.contains("$/permissionRequest")),
            "non-owner B must NOT receive $/permissionRequest: {b_sent:?}"
        );
        let _ = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":9,"method":"$/cancelRequest","params":{"id":5}}"#,
                &a,
            )
            .await
            .unwrap();
        let _ = run_task.await;
    }

    #[tokio::test]
    async fn serve_connection_two_clients_get_unique_connection_ids() {
        let server = Arc::new(IpcServer::new(stub_agent_loop(), Kernel::new()));
        let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
        let (conn_b, log_b, tx_b) = scripted_conn("tcp-B");
        let s = Arc::clone(&server);
        let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a), None).await });
        let s = Arc::clone(&server);
        let task_b = tokio::spawn(async move { s.serve_connection(Box::new(conn_b), None).await });
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#;
        tx_a.send(init.to_string()).await.unwrap();
        tx_b.send(init.to_string()).await.unwrap();
        drop(tx_a);
        drop(tx_b);
        task_a.await.unwrap().unwrap();
        task_b.await.unwrap().unwrap();
        let va: serde_json::Value = serde_json::from_str(&log_a.lock().unwrap()[0]).unwrap();
        let vb: serde_json::Value = serde_json::from_str(&log_b.lock().unwrap()[0]).unwrap();
        assert_eq!(va["result"]["connection_id"], "tcp-A");
        assert_eq!(vb["result"]["connection_id"], "tcp-B");
        assert_ne!(
            va["result"]["connection_id"], vb["result"]["connection_id"],
            "each TCP client must receive its own connection id"
        );
    }

    #[tokio::test]
    async fn serve_connection_broadcasts_progress_to_real_connection() {
        let server = Arc::new(IpcServer::new(stub_agent_loop(), Kernel::new()));
        let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
        let s = Arc::clone(&server);
        let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a), None).await });
        tx_a.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
            .await
            .unwrap();
        // Wait for the session/new response so the run dispatches against a real sid.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !log_a.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("session/new response never arrived");
        let sid = extract_sid(&log_a.lock().unwrap()[0]);
        tx_a
            .send(format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ))
            .await
            .unwrap();
        drop(tx_a); // EOF after the run so the serve loop returns
        task_a.await.unwrap().unwrap();
        let a_sent = log_a.lock().unwrap().clone();
        assert!(
            a_sent
                .iter()
                .any(|m| m.contains("$/progress") && m.contains("hello")),
            "run initiator must receive $/progress through the registry: {a_sent:?}"
        );
        assert!(
            server.registry.read().unwrap().conns.is_empty(),
            "disconnected connection must be unregistered"
        );
    }
}
