//! Unified Connection abstraction (design 决策 #1/#2) — transport-agnostic serve.
pub mod file_list;
pub(crate) mod login_limiter;
pub mod tcp;
pub mod trajectory;
pub mod web;
pub mod ws;

use agentj_core::artifacts::{
    FileChangeArtifact, FileChangeOp, TodoItemArtifact, TodoStatusArtifact,
};
use agentj_core::error::{IpcError, KernelError, ModuleError};
use agentj_core::kernel::Kernel;
use agentj_core::module::{KernelMessage, SessionStore};
use agentj_core::types::{AgentEvent, SessionId};
use agentj_permission::{ApprovalDecision, PermissionModule};
use agentj_protocol::base::error_codes;
use agentj_protocol::file::{FileKind, FileListParams, FileListResult};
use agentj_protocol::lifecycle::InitializeParams;
use agentj_protocol::permission::{PermissionRequestParams, PermissionRespondParams};
use agentj_protocol::question::{AskQuestionParams, QuestionRespondParams, QuestionRespondResult};
use agentj_protocol::session::{
    SessionIdParams, SessionNewParams, SessionStatusResult, SessionSubscribeParams,
};
use agentj_protocol::streaming::CancelParams;
use agentj_question::{
    QuestionModule, QuestionNotifier, QuestionOutcome, QuestionSettlement, RespondError,
    RespondOutcome, SettleHook,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Transport kind — new transports (Tcp / UnixSocket) add a variant + impl, serve loop unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Stdio,
    Tcp,
    UnixSocket,
    WebSocket,
}

/// Send closure shared between a connection and its registered writer handle.
pub type ConnSend = Arc<dyn Fn(&str) -> Result<(), IpcError> + Send + Sync>;

/// Send-only handle to a connection, registered in the `ConnectionRegistry`
/// so broadcasts and owner-directed routing can reach a connection while its
/// serve loop owns the reader half (and is therefore awaiting `recv`). The
/// handle shares the connection's writer — messages sent through it reach the
/// same peer. Kept registered for the connection's lifetime, so an idle
/// subscriber never drops out of broadcast reach (BUG-006).
pub struct ConnectionWriter {
    id: String,
    kind: TransportKind,
    peer: String,
    send: ConnSend,
}

impl ConnectionWriter {
    pub fn new(id: String, kind: TransportKind, peer: String, send: ConnSend) -> Self {
        Self {
            id,
            kind,
            peer,
            send,
        }
    }
    pub fn id(&self) -> String {
        self.id.clone()
    }
    pub fn kind(&self) -> TransportKind {
        self.kind
    }
    pub fn peer_desc(&self) -> String {
        self.peer.clone()
    }
    pub fn send(&self, msg: &str) -> Result<(), IpcError> {
        (self.send)(msg)
    }
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
    /// Detach a send-only handle sharing this connection's writer, so the
    /// registry can route broadcasts to it while the serve loop owns the
    /// reader half (`recv` needs `&mut`, so the full connection cannot stay
    /// registered across the await).
    fn writer(&self) -> ConnectionWriter;
}

/// Multi-client connection registry (design 决策 #1/#5) — v0.1 uses the single
/// stdio path; session_owner routing is consumed by v0.3 permission direction.
pub struct ConnectionRegistry {
    conns: std::collections::HashMap<String, Box<dyn Connection>>,
    /// Send-only handles for connections whose serve loop owns the reader half
    /// (TCP). Registered for the connection's lifetime so broadcasts reach idle
    /// subscribers; removed on disconnect.
    writers: std::collections::HashMap<String, ConnectionWriter>,
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
            writers: std::collections::HashMap::new(),
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
    /// Register a send-only handle so the connection stays reachable for
    /// broadcasts while its serve loop owns the reader half.
    pub fn register_writer(&mut self, writer: ConnectionWriter) -> String {
        let id = writer.id();
        self.writers.insert(id.clone(), writer);
        id
    }
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn Connection>> {
        self.authorized.remove(id);
        self.session_owner.retain(|_, owner| owner != id);
        for subs in self.subscribers.values_mut() {
            subs.remove(id);
        }
        self.subscribers.retain(|_, s| !s.is_empty());
        self.writers.remove(id);
        self.conns.remove(id)
    }
    /// Record which connection owns a session (design 决策 #5: 审批定向路由依据).
    pub fn set_session_owner(&mut self, session: &SessionId, conn: &str) {
        self.session_owner.insert(session.clone(), conn.to_string());
    }
    pub fn session_owner(&self, session: &SessionId) -> Option<&String> {
        self.session_owner.get(session)
    }
    /// Sessions owned by a connection (owner-disconnect question settlement).
    pub fn sessions_owned_by(&self, conn_id: &str) -> Vec<SessionId> {
        self.session_owner
            .iter()
            .filter(|(_, owner)| owner.as_str() == conn_id)
            .map(|(sid, _)| sid.clone())
            .collect()
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
    /// Full connections (stdio) take precedence; TCP connections are reached
    /// through their registered send-only handle.
    pub fn send_to(&self, conn_id: &str, msg: &str) -> Result<(), IpcError> {
        if let Some(conn) = self.conns.get(conn_id) {
            return conn.send(msg);
        }
        match self.writers.get(conn_id) {
            Some(writer) => writer.send(msg),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Parse `raw` into a [`JsonRpcRequest`] or, on failure, build the JSON-RPC
/// 2.0 error response (JSON-RPC 2.0 §5). A syntax error is `-32700` with a
/// null id; valid JSON that fails request schema validation is `-32600` with
/// the raw request id echoed when it is a string, number or null.
fn parse_request(raw: &str) -> Result<JsonRpcRequest, Box<JsonRpcResponse>> {
    match serde_json::from_str::<JsonRpcRequest>(raw) {
        Ok(req) => Ok(req),
        Err(e) => {
            let (code, id, message) = match serde_json::from_str::<Value>(raw) {
                Ok(value) => (
                    error_codes::INVALID_REQUEST,
                    extract_request_id(&value),
                    format!("Invalid Request: {e}"),
                ),
                Err(_) => (error_codes::PARSE_ERROR, None, format!("Parse error: {e}")),
            };
            Err(Box::new(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: id.unwrap_or(Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code,
                    message,
                    data: None,
                }),
            }))
        }
    }
}

/// Extract a JSON-RPC 2.0 id from a raw JSON value. Only string, number and
/// null ids are valid; absent or invalid types (object/array/bool) yield
/// `None` so the error response falls back to a null id.
fn extract_request_id(value: &Value) -> Option<Value> {
    match value.get("id") {
        Some(id @ (Value::String(_) | Value::Number(_) | Value::Null)) => Some(id.clone()),
        _ => None,
    }
}

/// Method-dispatch failure carrying the JSON-RPC 2.0 standard error code
/// (design 决策 #4). `handle_request` echoes the request `id` alongside this
/// code so every error path round-trips the correlation id. `data` carries
/// parameterized error details (`error.data`, design 决策 #5) and stays off
/// the wire when None.
#[derive(Debug, Serialize)]
struct DispatchError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl DispatchError {
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }
    fn invalid_params_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: Some(data),
        }
    }
    fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
            data: None,
        }
    }
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
            data: None,
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: None,
        }
    }
    fn internal_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: Some(data),
        }
    }
}

pub struct IpcServer {
    kernel: Kernel,
    session_store: Arc<dyn SessionStore>,
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
    /// Optional QuestionModule: `question/respond` dispatch, owner-directed
    /// `$/askQuestion` notify and settle-on-cancel/disconnect.
    pub question_module: Option<Arc<QuestionModule>>,
    /// Optional TrajectoryStore: run/respond recording + session/history reads.
    pub trajectory_store: Option<Arc<trajectory::TrajectoryStore>>,
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
    pub fn new(mut kernel: Kernel, session_store: Arc<dyn SessionStore>) -> Self {
        let kernel_handle = kernel.start();
        Self {
            kernel,
            session_store,
            kernel_handle: Some(kernel_handle),
            shutdown_requested: AtomicBool::new(false),
            registry: Arc::new(std::sync::RwLock::new(ConnectionRegistry::new())),
            token: std::sync::Mutex::new(None),
            permission_module: None,
            question_module: None,
            trajectory_store: None,
        }
    }

    pub fn create_session(&self) -> Result<SessionId, ModuleError> {
        self.session_store.create()
    }

    pub fn run_turn_stream(&self, sid: &str, input: &str) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(64);
        let sid = sid.to_string();
        let content = input.to_string();
        if self
            .kernel
            .sender()
            .send(KernelMessage::UserRequest {
                session_id: sid,
                content,
                attachments: vec![],
                reply: tx,
            })
            .is_err()
        {
            let (etx, erx) = mpsc::channel(1);
            let _ = etx.try_send(AgentEvent::Error("bus send failed".into()));
            return erx;
        }
        rx
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

    /// Handle a single JSON-RPC request from connection `conn_id`. Valid
    /// requests echo the request `id` (JSON-RPC 2.0 §5) with the semantically
    /// correct standard code, never falling through to the serve loop.
    /// Malformed requests still get an error line: a syntax error yields
    /// `-32700` with a null id (the id is undetectable), while valid JSON that
    /// fails request schema validation yields `-32600` and echoes the raw `id`
    /// when it is a string, number or null.
    pub async fn handle_request_from(&self, raw: &str, conn_id: &str) -> Result<String, IpcError> {
        let req = match parse_request(raw) {
            Ok(r) => r,
            Err(err_resp) => return Ok(serde_json::to_string(&*err_resp)?),
        };

        let result = if let Some(err) = self.auth_gate(&req, conn_id) {
            Err(err)
        } else {
            match req.method.as_str() {
                "initialize" => self.dispatch_initialize(&req, conn_id),
                "session/new" => self.dispatch_session_new(&req, conn_id),
                "session/run" => self.dispatch_session_run(&req, conn_id).await,
                "session/subscribe" => self.dispatch_subscribe(&req, conn_id),
                "session/unsubscribe" => self.dispatch_unsubscribe(&req, conn_id),
                "kernel/status" => self.dispatch_kernel_status(),
                "session/list" => self.dispatch_session_list(),
                "session/history" => self.dispatch_session_history(&req),
                "session/status" => self.dispatch_session_status(&req),
                "session/archive" => self.dispatch_session_archive(&req),
                "session/delete" => self.dispatch_session_delete(&req),
                "file/list" => self.dispatch_file_list(&req).await,
                "shutdown" => self.dispatch_shutdown(),
                "permission/respond" => self.dispatch_permission_respond(&req, conn_id).await,
                "question/respond" => self.dispatch_question_respond(&req, conn_id),
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
                    data: e.data,
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
                data: None,
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
                data: None,
            });
        }
        // Successful auth: the connection may now issue business methods on a
        // token-gated server (the non-initialize gate checks this registry).
        if let Ok(mut reg) = self.registry.write() {
            reg.mark_authorized(conn_id);
        }
        let product_version = env!("CARGO_PKG_VERSION");
        Ok(serde_json::json!({
            "protocol_version": agentj_protocol::PROTOCOL_VERSION,
            "capabilities": { "multi_client": true },
            "server_info": { "name": "agentj", "version": product_version },
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

    fn dispatch_session_new(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let requested = Self::resolve_project_root(req.params.as_ref())?;
        let effective = match &requested {
            Some(root) => root.clone(),
            None => std::env::current_dir()
                .and_then(|dir| dir.canonicalize())
                .map_err(|e| DispatchError::internal(e.to_string()))?,
        };
        let sid = self
            .session_store
            .create()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        if let Some(root) = requested
            && let Err(e) = self.session_store.set_project_root(&sid, root)
        {
            let _ = self.session_store.delete(&sid);
            return Err(DispatchError::internal_with_data(
                e.to_string(),
                serde_json::json!({ "session_id": sid, "reason": "project_root_persist_failed" }),
            ));
        }
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid, conn_id);
        }
        Ok(serde_json::json!({
            "session_id": sid,
            "project_root": effective.to_string_lossy(),
        }))
    }

    /// Validate `session/new {project_root}` (design item 6): absent params keep
    /// the legacy session (no root); when present the root must be an absolute
    /// existing directory, returned canonicalized.
    fn resolve_project_root(
        params: Option<&Value>,
    ) -> Result<Option<std::path::PathBuf>, DispatchError> {
        let Some(raw) = params else {
            return Ok(None);
        };
        let parsed: SessionNewParams = serde_json::from_value(raw.clone()).map_err(|_| {
            let bad = raw.get("project_root").unwrap_or(&Value::Null);
            Self::invalid_project_root(bad)
        })?;
        let Some(root) = parsed.project_root else {
            return Ok(None);
        };
        let path = std::path::PathBuf::from(&root);
        if !path.is_absolute() {
            return Err(Self::invalid_project_root(&Value::String(root)));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| Self::invalid_project_root(&Value::String(root.clone())))?;
        if !canonical.is_dir() {
            return Err(Self::invalid_project_root(&Value::String(root)));
        }
        Ok(Some(canonical))
    }

    /// `-32602` + `error.data {path, reason}` for an invalid `session/new`
    /// `project_root` (design item 7).
    fn invalid_project_root(path: &Value) -> DispatchError {
        DispatchError::invalid_params_with_data(
            "invalid project_root: must be an absolute existing directory",
            serde_json::json!({ "path": path, "reason": "invalid_project_root" }),
        )
    }

    fn record_trajectory(&self, sid: &str, value: agentj_protocol::trajectory::TrajectoryValue) {
        let Some(store) = self.trajectory_store.as_ref() else {
            return;
        };
        if let Err(e) = store.record(&sid.to_string(), value) {
            tracing::warn!("trajectory record failed for {sid}: {e}");
        }
    }

    fn flush_thinking(&self, pending: &mut String, sid: &str) {
        if !pending.is_empty() {
            self.record_trajectory(
                sid,
                agentj_protocol::trajectory::TrajectoryValue::Thinking {
                    content: std::mem::take(pending),
                },
            );
        }
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
                data: None,
            });
        }
        let _run_guard = RunGuard {
            registry: Arc::clone(&self.registry),
            sid: sid.to_string(),
        };
        // The run initiator owns the session for approval routing (决策 #5)
        // and observes $/progress without an explicit subscribe. Re-recording
        // here also covers clients that skip session/new (hard-coded ids).
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid.to_string(), conn_id);
            reg.subscribe(&sid.to_string(), conn_id);
        }
        // §7.2: a `$/progress` `token` must be ProgressToken (integer|string).
        // Echo `progress_token` when provided; otherwise fall back to the
        // request id; a notification (no id) falls back to a valid number.
        let progress_token = match params.get("progress_token") {
            Some(t) if !t.is_null() => t.clone(),
            _ => match &req.id {
                Value::Null => Value::Number(0.into()),
                id => id.clone(),
            },
        };
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::User {
                content: content.to_string(),
            },
        );
        let mut rx = self.run_turn_stream(sid, content);
        let mut text = String::new();
        let mut tc_list = vec![];
        let mut usage_opt: Option<serde_json::Value> = None;
        let mut current_run_id: Option<String> = None;
        let mut pending_thinking = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(d) => {
                    text.push_str(&d);
                    pending_thinking.push_str(&d);
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
                AgentEvent::ToolCall(tc) => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    let wire: agentj_protocol::types::ToolCall = agentj_protocol::types::ToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    };
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::ToolCall {
                            tool_call: wire.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call: wire },
                    );
                }
                AgentEvent::ToolResult { call_id, result } => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::ToolResult {
                            tool_call_id: call_id.clone(),
                            content: result.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::ToolResult {
                            tool_call_id: call_id,
                            content: result,
                        },
                    );
                }
                AgentEvent::Done {
                    run_id,
                    tool_calls,
                    usage,
                    ..
                } => {
                    let done_run_id = sanitize_run_id(run_id);
                    if done_run_id.is_some() {
                        current_run_id = done_run_id.clone();
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    if let Some(u) = &usage {
                        let info = agentj_protocol::session::UsageInfo {
                            prompt_tokens: u.prompt_tokens,
                            completion_tokens: u.completion_tokens,
                        };
                        let frame = build_progress_frame(
                            &progress_token,
                            &agentj_protocol::streaming::ProgressValue::Usage { usage: info },
                        );
                        if let Ok(reg) = self.registry.read() {
                            reg.broadcast_to_session(&sid.to_string(), &frame);
                        }
                        self.record_trajectory(
                            sid,
                            agentj_protocol::trajectory::TrajectoryValue::Usage { usage: info },
                        );
                    }
                    tc_list = tool_calls;
                    usage_opt = usage.map(|u| {
                        serde_json::json!({
                            "prompt_tokens": u.prompt_tokens,
                            "completion_tokens": u.completion_tokens
                        })
                    });
                    break;
                }
                AgentEvent::Error(e) => {
                    if e == "cancelled" {
                        return Err(DispatchError {
                            code: -32800,
                            message: "RequestCancelled".into(),
                            data: None,
                        });
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::Error { message: e.clone() },
                    );
                    return Err(DispatchError::internal(e));
                }
                AgentEvent::Todo {
                    run_id,
                    revision,
                    snapshot,
                } => {
                    let sanitized = sanitize_run_id(run_id);
                    if sanitized.is_some() {
                        current_run_id = sanitized.clone();
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    let items = todo_items_into_wire(snapshot.items);
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::Todo {
                            session_id: sid.to_string(),
                            run_id: sanitized.clone(),
                            revision,
                            items: items.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::Todo {
                            revision,
                            items,
                            run_id: sanitized,
                        },
                    );
                }
                AgentEvent::FileChange(artifact) => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    // artifact already carries run_id/seq from TurnRunner; sanitize illegal ids to None
                    let mut artifact = artifact;
                    artifact.run_id = sanitize_run_id(artifact.run_id);
                    if artifact.run_id.is_some() {
                        current_run_id = artifact.run_id.clone();
                    }
                    let change = file_change_into_wire(artifact.clone());
                    let top_run_id = change.run_id.clone();
                    let file_change_run_id = change.run_id.clone();
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::FileChange {
                            session_id: sid.to_string(),
                            run_id: top_run_id,
                            change: change.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::FileChange {
                            change,
                            run_id: file_change_run_id,
                        },
                    );
                }
            }
        }
        let mut result = serde_json::json!({ "content": text, "tool_calls": tc_list });
        if let Some(u) = usage_opt {
            result["usage"] = u;
        }
        if let Some(rid) = current_run_id
            && let Some(sanitized) = sanitize_run_id(Some(rid))
        {
            result["run_id"] = Value::String(sanitized);
        }
        Ok(result)
    }

    fn dispatch_shutdown(&self) -> Result<Value, DispatchError> {
        self.signal_shutdown()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        self.shutdown_requested.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "status": "ok" }))
    }

    fn dispatch_kernel_status(&self) -> Result<Value, DispatchError> {
        let modules: Vec<Value> = self
            .kernel
            .modules()
            .iter()
            .map(|(id, state)| {
                serde_json::json!({
                    "id": id,
                    "state": format!("{state:?}").to_lowercase()
                })
            })
            .collect();
        let (default_model, providers) = kernel_model_info(self.kernel.ctx().as_ref());
        let mut body = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "modules": modules,
        });
        if let Some(model) = default_model {
            body["default_model"] = serde_json::json!(model);
        }
        if let Some(ps) = providers {
            body["providers"] = serde_json::to_value(ps).expect("serializable");
        }
        Ok(body)
    }

    fn dispatch_session_list(&self) -> Result<Value, DispatchError> {
        let entries = self
            .session_store
            .list()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        let sessions: Vec<Value> = entries
            .iter()
            .map(|e| {
                let state = match e.status {
                    agentj_core::module::SessionStatus::Active => "active",
                    agentj_core::module::SessionStatus::Archived => "archived",
                    agentj_core::module::SessionStatus::Deleted => "deleted",
                };
                let mut obj = serde_json::json!({
                    "session_id": e.session_id,
                    "updated_at": e.updated_at,
                    "state": state,
                });
                if let Some(p) = &e.preview {
                    obj["preview"] = serde_json::json!(p);
                }
                obj
            })
            .collect();
        Ok(serde_json::json!({ "sessions": sessions }))
    }

    /// Wire mapping shared by `session/status` / `archive` / `delete`.
    fn wire_session_status(
        status: agentj_core::module::SessionStatus,
    ) -> agentj_protocol::types::SessionStatus {
        use agentj_protocol::types::SessionStatus as WireStatus;

        match status {
            agentj_core::module::SessionStatus::Active => WireStatus::Active,
            agentj_core::module::SessionStatus::Archived => WireStatus::Archived,
            agentj_core::module::SessionStatus::Deleted => WireStatus::Deleted,
        }
    }

    /// Shared existence probe: unknown sessions map to `-32802 SessionNotFound`.
    fn require_session_status(
        &self,
        session_id: &SessionId,
    ) -> Result<agentj_core::module::SessionStatus, DispatchError> {
        self.session_store
            .status(session_id)
            .map_err(|_| DispatchError {
                code: error_codes::SESSION_NOT_FOUND,
                message: "SessionNotFound".into(),
                data: Some(serde_json::json!({ "session_id": session_id })),
            })
    }

    /// `-32803 SessionInactive` with the offending `{session_id, status}` data.
    fn session_inactive(
        session_id: &SessionId,
        status: agentj_core::module::SessionStatus,
    ) -> DispatchError {
        DispatchError {
            code: error_codes::SESSION_INACTIVE,
            message: "SessionInactive".into(),
            data: Some(serde_json::json!({
                "session_id": session_id,
                "status": Self::wire_session_status(status),
            })),
        }
    }

    /// `session/status` (spec §8.8): the existence probe clients fire before
    /// `session/subscribe`.
    fn dispatch_session_status(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        let project_root = self
            .session_store
            .project_root(&params.session_id)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|d| d.canonicalize().ok())
            })
            .map(|p| p.to_string_lossy().into_owned());
        let result = SessionStatusResult {
            status: Self::wire_session_status(status),
            project_root,
        };
        Ok(serde_json::to_value(result).expect("serializable"))
    }

    /// `session/archive` (spec §8.9): only an active session may be archived.
    fn dispatch_session_archive(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        if status != agentj_core::module::SessionStatus::Active {
            return Err(Self::session_inactive(&params.session_id, status));
        }
        self.session_store
            .archive(&params.session_id)
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        Ok(Value::Null)
    }

    /// `session/delete` (spec §8.10): archived sessions may be deleted;
    /// deleting a deleted session is `-32803 SessionInactive`.
    fn dispatch_session_delete(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        if status == agentj_core::module::SessionStatus::Deleted {
            return Err(Self::session_inactive(&params.session_id, status));
        }
        self.session_store
            .delete(&params.session_id)
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        Ok(Value::Null)
    }

    fn dispatch_session_history(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let sid = params["session_id"]
            .as_str()
            .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
        let raw_run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(rid) = &raw_run_id
            && !is_valid_run_id(rid)
        {
            return Err(DispatchError::invalid_params_with_data(
                "InvalidParams",
                serde_json::json!({"reason": "invalid_run_id"}),
            ));
        }
        let limit = params["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
        let cursor = params["cursor"].as_u64();
        let (events, next) = match &self.trajectory_store {
            Some(store) => {
                history_events_filtered(store, sid, raw_run_id.as_deref(), cursor, limit)
                    .map_err(|e| DispatchError::internal(e.to_string()))?
            }
            None => (vec![], None),
        };
        let result = agentj_protocol::session::SessionHistoryResult {
            events,
            next_cursor: next,
        };
        Ok(serde_json::to_value(result).expect("serializable"))
    }
    /// the session `project_root`, with git porcelain statuses per file entry.
    async fn dispatch_file_list(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params: FileListParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(format!("invalid file/list params: {e}")))?;
        if self.session_store.status(&params.session_id).is_err() {
            return Err(DispatchError {
                code: error_codes::SESSION_NOT_FOUND,
                message: "SessionNotFound".into(),
                data: None,
            });
        }
        let root = match self.session_store.project_root(&params.session_id) {
            Some(root) => root,
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty());
        let result = match query {
            Some(q) => file_list::search(&root, params.path.as_deref(), q),
            None => file_list::list(&root, params.path.as_deref(), params.depth),
        };
        match result {
            Ok(result) => {
                let result = attach_git_statuses(result, &root).await;
                Ok(serde_json::to_value(result).expect("serializable"))
            }
            Err(e) => Err(map_file_list_error(e, &params.path)),
        }
    }

    /// `permission/respond`: deliver the client's decision to a pending approval.
    /// An unknown request_id is an error (-32600), never silent success.
    async fn dispatch_permission_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
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
        let request_id = params.request_id.clone();
        let wire_decision = params.decision.clone();
        let rid = agentj_core::permission::PermissionRequestId(request_id.clone());
        let sid = module.session_of(&rid).ok_or_else(|| {
            DispatchError::invalid_request(format!("unknown permission request: {}", rid.0))
        })?;
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
                data: None,
            });
        }
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
        let view = self
            .permission_module
            .as_ref()
            .and_then(|m| m.view(&request_id));
        if !module.respond(&rid, decision) {
            return Err(DispatchError::invalid_request(format!(
                "unknown permission request: {}",
                rid.0
            )));
        }
        if let Some(view) = view {
            let sid_clone = view.session_id.clone();
            let frame = build_progress_frame(
                &Value::Null,
                &agentj_protocol::streaming::ProgressValue::Permission {
                    request_id: request_id.clone(),
                    tool: view.tool.clone(),
                    decision: wire_decision.clone(),
                    session_id: Some(sid_clone.clone()),
                },
            );
            if let Ok(reg) = self.registry.read() {
                reg.broadcast_to_session(&sid_clone, &frame);
            }
            self.record_trajectory(
                &sid_clone,
                agentj_protocol::trajectory::TrajectoryValue::Permission {
                    request_id,
                    tool: view.tool,
                    decision: wire_decision,
                },
            );
        }
        Ok(serde_json::json!({}))
    }

    /// Wire result for `question/respond` (design item 5): `false` is the
    /// idempotent answer for duplicate / already-settled / unknown ids.
    fn question_result(accepted: bool) -> Value {
        serde_json::to_value(QuestionRespondResult { accepted }).expect("serializable")
    }

    /// `question/respond`: owner-gated, idempotent answer delivery. Check order
    /// (design item 5): question_id shape → pending session lookup (unknown →
    /// `{accepted:false}` without owner check) → owner gate → parameter
    /// validation/settlement inside the module.
    fn dispatch_question_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let module = self
            .question_module
            .clone()
            .ok_or_else(|| DispatchError::invalid_request("no question module"))?;
        let raw = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let question_id = raw
            .get("question_id")
            .and_then(Value::as_str)
            .ok_or_else(|| DispatchError::invalid_params("missing question_id"))?
            .to_string();
        let Some(session_id) = module.session_of(&question_id) else {
            return Ok(Self::question_result(false));
        };
        let is_owner = self
            .registry
            .read()
            .ok()
            .and_then(|r| r.session_owner(&session_id).cloned())
            .map(|owner| owner == conn_id)
            .unwrap_or(false);
        if !is_owner {
            return Err(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
                data: None,
            });
        }
        let params: QuestionRespondParams = serde_json::from_value(raw.clone())
            .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        match module.respond(&question_id, params.answers, params.skipped) {
            Ok(RespondOutcome::Accepted(accepted)) => Ok(Self::question_result(accepted)),
            Err(RespondError::UnknownOptions { unknown }) => {
                Err(DispatchError::invalid_params_with_data(
                    "unknown option ids",
                    serde_json::json!({ "unknown_options": unknown }),
                ))
            }
            Err(RespondError::TooManyAnswers) => Err(DispatchError::invalid_params_with_data(
                "multiple answers for a single-choice question",
                serde_json::json!({ "reason": "too_many_answers" }),
            )),
        }
    }

    async fn dispatch_cancel_request(&self, req: &JsonRpcRequest) -> Result<Value, DispatchError> {
        let params: CancelParams =
            serde_json::from_value(req.params.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| DispatchError::invalid_request(e.to_string()))?;
        let session_id = match params.session_id {
            Some(sid) => Some(sid),
            None => {
                let inflight: Vec<String> = self
                    .registry
                    .read()
                    .ok()
                    .map(|r| r.pending_runs.iter().cloned().collect())
                    .unwrap_or_default();
                match inflight.len() {
                    1 => Some(inflight[0].clone()),
                    n => {
                        if n > 1 {
                            tracing::warn!(
                                "$/cancelRequest without session_id is ambiguous ({} in-flight); ignoring",
                                n
                            );
                        }
                        None
                    }
                }
            }
        };
        if let Some(ref sid) = session_id {
            if let Some(module) = self.permission_module.as_ref() {
                let _ = module.cancel_session(sid, "cancelled");
            }
            if let Some(module) = self.question_module.as_ref() {
                let _ = module.cancel_session(sid);
            }
            let (tx, _rx) = tokio::sync::oneshot::channel();
            let _ = self.kernel.sender().send(KernelMessage::ServiceCall {
                service: "agent".into(),
                method: "cancel".into(),
                params: serde_json::json!({"session_id": sid}),
                reply: Arc::new(StdMutex::new(Some(tx))),
            });
        }
        Ok(Value::Null)
    }

    /// Transport-agnostic per-connection request loop (design 决策 #2): read
    /// NDJSON lines, dispatch each through `handle_request_from` with the
    /// connection's real id, write responses back on the same connection. The
    /// connection is registered for owner routing / subscription broadcasts
    /// and unregistered on every exit path (EOF, error, requested shutdown).
    pub async fn serve_connection(
        self: Arc<Self>,
        conn: Box<dyn Connection>,
    ) -> Result<(), IpcError> {
        let conn_id = conn.id();
        // Register a send-only handle so broadcasts / owner-directed routing
        // can reach this connection while its serve loop awaits `recv` (the
        // reader half needs `&mut`, so the full connection cannot stay in the
        // registry — no std lock may be held across the await).
        if let Ok(mut reg) = self.registry.write() {
            reg.register_writer(conn.writer());
        }
        let serve_result = Arc::clone(&self)
            .serve_connection_requests(conn, &conn_id)
            .await;
        self.cleanup_connection(&conn_id);
        serve_result
    }

    /// Unregister a closed connection and settle the pending questions of every
    /// session it owned (design 错误处理: owner disconnect → skipped).
    fn cleanup_connection(&self, conn_id: &str) {
        let owned = self
            .registry
            .read()
            .ok()
            .map(|r| r.sessions_owned_by(conn_id))
            .unwrap_or_default();
        if let Ok(mut reg) = self.registry.write() {
            reg.unregister(conn_id);
        }
        if let Some(module) = &self.question_module {
            for sid in owned {
                let _ = module.cancel_session(&sid);
            }
        }
    }

    /// Per-connection request loop body. The connection stays owned by this
    /// loop for its whole lifetime (reader half for `recv`), while the
    /// registry holds a detached `ConnectionWriter` handle — so an idle
    /// subscriber never drops out of broadcast reach, and no std RwLock guard
    /// is held across an await. Each request is spawned into its own task so
    /// a long-running `session/run` does not block `permission/respond` or
    /// `$/cancelRequest` on the same connection.
    async fn serve_connection_requests(
        self: Arc<Self>,
        mut conn: Box<dyn Connection>,
        conn_id: &str,
    ) -> Result<(), IpcError> {
        let conn_id_owned = conn_id.to_string();
        let mut pending = tokio::task::JoinSet::new();
        loop {
            let Some(line) = conn.recv().await else {
                break; // EOF — connection dropped here; unregistered by the caller
            };
            let trimmed = match line {
                Ok(l) => l.trim().to_string(),
                Err(e) => return Err(e),
            };
            if trimmed.is_empty() {
                continue;
            }
            let this = Arc::clone(&self);
            let registry = Arc::clone(&self.registry);
            let cid = conn_id_owned.clone();
            let trimmed_owned = trimmed.clone();
            pending.spawn(async move {
                let response = this
                    .handle_request_from(&trimmed_owned, &cid)
                    .await
                    .unwrap_or_else(|e| {
                        format!(
                            r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"{e}"}}}}"#
                        )
                    });
                if !response.is_empty()
                    && let Ok(reg) = registry.read()
                {
                    let _ = reg.send_to(&cid, &response);
                }
            });
            if self.shutdown_requested() {
                break;
            }
        }
        while pending.join_next().await.is_some() {}
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
                        "session_id": p.session_id,
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

    /// Route $/askQuestion notifications to the session owner through the
    /// registry and record the question in the trajectory at notify time. The
    /// settle hook records exactly one QuestionAnswer per question (the module
    /// settles each question through a single winner). Attach once per
    /// transport entry point.
    pub(crate) fn attach_question_notifier(&self) {
        if let Some(module) = &self.question_module {
            module.set_notifier(self.question_notifier());
            module.set_settle_hook(self.question_settle_hook());
        }
    }

    /// Owner-directed question notifier; the returned bool is the module's
    /// "delivered" signal (an absent owner settles the question as skipped).
    fn question_notifier(&self) -> QuestionNotifier {
        let registry = Arc::clone(&self.registry);
        let trajectory = self.trajectory_store.clone();
        Arc::new(move |p: &AskQuestionParams, session_id: &str| {
            record_ask_question(trajectory.as_ref(), session_id, p);
            let owner = registry
                .read()
                .ok()
                .and_then(|r| r.session_owner(&session_id.to_string()).cloned());
            let delivered = owner.is_some();
            if let Some(owner_id) = owner {
                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "$/askQuestion",
                    "params": serde_json::to_value(p).expect("serializable"),
                });
                if let Ok(r) = registry.read() {
                    let _ = r.send_to(&owner_id, &msg.to_string());
                }
            }
            delivered
        })
    }

    /// Settlement sink: one QuestionAnswer trajectory node per settled question.
    fn question_settle_hook(&self) -> SettleHook {
        let trajectory = self.trajectory_store.clone();
        Arc::new(
            move |session_id: &str, _question_id: &str, s: &QuestionSettlement| {
                record_question_answer(trajectory.as_ref(), session_id, s);
            },
        )
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
        self.attach_question_notifier();
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

fn build_progress_frame(
    token: &Value,
    value: &agentj_protocol::streaming::ProgressValue,
) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": { "token": token.clone(), "value": serde_json::to_value(value).expect("serializable") },
    })
    .to_string()
}

/// Map core TODO items to wire items (design contract item 2), enums 1:1.
fn todo_items_into_wire(items: Vec<TodoItemArtifact>) -> Vec<agentj_protocol::streaming::TodoItem> {
    items
        .into_iter()
        .map(|item| agentj_protocol::streaming::TodoItem {
            id: item.id,
            content: item.content,
            status: todo_status_into_wire(item.status),
        })
        .collect()
}

/// 1:1 TODO status mapping — the wire `Unknown` tolerance is never produced
/// by the kernel (artifacts only exist for known statuses).
fn todo_status_into_wire(status: TodoStatusArtifact) -> agentj_protocol::streaming::TodoStatus {
    match status {
        TodoStatusArtifact::Pending => agentj_protocol::streaming::TodoStatus::Pending,
        TodoStatusArtifact::InProgress => agentj_protocol::streaming::TodoStatus::InProgress,
        TodoStatusArtifact::Completed => agentj_protocol::streaming::TodoStatus::Completed,
        TodoStatusArtifact::Cancelled => agentj_protocol::streaming::TodoStatus::Cancelled,
    }
}

fn is_valid_run_id(s: &str) -> bool {
    s.len() <= 128 && Uuid::parse_str(s).is_ok()
}

fn sanitize_run_id(opt: Option<String>) -> Option<String> {
    match opt {
        Some(s) if is_valid_run_id(&s) => Some(s),
        _ => None,
    }
}

/// Map a core file-change artifact to the wire shape (design contract item 3).
/// `diff_truncated` is `Some(true)` only when truncated; `false` stays omitted
/// per the field's `skip_serializing_if`.
fn file_change_into_wire(change: FileChangeArtifact) -> agentj_protocol::streaming::FileChange {
    agentj_protocol::streaming::FileChange {
        path: change.path,
        op: file_change_op_into_wire(change.op),
        additions: change.additions,
        deletions: change.deletions,
        tool_call_id: change.tool_call_id,
        diff: change.diff,
        diff_truncated: change.diff_truncated.then_some(true),
        run_id: sanitize_run_id(change.run_id),
        seq: change.seq,
        source: change.source,
        total_bytes: change.total_bytes,
    }
}

/// 1:1 file-change op mapping (artifact ops are a closed set).
fn file_change_op_into_wire(op: FileChangeOp) -> agentj_protocol::streaming::FileChangeKind {
    match op {
        FileChangeOp::Add => agentj_protocol::streaming::FileChangeKind::Add,
        FileChangeOp::Modify => agentj_protocol::streaming::FileChangeKind::Modify,
        FileChangeOp::Delete => agentj_protocol::streaming::FileChangeKind::Delete,
    }
}

fn history_events_filtered(
    store: &trajectory::TrajectoryStore,
    sid: &str,
    run_id: Option<&str>,
    cursor: Option<u64>,
    limit: usize,
) -> std::io::Result<(
    Vec<agentj_protocol::trajectory::TrajectoryEvent>,
    Option<u64>,
)> {
    let (all, _) = store.read_paged(&sid.to_string(), None, usize::MAX)?;
    let filtered: Vec<_> = if let Some(rid) = run_id {
        all.into_iter()
            .filter(|ev| match &ev.value {
                agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. }
                | agentj_protocol::trajectory::TrajectoryValue::FileChange { run_id, .. } => {
                    run_id.as_deref() == Some(rid)
                }
                _ => true,
            })
            .collect()
    } else {
        all
    };
    let mut out = Vec::new();
    let mut seen: Option<u64> = None;
    for ev in filtered {
        if ev.seq < cursor.unwrap_or(0) {
            continue;
        }
        seen = Some(ev.seq);
        out.push(ev);
        if out.len() == limit {
            break;
        }
    }
    let nxt = if out.len() == limit {
        seen.map(|s| s + 1)
    } else {
        None
    };
    Ok((out, nxt))
}

/// AskQuestion trajectory node recorded at notify time (design 决策 #10).
fn record_ask_question(
    store: Option<&Arc<trajectory::TrajectoryStore>>,
    session_id: &str,
    p: &AskQuestionParams,
) {
    let Some(store) = store else {
        return;
    };
    let value = agentj_protocol::trajectory::TrajectoryValue::AskQuestion {
        question_id: p.question_id.clone(),
        question: p.question.clone(),
        header: p.header.clone(),
        options: p.options.clone(),
        allow_multiple: p.allow_multiple,
        allow_other: p.allow_other,
    };
    if let Err(e) = store.record(&session_id.to_string(), value) {
        tracing::warn!("trajectory record failed for {session_id}: {e}");
    }
}

/// QuestionAnswer trajectory node; the settle hook fires at most once per
/// question, so exactly one node is recorded (module single-winner settle).
fn record_question_answer(
    store: Option<&Arc<trajectory::TrajectoryStore>>,
    session_id: &str,
    s: &QuestionSettlement,
) {
    let Some(store) = store else {
        return;
    };
    let (answers, skipped) = match &s.outcome {
        QuestionOutcome::Answered { answers } => (answers.clone(), false),
        QuestionOutcome::Skipped => (Vec::new(), true),
    };
    let value = agentj_protocol::trajectory::TrajectoryValue::QuestionAnswer {
        question_id: s.question_id.clone(),
        answers,
        skipped,
    };
    if let Err(e) = store.record(&session_id.to_string(), value) {
        tracing::warn!("trajectory record failed for {session_id}: {e}");
    }
}

/// Attach `git_status` to file entries from one snapshot of `root`; a non-git
/// root or git failure degrades to all-null (design error handling).
async fn attach_git_statuses(result: FileListResult, root: &std::path::Path) -> FileListResult {
    let snapshot = agentj_core::git_sample::sample(root).await;
    if snapshot.is_none() {
        tracing::debug!(root = %root.display(), "file/list: git sample unavailable");
    }
    let entries = result
        .entries
        .into_iter()
        .map(|mut entry| {
            match entry.kind {
                FileKind::File => {
                    entry.git_status = snapshot
                        .as_ref()
                        .and_then(|s| file_list::git_statuses(s, &entry.path))
                        .and_then(|(x, y)| file_list::map_git_status(x, y));
                }
                FileKind::Dir => {
                    entry.git_status = snapshot
                        .as_ref()
                        .and_then(|s| file_list::aggregate_dir_status(&entry.path, &s.statuses));
                }
                FileKind::Unknown => {}
            }
            entry
        })
        .collect();
    FileListResult {
        entries,
        truncated: result.truncated,
        total: result.total,
    }
}

/// Map a `file/list` failure onto its JSON-RPC code + `error.data` shape
/// (design error handling).
fn map_file_list_error(
    error: file_list::FileListError,
    requested: &Option<String>,
) -> DispatchError {
    match error {
        file_list::FileListError::NotFound => DispatchError::invalid_params_with_data(
            "file/list: path not found",
            serde_json::json!({ "path": requested, "reason": "not_found" }),
        ),
        file_list::FileListError::NotADirectory => DispatchError::invalid_params_with_data(
            "file/list: path is not a directory",
            serde_json::json!({ "path": requested, "reason": "not_a_directory" }),
        ),
        file_list::FileListError::InvalidDepth => DispatchError::invalid_params_with_data(
            "file/list: depth must be within 1..=8",
            serde_json::json!({ "reason": "invalid_depth" }),
        ),
        file_list::FileListError::OutsideRoot { path } => DispatchError {
            code: error_codes::PATH_NOT_ALLOWED,
            message: "PathNotAllowed".into(),
            data: Some(serde_json::json!({ "path": path, "reason": "outside_project_root" })),
        },
        file_list::FileListError::Io { path, reason } => DispatchError::internal_with_data(
            "file/list: io error",
            serde_json::json!({ "path": path, "reason": reason }),
        ),
    }
}

fn kernel_model_info(
    ctx: Option<&agentj_core::module::KernelContext>,
) -> (
    Option<String>,
    Option<Vec<agentj_protocol::streaming::ProviderInfo>>,
) {
    let Some(ctx) = ctx else { return (None, None) };
    let cfg = ctx.config();
    let default_model = cfg.get_str("model.default_model").filter(|s| !s.is_empty());
    let pairs = ctx.providers().list_with_models();
    let providers = (!pairs.is_empty()).then(|| {
        pairs
            .into_iter()
            .map(|(id, models)| agentj_protocol::streaming::ProviderInfo { id, models })
            .collect()
    });
    (default_model, providers)
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
    fn writer(&self) -> ConnectionWriter {
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(|msg: &str| {
                // Sync trait method — std blocking stdout; tokio stdout's
                // block_on panics inside an async context.
                use std::io::Write;
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(msg.as_bytes())?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                Ok(())
            }),
        )
    }
}

/// Crate-internal fixtures shared by lib and tcp test modules.
#[cfg(test)]
pub(crate) mod test_support {
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop_module::AgentLoopModule;
    use agentj_core::module::*;
    use agentj_core::tool_executor::ToolExecutorModule;
    use agentj_core::turn_runner::TurnRunner;
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

    pub(crate) fn stub_session() -> Arc<InMemoryStore> {
        Arc::new(InMemoryStore::new())
    }

    #[allow(dead_code)]
    pub(crate) fn stub_server() -> crate::IpcServer {
        let session = stub_session();
        crate::IpcServer::new(agentj_core::kernel::Kernel::new(), session)
    }

    /// Build an IpcServer whose kernel has AgentLoopModule + ToolExecutorModule
    /// wired to a MockModelProvider emitting `hello`.
    pub(crate) fn stub_bus_server() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
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
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_bus_server_with_usage() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Usage(TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                })),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_agent_loop() -> TurnRunner {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
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
            session: session_dyn,
            tools,
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let kernel = agentj_core::kernel::Kernel::new();
        TurnRunner::new(
            ctx,
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        MockConfig, stub_bus_server, stub_bus_server_with_usage, stub_session,
    };
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop_module::AgentLoopModule;
    use agentj_core::kernel::Kernel;
    use agentj_core::module::*;
    use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
    use agentj_core::tool_executor::ToolExecutorModule;
    use agentj_core::turn_runner::TurnRunner;
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
        let mut server = IpcServer::new(Kernel::new(), stub_session());
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
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = store.clone();
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
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        let mut server = IpcServer::new(kernel, session_dyn);
        server.permission_module = Some(module.clone());
        (server, module)
    }

    #[tokio::test]
    async fn permission_respond_owner_gate_is_per_session() {
        let (server, module) = permission_server();
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), "conn-a");
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s2".to_string(), "conn-b");
        let ctx1 = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl x"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let d1 = module.check(&ctx1);
        let PermissionDecision::Ask { request_id: rid1 } = d1 else {
            panic!("expected Ask")
        };
        let raw1 = format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
            rid1.0
        );
        let resp = server.handle_request_from(&raw1, "conn-b").await.unwrap();
        assert!(
            resp.contains("-32806"),
            "non-owner must get NotAuthorized: {resp}"
        );
        let resp = server.handle_request_from(&raw1, "conn-a").await.unwrap();
        assert!(resp.contains("\"result\""), "owner must resolve: {resp}");
        let ctx2 = PermissionContext {
            session_id: "s2".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl y"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let d2 = module.check(&ctx2);
        let PermissionDecision::Ask { request_id: rid2 } = d2 else {
            panic!("expected Ask")
        };
        let raw2 = format!(
            r#"{{"jsonrpc":"2.0","id":8,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
            rid2.0
        );
        let resp = server.handle_request_from(&raw2, "conn-b").await.unwrap();
        assert!(resp.contains("\"result\""), "s2 owner must resolve: {resp}");
    }

    #[tokio::test]
    async fn permission_respond_resolves_pending_approval() {
        let (server, module) = permission_server();
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), "stdio");
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
            let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
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

    /// Fixture: an IpcServer wired to a fresh QuestionModule (notifier not yet attached).
    fn question_server() -> (IpcServer, Arc<agentj_question::QuestionModule>) {
        let module = Arc::new(agentj_question::QuestionModule::new());
        let mut server = IpcServer::new(Kernel::new(), stub_session());
        server.question_module = Some(module.clone());
        (server, module)
    }

    fn ask_params(sid: &str) -> agentj_protocol::question::AskQuestionParams {
        use agentj_protocol::question::AskOption;
        agentj_protocol::question::AskQuestionParams {
            question_id: String::new(), // ask_blocking generates the real id
            session_id: sid.to_string(),
            question: "Pick a color".into(),
            header: None,
            options: vec![
                AskOption {
                    id: "red".into(),
                    label: "Red".into(),
                    hint: None,
                },
                AskOption {
                    id: "blue".into(),
                    label: "Blue".into(),
                    hint: None,
                },
            ],
            allow_multiple: false,
            allow_other: true,
            timeout_s: None,
        }
    }

    fn question_id_of(msg: &str) -> String {
        serde_json::from_str::<serde_json::Value>(msg).unwrap()["params"]["question_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn respond_request(qid: &str, answers: &str) -> String {
        format!(
            concat!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"question/respond","#,
                r#""params":{{"question_id":"{}","answers":{}}}}}"#
            ),
            qid, answers
        )
    }

    /// Wait for the owner's `$/askQuestion` notification; return the generated id.
    async fn wait_for_ask_question(server: &Arc<IpcServer>, owner: &str) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(msg) = drain_sent(server, owner)
                    .into_iter()
                    .find(|m| m.contains("$/askQuestion"))
                {
                    return question_id_of(&msg);
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("timed out waiting for $/askQuestion")
    }

    #[tokio::test]
    async fn question_respond_non_owner_rejected() {
        let (server, module) = question_server();
        let server = Arc::new(server);
        server.attach_question_notifier();
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), &a);
        let m = module.clone();
        let params = ask_params("s1");
        let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
        let qid = wait_for_ask_question(&server, &a).await;
        let respond = respond_request(&qid, r#"["red"]"#);
        let resp = server.handle_request_from(&respond, &b).await.unwrap();
        assert!(
            resp.contains("-32806") && resp.contains("NotAuthorized"),
            "non-owner must be rejected with -32806: {resp}"
        );
        let resp = server.handle_request_from(&respond, &a).await.unwrap();
        assert!(
            resp.contains("\"accepted\":true"),
            "owner must resolve: {resp}"
        );
        let (settled_id, outcome) = handle.join().unwrap();
        assert_eq!(settled_id, qid);
        assert_eq!(
            outcome,
            agentj_question::QuestionOutcome::Answered {
                answers: vec!["red".into()]
            }
        );
    }

    #[tokio::test]
    async fn question_respond_unknown_id_is_idempotent_false() {
        let (server, module) = question_server();
        let raw = concat!(
            r#"{"jsonrpc":"2.0","id":4,"method":"question/respond","#,
            r#""params":{"question_id":"nope","answers":[]}}"#,
        );
        let resp = server.handle_request_from(raw, "conn-x").await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["accepted"], false, "{resp}");
        assert!(
            v.get("error").is_none(),
            "unknown id must not error: {resp}"
        );
        assert_eq!(module.pending_count(), 0);
    }

    #[tokio::test]
    async fn cancel_request_settles_pending_question() {
        let (server, module) = question_server();
        let server = Arc::new(server);
        server.attach_question_notifier();
        let a = register_fake_conn(&server);
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), &a);
        let m = module.clone();
        let params = ask_params("s1");
        let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
        let qid = wait_for_ask_question(&server, &a).await;
        assert_eq!(module.pending_count(), 1);
        let raw =
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
        let _ = server.handle_request_from(raw, &a).await.unwrap();
        assert_eq!(
            module.pending_count(),
            0,
            "cancel must settle the pending question"
        );
        let (settled_id, outcome) = handle.join().unwrap();
        assert_eq!(settled_id, qid);
        assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
    }

    #[tokio::test]
    async fn notifier_targets_owner_connection() {
        let (server, module) = question_server();
        let server = Arc::new(server);
        server.attach_question_notifier();
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), &a);
        let m = module.clone();
        let params = ask_params("s1");
        let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
        let qid = wait_for_ask_question(&server, &a).await;
        let ask = drain_sent(&server, &a)
            .into_iter()
            .find(|m| m.contains("$/askQuestion"))
            .expect("owner must receive $/askQuestion");
        let v: serde_json::Value = serde_json::from_str(&ask).unwrap();
        assert_eq!(v["method"], "$/askQuestion");
        assert_eq!(v["params"]["question_id"], qid);
        assert_eq!(v["params"]["session_id"], "s1");
        assert_eq!(v["params"]["question"], "Pick a color");
        assert_eq!(v["params"]["options"].as_array().unwrap().len(), 2);
        let b_sent = drain_sent(&server, &b);
        assert!(
            !b_sent.iter().any(|m| m.contains("$/askQuestion")),
            "non-owner must NOT receive $/askQuestion: {b_sent:?}"
        );
        let _ = module.cancel_session("s1");
        assert_eq!(
            handle.join().unwrap().1,
            agentj_question::QuestionOutcome::Skipped
        );
    }

    #[tokio::test]
    async fn question_respond_maps_validation_errors_to_error_data() {
        let (server, module) = question_server();
        let server = Arc::new(server);
        server.attach_question_notifier();
        let a = register_fake_conn(&server);
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), &a);
        let m = module.clone();
        let params = ask_params("s1");
        let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
        let qid = wait_for_ask_question(&server, &a).await;
        let unknown = respond_request(&qid, r#"["purple"]"#);
        let resp = server.handle_request_from(&unknown, &a).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "{resp}");
        assert_eq!(v["error"]["data"]["unknown_options"][0], "purple", "{resp}");
        let too_many = respond_request(&qid, r#"["red","blue"]"#);
        let resp = server.handle_request_from(&too_many, &a).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "{resp}");
        assert_eq!(v["error"]["data"]["reason"], "too_many_answers", "{resp}");
        assert_eq!(
            module.pending_count(),
            1,
            "rejected validation must leave the question pending"
        );
        let _ = module.cancel_session("s1");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn owner_disconnect_settles_pending_question_skipped() {
        let (server, module) = question_server();
        let server = Arc::new(server);
        server.attach_question_notifier();
        let (conn, log, tx) = scripted_conn("tcp-owner");
        let s = Arc::clone(&server);
        let task = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });
        tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
            .await
            .unwrap();
        let new_resp = wait_for_line(&log, |m| m.contains("\"session_id\"")).await;
        let sid = extract_sid(&new_resp);
        let m = module.clone();
        let params = ask_params(&sid);
        let handle = std::thread::spawn(move || m.ask_blocking(&sid, &params));
        let ask_line = wait_for_line(&log, |m| m.contains("$/askQuestion")).await;
        let qid = question_id_of(&ask_line);
        assert_eq!(module.pending_count(), 1);
        drop(tx); // EOF -> serve_connection unregisters and settles owned sessions
        task.await.unwrap().unwrap();
        assert_eq!(
            module.pending_count(),
            0,
            "disconnect must settle pending questions"
        );
        let (settled_id, outcome) = handle.join().unwrap();
        assert_eq!(settled_id, qid);
        assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
    }

    #[tokio::test]
    async fn question_settle_records_ask_and_answer_trajectory_once() {
        let dir = temp_dir_traj("question");
        let (mut server, module) = question_server();
        server.trajectory_store = Some(wired_store(&dir));
        let server = Arc::new(server);
        server.attach_question_notifier();
        let a = register_fake_conn(&server);
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), &a);
        let m = module.clone();
        let params = ask_params("s1");
        let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
        let qid = wait_for_ask_question(&server, &a).await;
        let respond = respond_request(&qid, r#"["red"]"#);
        let resp = server.handle_request_from(&respond, &a).await.unwrap();
        assert!(resp.contains("\"accepted\":true"), "{resp}");
        assert_eq!(
            handle.join().unwrap().1,
            agentj_question::QuestionOutcome::Answered {
                answers: vec!["red".into()]
            }
        );
        // Duplicate respond is idempotent and must NOT record a second answer.
        let resp = server.handle_request_from(&respond, &a).await.unwrap();
        assert!(resp.contains("\"accepted\":false"), "{resp}");
        let events = read_traj_events(&dir, "s1");
        assert_eq!(traj_kinds(&events), vec!["ask_question", "question_answer"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// M1 regression: JSON-RPC 2.0 §2.1 — a notification (request without an
    /// id, e.g. $/cancelRequest) must produce NO response line, while a
    /// request WITH an id still gets its response.
    #[tokio::test]
    async fn id_less_notification_gets_no_response() {
        let server = IpcServer::new(Kernel::new(), stub_session());
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

    async fn wait_for_pending_run(
        server: &Arc<IpcServer>,
        sid: &str,
    ) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if server.registry.read().unwrap().pending_runs.contains(sid) {
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
        wait_for_pending_run(&server, &sid)
            .await
            .expect("session/run must record the in-flight session id");
        let pending = spawn_pending_approval(&module, &sid);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let cancel = format!(
            r#"{{"jsonrpc":"2.0","method":"$/cancelRequest","params":{{"id":5,"session_id":"{sid}"}}}}"#
        );
        let _ = server.handle_request(&cancel).await.unwrap();
        assert_eq!(
            pending.await.unwrap(),
            PermissionDecision::Deny {
                reason: "cancelled".into()
            }
        );
        let run_resp = run_task.await.unwrap();
        assert!(
            run_resp.contains("\"result\"") || run_resp.contains("-32800"),
            "cancel should either complete or return RequestCancelled: {run_resp}"
        );
        assert!(
            !server.registry.read().unwrap().pending_runs.contains(&sid),
            "pending run cleared when the run finished"
        );
    }

    struct BlockerTool;
    impl agentj_core::module::Tool for BlockerTool {
        fn name(&self) -> &str {
            "blocker"
        }
        fn description(&self) -> &str {
            "blocks"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(
            &self,
            _i: serde_json::Value,
        ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
            std::thread::sleep(std::time::Duration::from_secs(1));
            Ok(serde_json::json!({"ok": 1}))
        }
    }
    struct RepeatingMock {
        c1: std::sync::atomic::AtomicUsize,
        c2: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl agentj_core::module::ModelProvider for RepeatingMock {
        async fn chat_stream(
            &self,
            messages: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<
            futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
            agentj_core::error::ModelError,
        > {
            let is_s2 = messages.iter().any(|m| m.content.contains("run2"));
            let n = if is_s2 {
                self.c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            } else {
                self.c1.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            };
            if n == 0 {
                let (tn, args) = if is_s2 {
                    ("blocker", serde_json::json!({}))
                } else {
                    ("bash", serde_json::json!({"command": "echo hi"}))
                };
                let ev = vec![Ok(ModelEvent::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: tn.into(),
                    arguments: args,
                }))];
                Ok(Box::pin(futures::stream::iter(ev)))
            } else {
                let ev = vec![Ok(ModelEvent::Delta("done".into())), Ok(ModelEvent::Done)];
                Ok(Box::pin(futures::stream::iter(ev)))
            }
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, agentj_core::error::ModelError> {
            Ok(ChatResponse {
                content: "".into(),
                tool_calls: vec![],
            })
        }
    }
    fn repeating_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "bash".to_string(),
            agentj_permission::ToolPolicy {
                mode: agentj_permission::PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        policies.insert(
            "blocker".to_string(),
            agentj_permission::ToolPolicy {
                mode: agentj_permission::PermissionMode::Allow,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let module = Arc::new(agentj_permission::PermissionModule::new(
            policies,
            std::time::Duration::from_secs(10),
        ));
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = store.clone();
        let tools = Arc::new(ToolRegistry::new());
        tools.register(Arc::new(BlockerTool));
        tools.set_permission_checker(module.clone());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(RepeatingMock {
                c1: std::sync::atomic::AtomicUsize::new(0),
                c2: std::sync::atomic::AtomicUsize::new(0),
            }),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = Kernel::new();
        let mut reg =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut reg,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut reg)
            .unwrap();
        let mut server = IpcServer::new(kernel, session_dyn);
        server.permission_module = Some(module.clone());
        (server, module)
    }

    #[tokio::test]
    async fn cancel_request_with_session_id_routes_precisely() {
        let (server, module) = repeating_run_server();
        let server = Arc::new(server);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
        module.set_notifier(Arc::new(move |p: &PermissionRequestParams, sid: &str| {
            let _ = tx.send((sid.to_string(), p.request_id.clone()));
        }));
        let raw1 = r#"{"jsonrpc":"2.0","id":10,"method":"session/run","params":{"session_id":"s1","content":"run1"}}"#;
        let raw2 = r#"{"jsonrpc":"2.0","id":11,"method":"session/run","params":{"session_id":"s2","content":"run2"}}"#;
        let run1 = {
            let s = Arc::clone(&server);
            tokio::spawn(async move { s.handle_request_from(raw1, "conn-a").await.unwrap() })
        };
        let run2 = {
            let s = Arc::clone(&server);
            tokio::spawn(async move { s.handle_request_from(raw2, "conn-b").await.unwrap() })
        };
        wait_for_pending_run(&server, "s1")
            .await
            .expect("s1 must be in-flight");
        wait_for_pending_run(&server, "s2")
            .await
            .expect("s2 must be in-flight");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let (sid, rid) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("s1 must block on Ask")
            .expect("channel closed");
        assert_eq!(sid, "s1", "first Ask must be s1");
        let rid_s1 = Some(rid);
        let cancel =
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s2"}}"#;
        let _ = server.handle_request_from(cancel, "conn-a").await.unwrap();
        let resp2 = tokio::time::timeout(std::time::Duration::from_secs(5), run2)
            .await
            .expect("cancelled run must finish")
            .unwrap();
        assert!(
            resp2.contains("-32800"),
            "cancelled must get -32800: {resp2}"
        );
        assert!(
            server.registry.read().unwrap().pending_runs.contains("s1"),
            "s1 stays"
        );
        let rid1 = rid_s1.clone().expect("s1 rid");
        let raw_ok = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
            rid1
        );
        let _ = server.handle_request_from(&raw_ok, "conn-a").await.unwrap();
        let resp1 = tokio::time::timeout(std::time::Duration::from_secs(5), run1)
            .await
            .expect("s1 must finish")
            .unwrap();
        assert!(resp1.contains("\"result\""), "s1 must complete: {resp1}");
    }

    #[tokio::test]
    async fn cancel_request_without_session_id_single_inflight_falls_back() {
        let (server, module) = blocking_run_server();
        assert!(
            server
                .registry
                .write()
                .unwrap()
                .try_acquire_run(&"s1".to_string())
        );
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

    #[tokio::test]
    async fn cancel_request_without_session_id_multiple_inflight_is_noop() {
        let (server, module) = blocking_run_server();
        assert!(
            server
                .registry
                .write()
                .unwrap()
                .try_acquire_run(&"s1".to_string())
        );
        assert!(
            server
                .registry
                .write()
                .unwrap()
                .try_acquire_run(&"s2".to_string())
        );
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "curl x"}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
            panic!("expected Ask")
        };
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
        let _ = server.handle_request(raw).await.unwrap();
        let out = tokio::time::timeout(std::time::Duration::from_millis(200), handle).await;
        assert!(out.is_err(), "ambiguous cancel must NOT resolve approvals");
    }

    #[tokio::test]
    async fn session_new_returns_valid_id() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["result"]["session_id"].is_string());
        assert!(!v["result"]["session_id"].as_str().unwrap().is_empty());
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(
            v["result"]["project_root"],
            cwd.to_str().unwrap(),
            "session/new without project_root must return the canonicalized cwd"
        );
    }

    /// Temp tree for `session/new` project_root tests (mirrors `temp_dir_traj`).
    fn temp_dir_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aj-ipc-root-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// T20: `session/new {project_root}` stores the canonicalized directory on
    /// the created session (design item 6).
    #[tokio::test]
    async fn session_new_with_project_root_persists_it() {
        let store = stub_session();
        let server = IpcServer::new(Kernel::new(), store.clone());
        let dir = temp_dir_root("persist");
        let canonical = dir.canonicalize().unwrap();
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session/new",
            "params": { "project_root": dir.to_str().unwrap() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            v["error"].is_null(),
            "valid project_root must not error: {resp}"
        );
        let sid = v["result"]["session_id"].as_str().unwrap().to_string();
        assert_eq!(
            v["result"]["project_root"],
            canonical.to_str().unwrap(),
            "session/new must echo the canonicalized request value"
        );
        assert_eq!(store.project_root(&sid), Some(canonical));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// T20: a relative `project_root` is rejected with -32602 carrying
    /// `error.data {path, reason}`; no session is created (validate before create).
    #[tokio::test]
    async fn session_new_rejects_relative_root_with_32602() {
        let dir = temp_dir_root("reject");
        let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
        let server = IpcServer::new(Kernel::new(), store.clone());
        let raw = r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"project_root":"rel/path"}}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
        assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
        assert_eq!(v["error"]["data"]["path"], "rel/path");
        assert!(v["result"].is_null(), "no result on error: {resp}");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            0,
            "invalid project_root must not create a session"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// T20 AC guard: a missing path and a regular file are also rejected with
    /// -32602 + `error.data {path, reason}`.
    #[tokio::test]
    async fn session_new_rejects_missing_and_file_root_with_32602() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let dir = temp_dir_root("badshape");
        let missing = dir.join("missing");
        let file = dir.join("not-a-dir.txt");
        std::fs::write(&file, "x").unwrap();
        for bad in [missing.as_path(), file.as_path()] {
            let raw = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "session/new",
                "params": { "project_root": bad.to_str().unwrap() },
            })
            .to_string();
            let resp = server.handle_request(&raw).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
            assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
            assert_eq!(v["error"]["data"]["path"], bad.to_str().unwrap());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// T20: the optional `data` key is omitted from the wire when None (additive
    /// discipline), and carried when a parameterized error sets it.
    #[test]
    fn error_object_omits_data_when_none() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(9),
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "bad params".into(),
                data: None,
            }),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(
            v["error"].get("data").is_none(),
            "data key must be omitted when None: {v}"
        );
        assert_eq!(v["error"]["code"], -32602);
        assert_eq!(v["error"]["message"], "bad params");

        let err = DispatchError::invalid_params_with_data(
            "bad root",
            serde_json::json!({ "path": "/x", "reason": "invalid_project_root" }),
        );
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], -32602);
        assert_eq!(v["data"]["reason"], "invalid_project_root");
    }

    #[tokio::test]
    async fn session_run_streams_delta() {
        let server = stub_bus_server();
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
    async fn session_run_result_includes_usage_when_reported() {
        let server = stub_bus_server_with_usage();
        let raw = r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["content"], "hello");
        assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
        assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_object());
        assert_eq!(v["error"]["code"], -32601); // Method not found
        assert_eq!(v["id"], 3, "error response must echo request id");
    }

    /// BUG-005: `kernel/status` must be dispatched by the IPC server — the
    /// dashboard polls it on load, and -32601 made the panel show "kernel
    /// unreachable". Wire shape per the KernelStatusResult schema: `version`
    /// string + `modules` array.
    #[tokio::test]
    async fn kernel_status_returns_version_and_modules_array() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":5,"method":"kernel/status"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"],
            serde_json::Value::Null,
            "kernel/status must not error: {resp}"
        );
        assert!(
            v["result"]["version"].is_string(),
            "kernel/status must return a version string: {resp}"
        );
        assert_eq!(
            v["result"]["version"],
            env!("CARGO_PKG_VERSION"),
            "version must reflect the daemon build"
        );
        assert!(
            v["result"]["modules"].is_array(),
            "kernel/status must return a modules array: {resp}"
        );
        assert_eq!(v["id"], 5, "response must echo request id");
    }

    #[tokio::test]
    async fn kernel_status_reports_registered_modules() {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "sys".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        let server = IpcServer::new(kernel, session_dyn);
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":6,"method":"kernel/status"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["result"]["modules"].is_array());
        let modules = v["result"]["modules"].as_array().unwrap();
        assert!(!modules.is_empty(), "modules must be non-empty");
        for m in modules {
            assert!(m["id"].is_string());
            assert_eq!(m["state"], "ready");
        }
    }

    #[tokio::test]
    async fn cancel_returns_minus_32800_when_tool_never_replies() {
        use agentj_core::module::{KernelModule, Module, ModuleMetadata};
        struct BlockingTool;
        impl Module for BlockingTool {
            fn id(&self) -> String {
                "tool_executor".into()
            }
            fn metadata(&self) -> ModuleMetadata {
                ModuleMetadata {
                    id: "tool_executor".into(),
                    name: "blocking".into(),
                    version: "0.1".into(),
                    description: "".into(),
                    author: "".into(),
                }
            }
            fn init(
                &mut self,
                _: &KernelContext,
                _: &mut ModuleRegister,
            ) -> Result<(), ModuleError> {
                Ok(())
            }
            fn shutdown(&mut self) -> Result<(), ModuleError> {
                Ok(())
            }
        }
        impl KernelModule for BlockingTool {
            fn handle(
                &mut self,
                msg: &KernelMessage,
            ) -> Result<agentj_core::module::KernelResponse, ModuleError> {
                if let KernelMessage::ToolCallRequest { reply, .. } = msg
                    && let Some(tx) = reply.lock().unwrap().take()
                {
                    std::mem::forget(tx);
                }
                Ok(agentj_core::module::KernelResponse::Handled)
            }
        }
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = store.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![Ok(ModelEvent::ToolCall(
                ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
            ))])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(Box::new(BlockingTool), &ctx, &mut register)
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "sys".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        let server = Arc::new(IpcServer::new(kernel, session_dyn));
        let new_resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_task = tokio::spawn({
            let s = Arc::clone(&server);
            async move { s.handle_request(&run_raw).await.unwrap() }
        });
        wait_for_pending_run(&server, &sid)
            .await
            .expect("run must be in-flight");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = server
            .handle_request(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#)
            .await
            .unwrap();
        let resp = run_task.await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"]["code"], -32800,
            "cancel must return -32800: {resp}"
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("RequestCancelled")
        );
    }

    #[tokio::test]
    async fn session_run_missing_params_echoes_id_and_invalid_params() {
        let server = IpcServer::new(Kernel::new(), stub_session());
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
        let server = IpcServer::new(Kernel::new(), stub_session());
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
        let mut server = IpcServer::new(Kernel::new(), stub_session());
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
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server.handle_request("not json").await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32700); // Parse error
        assert_eq!(v["id"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn schema_invalid_request_echoes_id_with_invalid_request_code() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":10,"params":{}}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32600); // Invalid Request
        assert_eq!(v["id"], 10);
    }

    #[tokio::test]
    async fn schema_invalid_request_echoes_string_id_with_invalid_request_code() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":"abc","params":{}}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32600); // Invalid Request
        assert_eq!(v["id"], "abc");
    }

    #[tokio::test]
    async fn non_object_json_returns_invalid_request_with_null_id() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let resp = server.handle_request("42").await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32600);
        assert_eq!(v["id"], serde_json::Value::Null);
    }

    #[test]
    fn request_id_extraction_accepts_only_jsonrpc_id_types() {
        let cases = [
            (
                serde_json::json!({"id": "abc"}),
                Some(serde_json::json!("abc")),
            ),
            (serde_json::json!({"id": 7}), Some(serde_json::json!(7))),
            (
                serde_json::json!({"id": null}),
                Some(serde_json::Value::Null),
            ),
            (serde_json::json!({}), None),
            (serde_json::json!({"id": {"x": 1}}), None),
            (serde_json::json!({"id": [1]}), None),
            (serde_json::json!({"id": true}), None),
            (serde_json::json!("42"), None),
        ];
        for (value, expected) in cases {
            assert_eq!(extract_request_id(&value), expected, "value: {value}");
        }
    }

    #[tokio::test]
    async fn kernel_completion_is_observable_for_signal_interrupt() {
        // design #13: the CLI needs to interrupt serve_stdio's blocked read_line when
        // SIGINT/SIGTERM arrives — signal_shutdown + awaiting the kernel handle must
        // terminate the loop. Run serve_stdio with a stdin that stays open, signal
        // shutdown from another task, and assert serve_stdio returns.
        let mut server = IpcServer::new(Kernel::new(), stub_session());
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
        fn writer(&self) -> ConnectionWriter {
            let log = std::sync::Arc::clone(&self.log);
            ConnectionWriter::new(
                self.id.clone(),
                self.kind(),
                self.peer_desc(),
                Arc::new(move |msg: &str| {
                    if let Ok(mut log) = log.lock() {
                        log.push(msg.to_string());
                    }
                    Ok(())
                }),
            )
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
        fn writer(&self) -> ConnectionWriter {
            let log = std::sync::Arc::clone(&self.log);
            ConnectionWriter::new(
                self.id.clone(),
                self.kind(),
                self.peer_desc(),
                Arc::new(move |msg: &str| {
                    if let Ok(mut log) = log.lock() {
                        log.push(msg.to_string());
                    }
                    Ok(())
                }),
            )
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

    /// Parsed `$/progress` frames a fake connection received (stream order).
    fn progress_frames(server: &Arc<IpcServer>, id: &str) -> Vec<serde_json::Value> {
        drain_sent(server, id)
            .iter()
            .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .filter(|v| v["method"] == "$/progress")
            .collect()
    }

    fn extract_sid(resp: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(resp).unwrap();
        v["result"]["session_id"].as_str().unwrap().to_string()
    }

    /// Wait until `predicate` matches a message the connection has sent.
    async fn wait_for_line(log: &SharedLog, predicate: impl Fn(&str) -> bool) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(m) = log
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .find(|m| predicate(m))
                {
                    return m.clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("timed out waiting for a connection log line")
    }

    #[tokio::test]
    async fn initialize_returns_connection_id_and_multi_client() {
        let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
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
    async fn initialize_reports_protocol_version_not_product_version() {
        let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
        let conn_id = register_fake_conn(&server);
        let resp = server
            .handle_request_from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.13.0"}}"#,
                &conn_id,
            )
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["result"]["protocol_version"],
            agentj_protocol::PROTOCOL_VERSION
        );
        assert_eq!(
            v["result"]["server_info"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn initialize_with_wrong_token_is_rejected() {
        let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
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
        let (server, module) = blocking_run_server();
        let server = Arc::new(server);
        let a = register_fake_conn(&server);
        let b = register_fake_conn(&server);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        module.set_notifier(Arc::new(move |p: &PermissionRequestParams, _sid: &str| {
            let _ = tx.send(p.request_id.clone());
        }));
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
        wait_for_pending_run(&server, &sid)
            .await
            .expect("run must record in-flight session");
        let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(r) = rx.try_recv() {
                    return r;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("permission request must arrive");
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
        );
        let resp = server.handle_request_from(&raw, &b).await.unwrap();
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
        wait_for_pending_run(&server, &sid)
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
        let server = Arc::new(stub_bus_server());
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

    /// T23 fixture: accepts any `todo` args; the executor derives the snapshot
    /// artifact itself (that production path is what T23 broadcasts).
    struct TodoMock;

    impl Tool for TodoMock {
        fn name(&self) -> &str {
            "todo"
        }

        fn description(&self) -> &str {
            "test todo"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn execute(
            &self,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
            let count = input["items"].as_array().map(Vec::len).unwrap_or(0);
            Ok(serde_json::json!({"ok": true, "count": count}))
        }
    }

    /// T23 fixture: really writes the file so the executor's pre/post-read pair
    /// yields a `file_change` artifact (op `add` for a fresh target).
    struct WriteMock;

    impl Tool for WriteMock {
        fn name(&self) -> &str {
            "write"
        }

        fn description(&self) -> &str {
            "test write"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn execute(
            &self,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
            let path = input["path"].as_str().unwrap_or("");
            let content = input["content"].as_str().unwrap_or("");
            std::fs::write(path, content).map_err(|e| {
                agentj_core::error::ToolError::ExecutionFailed {
                    name: "write".into(),
                    reason: e.to_string(),
                }
            })?;
            Ok(serde_json::json!({"ok": true}))
        }
    }

    #[test]
    fn todo_event_to_protocol_value_roundtrip() {
        use agentj_core::artifacts::{TodoItemArtifact, TodoStatusArtifact};
        use agentj_protocol::streaming::{ProgressValue, TodoItem, TodoStatus};

        let wire = todo_items_into_wire(vec![
            TodoItemArtifact {
                id: "t1".into(),
                content: "first".into(),
                status: TodoStatusArtifact::Pending,
            },
            TodoItemArtifact {
                id: "t2".into(),
                content: "second".into(),
                status: TodoStatusArtifact::InProgress,
            },
            TodoItemArtifact {
                id: "t3".into(),
                content: "third".into(),
                status: TodoStatusArtifact::Completed,
            },
            TodoItemArtifact {
                id: "t4".into(),
                content: "fourth".into(),
                status: TodoStatusArtifact::Cancelled,
            },
        ]);
        assert_eq!(
            wire,
            vec![
                TodoItem {
                    id: "t1".into(),
                    content: "first".into(),
                    status: TodoStatus::Pending,
                },
                TodoItem {
                    id: "t2".into(),
                    content: "second".into(),
                    status: TodoStatus::InProgress,
                },
                TodoItem {
                    id: "t3".into(),
                    content: "third".into(),
                    status: TodoStatus::Completed,
                },
                TodoItem {
                    id: "t4".into(),
                    content: "fourth".into(),
                    status: TodoStatus::Cancelled,
                },
            ],
            "all four statuses map 1:1"
        );

        let value = ProgressValue::Todo {
            session_id: "s1".into(),
            run_id: None,
            revision: 7,
            items: wire.clone(),
        };
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["kind"], "todo");
        assert_eq!(json["session_id"], "s1");
        assert_eq!(json["revision"], 7);
        assert_eq!(json["items"][1]["status"], "in_progress");
        let back: ProgressValue = serde_json::from_value(json).unwrap();
        match back {
            ProgressValue::Todo {
                session_id,
                revision,
                items,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(revision, 7);
                assert_eq!(items, wire, "items survive the wire roundtrip");
            }
            other => panic!("expected todo, got {other:?}"),
        }
    }

    #[test]
    fn file_change_event_to_protocol_value_roundtrip() {
        use agentj_core::artifacts::{FileChangeArtifact, FileChangeOp};
        use agentj_protocol::streaming::{FileChangeKind, ProgressValue};

        let change = file_change_into_wire(FileChangeArtifact {
            path: "src/lib.rs".into(),
            op: FileChangeOp::Modify,
            additions: 3,
            deletions: 1,
            tool_call_id: Some("c1".into()),
            diff: Some("@@ -1,2 +1,2 @@\n-a\n+b\n".into()),
            diff_truncated: false,
            run_id: None,
            seq: None,
            source: None,
            total_bytes: None,
        });
        assert_eq!(change.op, FileChangeKind::Modify);
        assert_eq!(change.diff_truncated, None, "false must stay omitted");
        let value = ProgressValue::FileChange {
            session_id: "s1".into(),
            run_id: None,
            change,
        };
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["kind"], "file_change");
        assert_eq!(json["session_id"], "s1");
        assert_eq!(json["change"]["path"], "src/lib.rs");
        assert_eq!(json["change"]["op"], "modify");
        assert_eq!(json["change"]["additions"], 3);
        assert_eq!(json["change"]["deletions"], 1);
        assert_eq!(json["change"]["tool_call_id"], "c1");
        assert!(
            json["change"]["diff"].as_str().unwrap().contains("+b"),
            "diff preserved: {json}"
        );
        assert!(
            json["change"].get("diff_truncated").is_none(),
            "false diff_truncated must be skipped: {json}"
        );
        let back: ProgressValue = serde_json::from_value(json).unwrap();
        match back {
            ProgressValue::FileChange {
                session_id, change, ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(change.op, FileChangeKind::Modify);
                assert_eq!(change.diff_truncated, None);
                assert_eq!(change.path, "src/lib.rs");
            }
            other => panic!("expected file_change, got {other:?}"),
        }

        let add = file_change_into_wire(FileChangeArtifact {
            path: "new.txt".into(),
            op: FileChangeOp::Add,
            additions: 1,
            deletions: 0,
            tool_call_id: None,
            diff: None,
            diff_truncated: false,
            run_id: None,
            seq: None,
            source: None,
            total_bytes: None,
        });
        assert_eq!(add.op, FileChangeKind::Add);
        let add_json = serde_json::to_value(&add).unwrap();
        assert!(add_json.get("tool_call_id").is_none());
        assert!(add_json.get("diff").is_none());

        let delete = file_change_into_wire(FileChangeArtifact {
            path: "gone.txt".into(),
            op: FileChangeOp::Delete,
            additions: 0,
            deletions: 4,
            tool_call_id: Some("c9".into()),
            diff: None,
            diff_truncated: true,
            run_id: None,
            seq: None,
            source: None,
            total_bytes: None,
        });
        assert_eq!(delete.op, FileChangeKind::Delete);
        let delete_json = serde_json::to_value(&delete).unwrap();
        assert_eq!(delete_json["diff_truncated"], true, "truncation surfaced");
    }

    /// T5: `is_valid_run_id` requires a parseable UUID within the 128-char cap;
    /// `sanitize_run_id` maps every reject (including `None`) to `None`.
    #[test]
    fn run_id_validators_accept_uuids_and_reject_malformed() {
        let valid = Uuid::new_v4().to_string();
        assert!(is_valid_run_id(&valid), "a real UUID is valid: {valid}");
        assert!(!is_valid_run_id("not-a-uuid"));
        assert!(
            !is_valid_run_id(&"x".repeat(129)),
            "over the 128-char cap is invalid"
        );
        assert_eq!(sanitize_run_id(None), None);
        assert_eq!(sanitize_run_id(Some("not-a-uuid".into())), None);
        assert_eq!(sanitize_run_id(Some(valid.clone())), Some(valid));
    }

    /// T23 AC: a run yielding todo + write artifacts must broadcast both
    /// `$/progress` kinds to subscribers and record them once for replay.
    #[tokio::test]
    async fn subscriber_receives_todo_and_file_change_broadcast() {
        let dir = temp_dir_traj("t23-artifacts");
        let target = std::env::temp_dir().join(format!("aj-ipc-t23-{}.txt", std::process::id()));
        std::fs::remove_file(&target).ok();
        let path = target.to_string_lossy().into_owned();
        let mut server = scripted_run_server_with_tools(
            vec![
                Ok(agentj_core::types::ModelEvent::ToolCall(
                    agentj_core::types::ToolCall {
                        id: "call_todo".into(),
                        name: "todo".into(),
                        arguments: serde_json::json!({"items": [
                            {"content": "write tests", "status": "in_progress"},
                            {"content": "implement", "status": "pending"}
                        ]}),
                    },
                )),
                Ok(agentj_core::types::ModelEvent::ToolCall(
                    agentj_core::types::ToolCall {
                        id: "call_write".into(),
                        name: "write".into(),
                        arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                    },
                )),
                Ok(agentj_core::types::ModelEvent::Done),
            ],
            |tools| {
                tools.register(Arc::new(TodoMock));
                tools.register(Arc::new(WriteMock));
            },
        );
        server.trajectory_store = Some(wired_store(&dir));
        let server = Arc::new(server);
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
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        server.handle_request_from(&run_raw, &a).await.unwrap();

        let frames: Vec<serde_json::Value> = drain_sent(&server, &b)
            .iter()
            .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .filter(|v| v["method"] == "$/progress")
            .collect();
        let todo = frames
            .iter()
            .find(|f| f["params"]["value"]["kind"] == "todo")
            .unwrap_or_else(|| panic!("subscriber must receive a todo frame: {frames:?}"));
        assert_eq!(todo["params"]["token"], 3, "token echoes the request id");
        assert_eq!(todo["params"]["value"]["session_id"], sid);
        assert_eq!(todo["params"]["value"]["revision"], 1);
        assert_eq!(todo["params"]["value"]["items"][0]["status"], "in_progress");
        assert_eq!(todo["params"]["value"]["items"][1]["status"], "pending");

        let change = frames
            .iter()
            .find(|f| f["params"]["value"]["kind"] == "file_change")
            .unwrap_or_else(|| panic!("subscriber must receive a file_change frame: {frames:?}"));
        assert_eq!(change["params"]["token"], 3);
        assert_eq!(change["params"]["value"]["session_id"], sid);
        assert_eq!(change["params"]["value"]["change"]["op"], "add");
        assert_eq!(change["params"]["value"]["change"]["path"], path);
        assert_eq!(change["params"]["value"]["change"]["additions"], 2);
        assert_eq!(
            change["params"]["value"]["change"]["tool_call_id"],
            "call_write"
        );
        assert!(
            change["params"]["value"]["change"]["diff"]
                .as_str()
                .unwrap()
                .contains("+b"),
            "diff preserved: {change}"
        );
        assert!(
            change["params"]["value"]["change"]
                .get("diff_truncated")
                .is_none(),
            "false diff_truncated stays omitted: {change}"
        );

        let events = read_traj_events(&dir, &sid);
        assert_eq!(
            traj_kinds(&events),
            vec![
                "user",
                "tool_call",
                "tool_result",
                "todo",
                "tool_call",
                "tool_result",
                "file_change"
            ],
            "each artifact is recorded exactly once, in stream order"
        );
        let agentj_protocol::trajectory::TrajectoryValue::Todo {
            revision, items, ..
        } = &events[3].value
        else {
            panic!("todo node at index 3: {events:?}")
        };
        assert_eq!(*revision, 1);
        assert_eq!(items[0].content, "write tests");
        assert_eq!(
            items[1].status,
            agentj_protocol::streaming::TodoStatus::Pending
        );
        let agentj_protocol::trajectory::TrajectoryValue::FileChange { change, .. } =
            &events[6].value
        else {
            panic!("file_change node at index 6: {events:?}")
        };
        assert_eq!(change.op, agentj_protocol::streaming::FileChangeKind::Add);
        assert_eq!(change.path, path);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&target).ok();
    }

    /// T5 AC: a live run broadcasts Todo/FileChange frames tagged with the same
    /// `run_id` as `SessionRunResult`, plus per-run `seq`/`source`.
    #[tokio::test]
    async fn broadcast_includes_run_id() {
        let dir = temp_dir_traj("run-id-broadcast");
        let target = std::env::temp_dir().join(format!("aj-ipc-runid-{}.txt", std::process::id()));
        std::fs::remove_file(&target).ok();
        let path = target.to_string_lossy().into_owned();
        let mut server = scripted_run_server_with_tools(
            vec![
                Ok(agentj_core::types::ModelEvent::ToolCall(
                    agentj_core::types::ToolCall {
                        id: "call_todo".into(),
                        name: "todo".into(),
                        arguments: serde_json::json!({"items": [
                            {"content": "write tests", "status": "in_progress"}
                        ]}),
                    },
                )),
                Ok(agentj_core::types::ModelEvent::ToolCall(
                    agentj_core::types::ToolCall {
                        id: "call_write".into(),
                        name: "write".into(),
                        arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                    },
                )),
                Ok(agentj_core::types::ModelEvent::Done),
            ],
            |tools| {
                tools.register(Arc::new(TodoMock));
                tools.register(Arc::new(WriteMock));
            },
        );
        server.trajectory_store = Some(wired_store(&dir));
        let server = Arc::new(server);
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
        let run_raw = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        );
        let run_resp = server.handle_request_from(&run_raw, &a).await.unwrap();
        let run_v: serde_json::Value = serde_json::from_str(&run_resp).unwrap();
        let run_id = run_v["result"]["run_id"]
            .as_str()
            .unwrap_or_else(|| panic!("SessionRunResult must carry run_id: {run_resp}"))
            .to_string();
        assert!(Uuid::parse_str(&run_id).is_ok(), "real UUID: {run_id}");

        let frames = progress_frames(&server, &b);
        let todos: Vec<_> = frames
            .iter()
            .filter(|f| f["params"]["value"]["kind"] == "todo")
            .collect();
        assert!(!todos.is_empty(), "todo frame expected: {frames:?}");
        assert!(
            todos
                .iter()
                .all(|f| f["params"]["value"]["run_id"].as_str() == Some(run_id.as_str())),
            "every todo frame carries the run id: {todos:?}"
        );
        let changes: Vec<_> = frames
            .iter()
            .filter(|f| f["params"]["value"]["kind"] == "file_change")
            .collect();
        assert!(
            !changes.is_empty(),
            "file_change frame expected: {frames:?}"
        );
        for c in &changes {
            let change = &c["params"]["value"]["change"];
            assert_eq!(
                c["params"]["value"]["run_id"].as_str(),
                Some(run_id.as_str()),
                "outer frame run_id: {c}"
            );
            assert_eq!(
                change["run_id"].as_str(),
                Some(run_id.as_str()),
                "wire change run_id: {c}"
            );
            assert_eq!(change["seq"], 1, "first change of the run: {c}");
            assert_eq!(change["source"], "write", "write-sourced change: {c}");
        }
        let recorded = read_traj_events(&dir, &sid);
        let todo_run = recorded.iter().find_map(|e| match &e.value {
            agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. } => {
                Some(run_id.clone())
            }
            _ => None,
        });
        assert_eq!(
            todo_run.flatten().as_deref(),
            Some(run_id.as_str()),
            "replay node shares the broadcast run id"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&target).ok();
    }

    /// BUG-004 regression: §7.2 — when the client omits `progress_token`, the
    /// `$/progress` `params.token` must echo the request id, never null.
    #[tokio::test]
    async fn progress_broadcast_uses_request_id_token_when_token_omitted() {
        let server = Arc::new(stub_bus_server());
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
                    r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
                ),
                &a,
            )
            .await
            .unwrap();
        let b_sent = drain_sent(&server, &b);
        let progress = b_sent
            .iter()
            .find(|m| m.contains("$/progress"))
            .unwrap_or_else(|| panic!("subscriber must receive $/progress: {b_sent:?}"));
        let v: serde_json::Value = serde_json::from_str(progress).unwrap();
        assert!(
            !v["params"]["token"].is_null(),
            "token must never be null: {progress}"
        );
        assert_eq!(
            v["params"]["token"], 2,
            "§7.2: omitted progress_token must fall back to the request id: {progress}"
        );
    }

    /// BUG-004 edge: a `session/run` notification (no request id) still
    /// broadcasts `$/progress`; the token falls back to a deterministic
    /// schema-valid number instead of null.
    #[tokio::test]
    async fn progress_broadcast_notification_without_id_keeps_token_valid() {
        let server = Arc::new(stub_bus_server());
        let a = register_fake_conn(&server);
        let new_resp = server
            .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        server
            .handle_request_from(
                &format!(
                    r#"{{"jsonrpc":"2.0","method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
                ),
                &a,
            )
            .await
            .unwrap();
        let a_sent = drain_sent(&server, &a);
        let progress = a_sent
            .iter()
            .find(|m| m.contains("$/progress"))
            .unwrap_or_else(|| panic!("initiator must receive $/progress: {a_sent:?}"));
        let v: serde_json::Value = serde_json::from_str(progress).unwrap();
        assert!(
            v["params"]["token"].is_i64() || v["params"]["token"].is_string(),
            "token must stay ProgressToken (integer|string), got: {progress}"
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
        wait_for_pending_run(&server, &sid)
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
        let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
        let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
        let (conn_b, log_b, tx_b) = scripted_conn("tcp-B");
        let s = Arc::clone(&server);
        let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
        let s = Arc::clone(&server);
        let task_b = tokio::spawn(async move { s.serve_connection(Box::new(conn_b)).await });
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
        let server = Arc::new(stub_bus_server());
        let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
        let s = Arc::clone(&server);
        let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
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
        assert!(
            server.registry.read().unwrap().writers.is_empty(),
            "disconnected connection's writer handle must be unregistered"
        );
    }

    /// BUG-006 regression: an idle subscriber (session/subscribe then no
    /// further requests, blocked in recv) must still receive $/progress while
    /// another client runs the session. The BUG-001-era detach window removed
    /// connections from the registry during recv, so broadcasts were dropped.
    #[tokio::test]
    async fn serve_connection_broadcasts_progress_to_idle_subscriber() {
        let server = Arc::new(stub_bus_server());
        let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
        let (conn_b, _log_b, tx_b) = scripted_conn("tcp-B");
        let s = Arc::clone(&server);
        let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
        let s = Arc::clone(&server);
        let task_b = tokio::spawn(async move { s.serve_connection(Box::new(conn_b)).await });

        // Subscriber A: initialize → session/new → session/subscribe, then idle.
        tx_a.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#
                .to_string(),
        )
        .await
        .unwrap();
        tx_a.send(r#"{"jsonrpc":"2.0","id":2,"method":"session/new"}"#.to_string())
            .await
            .unwrap();
        let sid = extract_sid(&wait_for_line(&log_a, |m| m.contains("\"session_id\"")).await);
        tx_a.send(format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
        ))
        .await
        .unwrap();
        wait_for_line(&log_a, |m| m.contains("\"id\":3")).await;
        // Let A's serve loop settle into recv (detached from the registry in
        // the buggy build) before the initiator runs the session.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Initiator B: initialize → session/run on the same session.
        tx_b.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#
                .to_string(),
        )
        .await
        .unwrap();
        tx_b.send(format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        ))
        .await
        .unwrap();
        drop(tx_b); // EOF so B's serve loop returns after the run completes
        task_b.await.unwrap().unwrap();

        let a_sent = log_a.lock().unwrap().clone();
        assert!(
            a_sent
                .iter()
                .any(|m| m.contains("$/progress") && m.contains("hello")),
            "idle subscriber must receive $/progress while blocked in recv: {a_sent:?}"
        );

        drop(tx_a);
        let _ = task_a.await;
    }

    #[test]
    fn transport_kind_has_websocket_variant() {
        let ws = TransportKind::WebSocket;
        assert_ne!(ws, TransportKind::Tcp);
        assert_ne!(ws, TransportKind::Stdio);
        assert_ne!(ws, TransportKind::UnixSocket);
    }

    #[derive(Debug)]
    struct CfgEmpty;
    impl agentj_core::module::ConfigProvider for CfgEmpty {
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
    #[derive(Debug)]
    struct CfgModel;
    impl agentj_core::module::ConfigProvider for CfgModel {
        fn get_str(&self, k: &str) -> Option<String> {
            (k == "model.default_model").then(|| "deepseek-chat".into())
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![("mock".into(), "http://localhost:11434".into())]
        }
    }

    #[test]
    fn kernel_model_info_empty_when_no_context_or_config() {
        let (dm, ps) = kernel_model_info(None);
        assert!(dm.is_none());
        assert!(ps.is_none());
        let (dm2, ps2) = kernel_model_info(Some(&agentj_core::module::KernelContext {
            session: stub_session(),
            tools: Arc::new(agentj_core::module::ToolRegistry::new()),
            providers: Arc::new(agentj_core::module::ProviderRegistry::new()),
            config: Arc::new(CfgEmpty),
        }));
        assert!(dm2.is_none());
        assert!(ps2.is_none());
    }

    #[derive(Debug)]
    struct StubModelsProvider(Vec<String>);
    #[async_trait::async_trait]
    impl agentj_core::module::ModelProvider for StubModelsProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<
            futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
            agentj_core::error::ModelError,
        > {
            unimplemented!()
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<agentj_core::types::ChatResponse, agentj_core::error::ModelError> {
            unimplemented!()
        }
        fn models(&self) -> Vec<String> {
            self.0.clone()
        }
    }

    #[test]
    fn kernel_model_info_surfaces_registered_provider_models() {
        let registry = agentj_core::module::ProviderRegistry::new();
        registry.register(
            "opencode-go".into(),
            Arc::new(StubModelsProvider(vec!["m1".into()])),
        );
        let (default_model, providers) =
            kernel_model_info(Some(&agentj_core::module::KernelContext {
                session: stub_session(),
                tools: Arc::new(agentj_core::module::ToolRegistry::new()),
                providers: Arc::new(registry),
                config: Arc::new(CfgModel),
            }));
        assert_eq!(default_model.as_deref(), Some("deepseek-chat"));
        let ps = providers.expect("providers present when a provider is registered");
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].id, "opencode-go");
        assert_eq!(ps[0].models, vec!["m1".to_string()]);
    }

    #[test]
    fn kernel_model_info_reports_empty_models_for_silent_provider() {
        let registry = agentj_core::module::ProviderRegistry::new();
        registry.register("silent".into(), Arc::new(StubModelsProvider(vec![])));
        let (_, providers) = kernel_model_info(Some(&agentj_core::module::KernelContext {
            session: stub_session(),
            tools: Arc::new(agentj_core::module::ToolRegistry::new()),
            providers: Arc::new(registry),
            config: Arc::new(CfgModel),
        }));
        let ps = providers.expect("providers present when a provider is registered");
        assert!(ps[0].models.is_empty());
    }

    #[test]
    fn kernel_model_info_reflects_registered_not_config_listed_providers() {
        // Config lists a provider but the registry is empty: kernel/status must report none.
        let (_, providers) = kernel_model_info(Some(&agentj_core::module::KernelContext {
            session: stub_session(),
            tools: Arc::new(agentj_core::module::ToolRegistry::new()),
            providers: Arc::new(agentj_core::module::ProviderRegistry::new()),
            config: Arc::new(CfgModel),
        }));
        assert!(providers.is_none());
    }

    fn temp_dir_traj(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aj-ipc-traj-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wired_store(dir: &std::path::Path) -> Arc<crate::trajectory::TrajectoryStore> {
        Arc::new(crate::trajectory::TrajectoryStore::new(
            dir.to_path_buf(),
            None,
            None,
        ))
    }

    fn read_traj_events(
        dir: &std::path::Path,
        sid: &str,
    ) -> Vec<agentj_protocol::trajectory::TrajectoryEvent> {
        let path = dir.join(format!("{sid}.trajectory"));
        if !path.exists() {
            return vec![];
        }
        let f = std::fs::File::open(path).unwrap();
        std::io::BufRead::lines(std::io::BufReader::new(f))
            .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
            .collect()
    }

    fn traj_kinds(events: &[agentj_protocol::trajectory::TrajectoryEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| {
                serde_json::to_value(&e.value).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    /// T5 fixture: serves `script` on the first `chat_stream` of each run and a
    /// bare `Done` afterwards, keyed by the run's user content, so two
    /// sequential runs on one session each replay one tool batch.
    struct TwoRunMockProvider {
        script: Vec<ModelEvent>,
        first_run_calls: std::sync::atomic::AtomicUsize,
        second_run_calls: std::sync::atomic::AtomicUsize,
    }

    impl TwoRunMockProvider {
        fn new(script: Vec<ModelEvent>) -> Self {
            Self {
                script,
                first_run_calls: std::sync::atomic::AtomicUsize::new(0),
                second_run_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl agentj_core::module::ModelProvider for TwoRunMockProvider {
        async fn chat_stream(
            &self,
            messages: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<
            futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
            agentj_core::error::ModelError,
        > {
            let run_calls = if messages.iter().any(|m| m.content.contains("second-turn")) {
                &self.second_run_calls
            } else {
                &self.first_run_calls
            };
            let n = run_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let events: Vec<Result<ModelEvent, agentj_core::error::ModelError>> = if n == 0 {
                self.script.iter().cloned().map(Ok).collect()
            } else {
                vec![Ok(ModelEvent::Done)]
            };
            Ok(Box::pin(futures::stream::iter(events)))
        }

        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, agentj_core::error::ModelError> {
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    fn scripted_run_server(
        script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
    ) -> IpcServer {
        scripted_run_server_with_tools(script, |_| {})
    }

    /// Common test-kernel wiring: ToolExecutor + AgentLoop over `provider`.
    fn run_server_with_provider(
        provider: Arc<dyn agentj_core::module::ModelProvider>,
        register_tools: impl FnOnce(&ToolRegistry),
    ) -> IpcServer {
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = store.clone();
        let tools = Arc::new(ToolRegistry::new());
        register_tools(&tools);
        let providers = Arc::new(ProviderRegistry::new());
        providers.register("mock".into(), provider);
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        IpcServer::new(kernel, session_dyn)
    }

    /// T23: like `scripted_run_server`, but lets the caller register test tools so
    /// tool artifacts (todo / write) flow as real `AgentEvent`s through the bus.
    fn scripted_run_server_with_tools(
        script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
        register_tools: impl FnOnce(&ToolRegistry),
    ) -> IpcServer {
        run_server_with_provider(Arc::new(MockModelProvider::new(script)), register_tools)
    }

    fn two_run_server(
        script: Vec<agentj_core::types::ModelEvent>,
        register_tools: impl FnOnce(&ToolRegistry),
    ) -> IpcServer {
        run_server_with_provider(Arc::new(TwoRunMockProvider::new(script)), register_tools)
    }

    #[tokio::test]
    async fn run_records_user_thinking_usage_and_keeps_response_shape() {
        let dir = temp_dir_traj("usage");
        let mut server = stub_bus_server_with_usage();
        server.trajectory_store = Some(wired_store(&dir));
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["content"], "hello");
        assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
        assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
        let events = read_traj_events(&dir, "s1");
        assert_eq!(traj_kinds(&events), vec!["user", "thinking", "usage"]);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
        let agentj_protocol::trajectory::TrajectoryValue::User { content } = &events[0].value
        else {
            panic!()
        };
        assert_eq!(content, "hi");
        let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
        else {
            panic!()
        };
        assert_eq!(content, "hello");
        assert_eq!(events[2].session_id, "s1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_records_one_merged_thinking_node_for_two_deltas() {
        let dir = temp_dir_traj("merge");
        let mut server = scripted_run_server(vec![
            Ok(agentj_core::types::ModelEvent::Delta("hel".into())),
            Ok(agentj_core::types::ModelEvent::Delta("lo".into())),
            Ok(agentj_core::types::ModelEvent::Done),
        ]);
        server.trajectory_store = Some(wired_store(&dir));
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        assert!(resp.contains("\"result\""), "{resp}");
        let events = read_traj_events(&dir, "s1");
        assert_eq!(
            traj_kinds(&events),
            vec!["user", "thinking"],
            "exactly two nodes"
        );
        let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
        else {
            panic!("expected one merged thinking node, got {events:?}")
        };
        assert_eq!(content, "hello", "both deltas concatenated into ONE node");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_records_tool_call_and_tool_result_nodes() {
        let dir = temp_dir_traj("tools");
        let mut server = scripted_run_server(vec![
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "curl x"}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::Done),
        ]);
        server.trajectory_store = Some(wired_store(&dir));
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        assert!(resp.contains("\"result\""), "{resp}");
        let events = read_traj_events(&dir, "s1");
        assert_eq!(
            traj_kinds(&events),
            vec!["user", "tool_call", "tool_result"]
        );
        let agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call } = &events[1].value
        else {
            panic!("tool_call node at index 1: {events:?}")
        };
        assert_eq!(tool_call.id, "call_1");
        assert_eq!(tool_call.name, "bash");
        assert_eq!(tool_call.arguments["command"], "curl x");
        let agentj_protocol::trajectory::TrajectoryValue::ToolResult {
            tool_call_id,
            content,
        } = &events[2].value
        else {
            panic!("tool_result node at index 2: {events:?}")
        };
        assert_eq!(tool_call_id, "call_1");
        assert!(
            content.starts_with("Error:"),
            "unregistered tool surfaces as executor error CONTENT (C9: content is a plain string): {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_records_error_node_on_provider_failure() {
        let dir = temp_dir_traj("boom");
        let mut server = scripted_run_server(vec![
            Ok(agentj_core::types::ModelEvent::Delta("x".into())),
            Err(agentj_core::error::ModelError::Network("boom".into())),
        ]);
        server.trajectory_store = Some(wired_store(&dir));
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        assert!(
            resp.contains("-32603"),
            "internal error, not cancel: {resp}"
        );
        let events = read_traj_events(&dir, "s1");
        assert_eq!(traj_kinds(&events), vec!["user", "thinking", "error"]);
        let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
        else {
            panic!()
        };
        assert_eq!(content, "x");
        let agentj_protocol::trajectory::TrajectoryValue::Error { message } = &events[2].value
        else {
            panic!()
        };
        assert!(
            message.contains("boom"),
            "error node carries the failure: {message}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn cancelled_run_trajectory_has_no_error_or_tail_nodes() {
        let dir = temp_dir_traj("cancel");
        let (mut server, _module) = blocking_run_server();
        server.trajectory_store = Some(wired_store(&dir));
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), "stdio");
        let server = Arc::new(server);
        let run = tokio::spawn({
            let s = Arc::clone(&server);
            async move {
                s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
                    .await
                    .unwrap()
            }
        });
        wait_for_pending_run(&server, "s1")
            .await
            .expect("run must be in-flight");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let _ = server
            .handle_request(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#)
            .await
            .unwrap();
        let resp = run.await.unwrap();
        // Cancel should surface -32800; allow a brief retry if race produced tool_result instead.
        if !resp.contains("-32800") {
            // If we got a normal result, the permission deny path won; still verify trajectory
            // has no error/usage nodes and at least user+tool_call.
            let events = read_traj_events(&dir, "s1");
            assert!(
                traj_kinds(&events).starts_with(&["user".to_string(), "tool_call".to_string()]),
                "cancelled run must at least have user+tool_call: {events:?}"
            );
            assert!(
                !traj_kinds(&events).contains(&"error".to_string()),
                "cancelled run must not have error node: {events:?}"
            );
            assert!(
                !traj_kinds(&events).contains(&"usage".to_string()),
                "cancelled run must not have usage node: {events:?}"
            );
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        let events = read_traj_events(&dir, "s1");
        // Strict expectation when -32800 wins: only user and tool_call, no tails.
        // Allow an extra tool_result with cancelled content if permission path raced,
        // but never error/usage.
        let kinds = traj_kinds(&events);
        assert!(
            kinds == vec!["user", "tool_call"] || kinds == vec!["user", "tool_call", "tool_result"],
            "tool_call was emitted at request-send (pre-Ask); the cancel discards pending thinking and records NO error/usage/permission tail: {events:?}"
        );
        if kinds.len() == 3 {
            let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } =
                &events[2].value
            else {
                panic!("third node must be tool_result: {events:?}");
            };
            assert!(
                content.contains("cancelled"),
                "extra tool_result must be cancelled deny: {content}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn respond_allow_records_permission_node_between_call_and_result() {
        let dir = temp_dir_traj("settle");
        let (mut server, module) = blocking_run_server();
        server.trajectory_store = Some(wired_store(&dir));
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), "stdio");
        let server = Arc::new(server);
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        module.set_notifier(Arc::new(
            move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
                let _ = ask_tx.send(p.request_id.clone());
            },
        ));
        let run = tokio::spawn({
            let s = Arc::clone(&server);
            async move {
                s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                    .await
                    .unwrap()
            }
        });
        let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(r) = ask_rx.try_recv() {
                    return r;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the run's bash execution must register an Ask");
        let respond = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
        );
        let resp = server.handle_request(&respond).await.unwrap();
        assert!(
            resp.contains("\"result\""),
            "owner respond accepted: {resp}"
        );
        let resp = run.await.unwrap();
        assert!(resp.contains("\"result\""), "run completed: {resp}");
        let events = read_traj_events(&dir, "s1");
        assert_eq!(
            traj_kinds(&events),
            vec!["user", "tool_call", "permission", "tool_result"]
        );
        let agentj_protocol::trajectory::TrajectoryValue::Permission {
            request_id,
            tool,
            decision,
        } = &events[2].value
        else {
            panic!("permission node at index 2: {events:?}")
        };
        assert_eq!(request_id, &rid);
        assert_eq!(tool, "bash");
        assert_eq!(
            *decision,
            agentj_protocol::permission::PermissionDecision::Allow
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn respond_deny_records_deny_and_run_still_completes() {
        let dir = temp_dir_traj("deny");
        let (mut server, module) = blocking_run_server();
        server.trajectory_store = Some(wired_store(&dir));
        server
            .registry
            .write()
            .unwrap()
            .set_session_owner(&"s1".to_string(), "stdio");
        let server = Arc::new(server);
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        module.set_notifier(Arc::new(
            move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
                let _ = ask_tx.send(p.request_id.clone());
            },
        ));
        let run = tokio::spawn({
            let s = Arc::clone(&server);
            async move {
                s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                    .await
                    .unwrap()
            }
        });
        let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(r) = ask_rx.try_recv() {
                    return r;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the run's bash execution must register an Ask");
        let respond = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"deny"}}}}"#
        );
        let resp = server.handle_request(&respond).await.unwrap();
        assert!(
            resp.contains("\"result\""),
            "owner respond accepted: {resp}"
        );
        let resp = run.await.unwrap();
        assert!(
            resp.contains("\"result\""),
            "denied tool is a content error, not a run error: {resp}"
        );
        let events = read_traj_events(&dir, "s1");
        assert_eq!(
            traj_kinds(&events),
            vec!["user", "tool_call", "permission", "tool_result"]
        );
        let agentj_protocol::trajectory::TrajectoryValue::Permission { decision, .. } =
            &events[2].value
        else {
            panic!("permission node at index 2: {events:?}")
        };
        assert_eq!(
            *decision,
            agentj_protocol::permission::PermissionDecision::Deny
        );
        let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } =
            &events[3].value
        else {
            panic!("tool_result at index 3: {events:?}")
        };
        assert!(
            content.starts_with("Error:"),
            "executor denial flows into tool_result content (C9): {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn session_list_returns_file_backed_rows() {
        let dir = temp_dir_traj("list");
        let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
        let session: Arc<dyn SessionStore> = store.clone();
        let server = IpcServer::new(Kernel::new(), session);
        let sid = store.create().unwrap();
        store
            .append(
                &sid,
                ChatMessage {
                    role: Role::User,
                    content: "x".repeat(150),
                    tool_call_id: None,
                    tool_calls: vec![],
                },
            )
            .unwrap();
        let raw = r#"{"jsonrpc":"2.0","id":4,"method":"session/list"}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let row = &v["result"]["sessions"][0];
        assert_eq!(row["session_id"].as_str(), Some(sid.as_str()));
        assert_eq!(row["state"], "active");
        assert!(row["updated_at"].is_u64());
        assert_eq!(row["preview"].as_str().unwrap().len(), 120);
        assert!(row["preview"].as_str().unwrap().starts_with("xxxxx"));
        store.archive(&sid).unwrap();
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":5,"method":"session/list"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["sessions"][0]["state"], "archived");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression guard: `session/status` is declared in the registry, schema,
    /// SDK and spec (§8.8) and is the existence probe every client fires
    /// before `session/subscribe`. It must be dispatched — falling through to
    /// `-32601 method not found` breaks every session click in Web/TUI.
    #[tokio::test]
    async fn session_status_returns_store_status() {
        let store = stub_session();
        let server = IpcServer::new(Kernel::new(), store.clone());
        let sid = store.create().unwrap();
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "session/status",
            "params": { "session_id": sid.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            v["error"].is_null(),
            "session/status must be implemented, not -32601: {resp}"
        );
        assert_eq!(v["result"]["status"], "active");
        store.archive(&sid).unwrap();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["status"], "archived");
    }

    /// T14: `session/status` reports the effective project root — the stored
    /// canonicalized root when present, else the canonicalized cwd fallback.
    #[tokio::test]
    async fn dispatch_session_status_reports_effective_project_root() {
        let store = stub_session();
        let server = IpcServer::new(Kernel::new(), store.clone());
        let dir = temp_dir_root("status-eff-root");
        let canonical = dir.canonicalize().unwrap();
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "session/new",
            "params": { "project_root": dir.to_str().unwrap() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let sid = v["result"]["session_id"].as_str().unwrap().to_string();
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "session/status",
            "params": { "session_id": sid },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["result"]["project_root"],
            canonical.to_str().unwrap(),
            "status must report the stored canonicalized root"
        );

        let bare = store.create().unwrap();
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "session/status",
            "params": { "session_id": bare.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(
            v["result"]["project_root"],
            cwd.to_str().unwrap(),
            "status without a stored root must fall back to the canonicalized cwd"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Unknown session id maps to the documented `-32802 SessionNotFound`
    /// with `error.data.session_id` (§8.8), never the generic `-32601`.
    #[tokio::test]
    async fn session_status_unknown_session_returns_32802() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let raw =
            r#"{"jsonrpc":"2.0","id":13,"method":"session/status","params":{"session_id":"nope"}}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"]["code"], -32802,
            "unknown session must be SessionNotFound: {resp}"
        );
        assert_eq!(v["error"]["data"]["session_id"], "nope");
    }

    /// Regression guard: `session/archive` is declared in the registry/spec
    /// §8.9 and exported by the TUI session hook; it must be dispatched.
    /// Only active sessions may be archived — repeats are `-32803`.
    #[tokio::test]
    async fn session_archive_transitions_and_rejects_inactive() {
        let store = stub_session();
        let server = IpcServer::new(Kernel::new(), store.clone());
        let sid = store.create().unwrap();
        let archive = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "session/archive",
            "params": { "session_id": sid.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&archive).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            v["error"].is_null(),
            "session/archive must be implemented, not -32601: {resp}"
        );
        assert!(v["result"].is_null(), "archive result is null: {resp}");
        let status = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "session/status",
            "params": { "session_id": sid.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&status).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["status"], "archived");
        let resp = server.handle_request(&archive).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"]["code"], -32803,
            "repeat archive must be SessionInactive: {resp}"
        );
        assert_eq!(v["error"]["data"]["status"], "archived");
        let unknown = r#"{"jsonrpc":"2.0","id":16,"method":"session/archive","params":{"session_id":"nope"}}"#;
        let resp = server.handle_request(unknown).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32802, "unknown archive: {resp}");
    }

    /// Regression guard: `session/delete` (§8.10) must be dispatched;
    /// deleting a deleted session is `-32803 SessionInactive`.
    #[tokio::test]
    async fn session_delete_transitions_and_rejects_deleted() {
        let store = stub_session();
        let server = IpcServer::new(Kernel::new(), store.clone());
        let sid = store.create().unwrap();
        let delete = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "session/delete",
            "params": { "session_id": sid.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&delete).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            v["error"].is_null(),
            "session/delete must be implemented, not -32601: {resp}"
        );
        assert!(v["result"].is_null(), "delete result is null: {resp}");
        let status = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "session/status",
            "params": { "session_id": sid.as_str() },
        })
        .to_string();
        let resp = server.handle_request(&status).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["status"], "deleted");
        let resp = server.handle_request(&delete).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"]["code"], -32803,
            "repeat delete must be SessionInactive: {resp}"
        );
        assert_eq!(v["error"]["data"]["status"], "deleted");
        let unknown =
            r#"{"jsonrpc":"2.0","id":19,"method":"session/delete","params":{"session_id":"nope"}}"#;
        let resp = server.handle_request(unknown).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32802, "unknown delete: {resp}");
    }

    #[tokio::test]
    async fn session_history_pages_recorded_events() {
        let dir = temp_dir_traj("history");
        let mut server = stub_bus_server_with_usage();
        server.trajectory_store = Some(wired_store(&dir));
        server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        let raw = r#"{"jsonrpc":"2.0","id":6,"method":"session/history","params":{"session_id":"s1","limit":2}}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let page1 = &v["result"];
        assert_eq!(page1["events"].as_array().unwrap().len(), 2);
        assert_eq!(page1["events"][0]["seq"], 1);
        assert_eq!(page1["events"][0]["value"]["kind"], "user");
        assert_eq!(page1["events"][1]["value"]["kind"], "thinking");
        assert_eq!(page1["next_cursor"], 3);
        let raw2 = r#"{"jsonrpc":"2.0","id":7,"method":"session/history","params":{"session_id":"s1","cursor":3,"limit":2}}"#;
        let resp2 = server.handle_request(raw2).await.unwrap();
        let v2: serde_json::Value = serde_json::from_str(&resp2).unwrap();
        assert_eq!(v2["result"]["events"][0]["value"]["kind"], "usage");
        assert!(v2["result"].get("next_cursor").is_none(), "no next page");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Dispatch one `session/run` and return its `SessionRunResult.run_id`.
    async fn run_and_run_id(server: &IpcServer, sid: &str, content: &str, id: u64) -> String {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"session/run","params":{{"session_id":"{sid}","content":"{content}"}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"]["run_id"]
            .as_str()
            .unwrap_or_else(|| panic!("run must report a run_id: {resp}"))
            .to_string()
    }

    /// Mirror of the T5 run filter: Todo/FileChange keep only `run_id`, all
    /// other trajectory kinds pass.
    fn event_kept_for_run(
        value: &agentj_protocol::trajectory::TrajectoryValue,
        run_id: &str,
    ) -> bool {
        match value {
            agentj_protocol::trajectory::TrajectoryValue::Todo { run_id: r, .. }
            | agentj_protocol::trajectory::TrajectoryValue::FileChange { run_id: r, .. } => {
                r.as_deref() == Some(run_id)
            }
            _ => true,
        }
    }

    /// T5 AC: `session/history {run_id}` keeps that run's Todo/FileChange, lets
    /// every other kind through, and filters before paginating.
    #[tokio::test]
    async fn history_filters_by_run_id() {
        let dir = temp_dir_traj("history-run-filter");
        let target =
            std::env::temp_dir().join(format!("aj-ipc-runfilter-{}.txt", std::process::id()));
        std::fs::remove_file(&target).ok();
        let path = target.to_string_lossy().into_owned();
        let script = vec![
            agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
                id: "call_todo".into(),
                name: "todo".into(),
                arguments: serde_json::json!({"items": [
                    {"content": "step", "status": "in_progress"}
                ]}),
            }),
            agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
                id: "call_write".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
            }),
            agentj_core::types::ModelEvent::Done,
        ];
        let mut server = two_run_server(script, |tools| {
            tools.register(Arc::new(TodoMock));
            tools.register(Arc::new(WriteMock));
        });
        server.trajectory_store = Some(wired_store(&dir));
        let new_resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
            .await
            .unwrap();
        let sid = extract_sid(&new_resp);
        let run1 = run_and_run_id(&server, &sid, "first-turn", 2).await;
        std::fs::remove_file(&target).ok();
        let run2 = run_and_run_id(&server, &sid, "second-turn", 3).await;
        assert_ne!(run1, run2, "each run gets a fresh UUID");

        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","limit":500}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_null(), "r1 filter accepted: {resp}");
        let events = v["result"]["events"].as_array().unwrap();
        let kinds: Vec<&str> = events
            .iter()
            .filter_map(|e| e["value"]["kind"].as_str())
            .collect();
        for e in events {
            let kind = e["value"]["kind"].as_str().unwrap();
            if kind == "todo" || kind == "file_change" {
                assert_eq!(
                    e["value"]["run_id"].as_str(),
                    Some(run1.as_str()),
                    "artifacts stay r1-scoped: {e}"
                );
            }
        }
        assert_eq!(kinds.iter().filter(|k| **k == "todo").count(), 1);
        assert_eq!(kinds.iter().filter(|k| **k == "file_change").count(), 1);
        assert_eq!(
            kinds.iter().filter(|k| **k == "user").count(),
            2,
            "non-artifact kinds from both runs pass through"
        );
        assert!(kinds.contains(&"tool_call"));
        assert!(v["result"].get("next_cursor").is_none(), "single page");

        // Filter first, then paginate: a cursor at r2's Todo must skip that
        // node instead of returning it as the first page item.
        let all = read_traj_events(&dir, &sid);
        let r2_todo_seq = all
            .iter()
            .find(|e| {
                matches!(
                    &e.value,
                    agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. }
                        if run_id.as_deref() == Some(run2.as_str())
                )
            })
            .expect("r2 todo recorded")
            .seq;
        let expected = all
            .iter()
            .find(|e| e.seq >= r2_todo_seq && event_kept_for_run(&e.value, &run1))
            .expect("a later kept event exists");
        assert_ne!(expected.seq, r2_todo_seq, "the r2 todo is filtered out");
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","cursor":{r2_todo_seq},"limit":1}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let page = v["result"]["events"].as_array().unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0]["seq"], expected.seq);
        assert_eq!(v["result"]["next_cursor"], expected.seq + 1);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&target).ok();
    }

    /// T5 AC: `session/history` rejects a malformed `run_id` as `-32602` with
    /// `data.reason == "invalid_run_id"`; a valid UUID is accepted.
    #[tokio::test]
    async fn history_invalid_run_id_rejects() {
        let server = IpcServer::new(Kernel::new(), stub_session());
        let long = "a".repeat(129);
        for bad in ["not-a-uuid", long.as_str()] {
            let raw = format!(
                r#"{{"jsonrpc":"2.0","id":20,"method":"session/history","params":{{"session_id":"s1","run_id":"{bad}"}}}}"#
            );
            let resp = server.handle_request(&raw).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(v["error"]["code"], -32602, "rejected: {resp}");
            assert_eq!(
                v["error"]["data"]["reason"], "invalid_run_id",
                "reason tag: {resp}"
            );
        }
        let valid = Uuid::new_v4().to_string();
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":21,"method":"session/history","params":{{"session_id":"s1","run_id":"{valid}"}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_null(), "valid UUID accepted: {resp}");
        assert_eq!(v["result"]["events"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn session_history_unknown_sid_and_no_store_return_empty() {
        let dir = temp_dir_traj("none");
        let server = stub_bus_server_with_usage();
        let raw =
            r#"{"jsonrpc":"2.0","id":8,"method":"session/history","params":{"session_id":"nope"}}"#;
        let resp = server.handle_request(raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["error"].is_null(), "unknown sid is not an error: {resp}");
        assert_eq!(v["result"]["events"], serde_json::json!([]));
        assert!(v["result"].get("next_cursor").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn session_list_mirrors_store_list_order() {
        let dir = temp_dir_traj("order");
        let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
        let session: Arc<dyn SessionStore> = store.clone();
        let server = IpcServer::new(Kernel::new(), session);
        let a = store.create().unwrap();
        store
            .append(
                &a,
                ChatMessage {
                    role: Role::User,
                    content: "first".into(),
                    tool_call_id: None,
                    tool_calls: vec![],
                },
            )
            .unwrap();
        let b = store.create().unwrap();
        store
            .append(
                &b,
                ChatMessage {
                    role: Role::User,
                    content: "second".into(),
                    tool_call_id: None,
                    tool_calls: vec![],
                },
            )
            .unwrap();
        store.archive(&b).unwrap();
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":9,"method":"session/list"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let rows = v["result"]["sessions"].as_array().expect("sessions array");
        assert_eq!(rows.len(), 2, "both sessions listed: {resp}");
        let entries = store.list().unwrap();
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(rows[i]["session_id"].as_str(), Some(e.session_id.as_str()));
            let want_state = format!("{:?}", e.status).to_lowercase();
            assert_eq!(rows[i]["state"].as_str(), Some(want_state.as_str()));
            assert_eq!(rows[i]["updated_at"].as_u64(), Some(e.updated_at));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn session_history_clamps_limit_into_range() {
        let dir = temp_dir_traj("clamp");
        let mut server = stub_bus_server_with_usage();
        server.trajectory_store = Some(wired_store(&dir));
        server
            .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
            .await
            .unwrap();
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/history","params":{"session_id":"s1","limit":0}}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["events"].as_array().unwrap().len(), 1);
        assert_eq!(v["result"]["events"][0]["seq"], 1);
        assert_eq!(
            v["result"]["next_cursor"], 2,
            "window full -> cursor advances"
        );
        let resp = server
            .handle_request(r#"{"jsonrpc":"2.0","id":11,"method":"session/history","params":{"session_id":"s1","limit":9999}}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["events"].as_array().unwrap().len(), 3);
        assert!(
            v["result"].get("next_cursor").is_none(),
            "nothing left behind"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Temp project root for the T21 `file/list` dispatch tests (std-only).
    fn temp_dir_filelist(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aj-ipc-filedisp-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One isolated session whose `project_root` is `root`.
    fn file_list_server(root: &std::path::Path) -> (IpcServer, String) {
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let sid = store.create().unwrap();
        store
            .set_project_root(&sid, root.canonicalize().unwrap())
            .unwrap();
        let session: Arc<dyn SessionStore> = store;
        (IpcServer::new(Kernel::new(), session), sid)
    }

    /// Dispatch one `file/list` request and return the parsed JSON-RPC response.
    async fn file_list_call(
        server: &IpcServer,
        sid: &str,
        path: Option<&str>,
        depth: Option<u32>,
    ) -> serde_json::Value {
        let mut params = serde_json::json!({ "session_id": sid });
        if let Some(p) = path {
            params["path"] = serde_json::json!(p);
        }
        if let Some(d) = depth {
            params["depth"] = serde_json::json!(d);
        }
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "file/list",
            "params": params,
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        serde_json::from_str(&resp).unwrap()
    }

    /// Query variant of `file_list_call` (the shared helper has no query param).
    async fn file_list_search_call(
        server: &IpcServer,
        sid: &str,
        path: Option<&str>,
        query: &str,
    ) -> serde_json::Value {
        let raw = serde_json::json!({
            "jsonrpc": "2.0", "id": 22, "method": "file/list",
            "params": { "session_id": sid, "path": path, "query": query },
        })
        .to_string();
        serde_json::from_str(&server.handle_request(&raw).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn dispatch_file_list_query_returns_ranked_matches_with_total() {
        let root = temp_dir_filelist("query");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "x").unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();
        let (server, sid) = file_list_server(&root);
        let v = file_list_search_call(&server, &sid, None, "main").await;
        assert!(v["error"].is_null(), "query call must succeed: {v}");
        assert_eq!(v["result"]["total"], 1);
        assert_eq!(v["result"]["entries"][0]["path"], "src/main.rs");
        let blank = file_list_search_call(&server, &sid, None, "   ").await;
        assert!(
            blank["result"].get("total").is_none(),
            "blank query = legacy listing"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn dispatch_file_list_lists_root_sorted_excluding_git() {
        let root = temp_dir_filelist("root");
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join("a.rs"), "hi").unwrap();
        let (server, sid) = file_list_server(&root);
        let v = file_list_call(&server, &sid, None, None).await;
        assert!(v["error"].is_null(), "valid call must succeed: {v}");
        let entries = v["result"]["entries"].as_array().unwrap();
        let names: Vec<&str> = entries
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["src", "a.rs"], "dirs first, .git excluded");
        assert_eq!(entries[0]["kind"], "dir");
        assert_eq!(entries[1]["kind"], "file");
        assert_eq!(entries[1]["size"], 2);
        assert_eq!(v["result"]["truncated"], false);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn dispatch_file_list_maps_errors_to_codes_and_data() {
        let root = temp_dir_filelist("errors");
        std::fs::write(root.join("file.txt"), "x").unwrap();
        let outside = temp_dir_filelist("errors-outside");
        let (server, sid) = file_list_server(&root);

        let v = file_list_call(&server, &sid, Some("../x"), None).await;
        assert_eq!(v["error"]["code"], -32807, "dotdot escape: {v}");
        assert_eq!(v["error"]["data"]["path"], "../x");
        assert_eq!(v["error"]["data"]["reason"], "outside_project_root");

        let abs = outside.to_string_lossy().into_owned();
        let v = file_list_call(&server, &sid, Some(&abs), None).await;
        assert_eq!(v["error"]["code"], -32807, "absolute escape: {v}");
        assert_eq!(v["error"]["data"]["path"], abs);

        let v = file_list_call(&server, &sid, Some("file.txt"), None).await;
        assert_eq!(v["error"]["code"], -32602, "file path: {v}");
        assert_eq!(v["error"]["data"]["path"], "file.txt");
        assert_eq!(v["error"]["data"]["reason"], "not_a_directory");

        let v = file_list_call(&server, &sid, Some("nope"), None).await;
        assert_eq!(v["error"]["code"], -32602, "missing path: {v}");
        assert_eq!(v["error"]["data"]["path"], "nope");
        assert_eq!(v["error"]["data"]["reason"], "not_found");

        let v = file_list_call(&server, &sid, None, Some(9)).await;
        assert_eq!(v["error"]["code"], -32602, "depth 9: {v}");
        assert_eq!(v["error"]["data"]["reason"], "invalid_depth");

        let v = file_list_call(&server, "missing-session", None, None).await;
        assert_eq!(v["error"]["code"], -32802, "unknown session: {v}");

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    /// Run a git command in `root`, panicking on any failure (git is available
    /// wherever the `git_sample` suite runs).
    fn git_run(root: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output();
        match out {
            Ok(o) if o.status.success() => {}
            Ok(o) => panic!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&o.stderr)
            ),
            Err(e) => panic!("git unavailable: {e}"),
        }
    }

    /// AC: a git repo with modified / untracked / deleted files maps to M/A/D.
    /// The deleted entry stays on disk via `git rm --cached` + `.gitignore`,
    /// otherwise it would not be reachable through a directory listing.
    #[tokio::test]
    async fn dispatch_file_list_reports_git_statuses() {
        let root = temp_dir_filelist("git");
        git_run(&root, &["init", "-q"]);
        std::fs::write(root.join("m.txt"), "1\n").unwrap();
        std::fs::write(root.join("d.txt"), "1\n").unwrap();
        git_run(&root, &["add", "m.txt", "d.txt"]);
        git_run(
            &root,
            &[
                "-c",
                "user.name=AgentJ",
                "-c",
                "user.email=agentj@example.com",
                "commit",
                "-qm",
                "init",
            ],
        );
        std::fs::write(root.join("m.txt"), "1\n2\n").unwrap();
        std::fs::write(root.join("a.txt"), "new\n").unwrap();
        git_run(&root, &["rm", "--cached", "-q", "d.txt"]);
        std::fs::write(root.join(".gitignore"), "d.txt\n").unwrap();
        let (server, sid) = file_list_server(&root);
        let v = file_list_call(&server, &sid, None, None).await;
        assert!(v["error"].is_null(), "call must succeed: {v}");
        let entry = |name: &str| -> serde_json::Value {
            v["result"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == name)
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        assert_eq!(entry("m.txt")["git_status"], "M", "modified: {v}");
        assert_eq!(entry("a.txt")["git_status"], "A", "untracked: {v}");
        assert_eq!(entry("d.txt")["git_status"], "D", "staged delete: {v}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// AC: a worktree deletion below `src` surfaces as the aggregate `D` on the
    /// `src` directory entry even though the deleted file is not listed.
    #[tokio::test]
    async fn dispatch_file_list_reports_aggregate_dir_status() {
        let root = temp_dir_filelist("git-dir-agg");
        git_run(&root, &["init", "-q"]);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join("src/deep/del.txt"), "1\n").unwrap();
        git_run(&root, &["add", "."]);
        git_run(
            &root,
            &[
                "-c",
                "user.name=AgentJ",
                "-c",
                "user.email=agentj@example.com",
                "commit",
                "-qm",
                "init",
            ],
        );
        std::fs::remove_file(root.join("src/deep/del.txt")).unwrap(); // worktree delete -> ' D'
        let (server, sid) = file_list_server(&root);
        let v = file_list_call(&server, &sid, None, None).await;
        assert!(v["error"].is_null(), "call must succeed: {v}");
        let entry = |name: &str| -> serde_json::Value {
            v["result"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == name)
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        assert_eq!(
            entry("src")["git_status"],
            "D",
            "deleted descendant surfaces on the dir: {v}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// Subdirectory listings keep entry paths root-relative so the git lookup
    /// (snapshot keys are root-relative) still reports M/A/D.
    #[tokio::test]
    async fn dispatch_file_list_subdirectory_git_status_is_root_relative() {
        let root = temp_dir_filelist("git-sub");
        git_run(&root, &["init", "-q"]);
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/m.txt"), "1\n").unwrap();
        git_run(&root, &["add", "src/m.txt"]);
        git_run(
            &root,
            &[
                "-c",
                "user.name=AgentJ",
                "-c",
                "user.email=agentj@example.com",
                "commit",
                "-qm",
                "init",
            ],
        );
        std::fs::write(root.join("src/m.txt"), "1\n2\n").unwrap();
        let (server, sid) = file_list_server(&root);
        let v = file_list_call(&server, &sid, Some("src"), None).await;
        assert!(v["error"].is_null(), "call must succeed: {v}");
        let entries = v["result"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "one file under src: {v}");
        assert_eq!(
            entries[0]["path"], "src/m.txt",
            "entry path must stay root-relative: {v}"
        );
        assert_eq!(
            entries[0]["git_status"], "M",
            "root-relative git lookup must hit: {v}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dispatch_file_list_unreadable_directory_is_internal_error() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_dir_filelist("unreadable");
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            // Privileged runner: permission bits are not enforced → unexercisable here.
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::remove_dir_all(&root).ok();
            return;
        }
        let (server, sid) = file_list_server(&root);
        let v = file_list_call(&server, &sid, Some("locked"), None).await;
        assert_eq!(v["error"]["code"], -32603, "unreadable dir: {v}");
        assert_eq!(v["error"]["data"]["path"], "locked");
        assert!(
            v["error"]["data"]["reason"]
                .as_str()
                .is_some_and(|r| !r.is_empty()),
            "io reason must be carried: {v}"
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).ok();
    }
}
