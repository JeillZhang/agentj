//! Unified Connection abstraction (design 决策 #1/#2) — transport-agnostic serve.
pub(crate) mod client_ip;
mod dispatch;
mod frames;
pub(crate) use frames::{attach_git_statuses, build_progress_frame, todo_items_into_wire};
pub mod file_list;
mod handlers;
pub(crate) mod stdio;

pub(crate) use handlers::provider::kernel_model_info;
pub(crate) mod login_limiter;
pub(crate) mod metrics_state;
pub mod secure;
pub mod tcp;
pub mod trajectory;
pub mod web;
pub mod ws;

use agentj_core::artifacts::{FileChangeArtifact, FileChangeOp};
use agentj_core::error::{IpcError, KernelError, ModuleError};
use agentj_core::kernel::Kernel;
use agentj_core::module::{KernelMessage, SessionStore};
#[cfg(test)]
use agentj_core::sync::{lock_or_recover, read_or_recover, write_or_recover};
use agentj_core::types::{AgentEvent, RunOverrides, SessionId};
use agentj_permission::PermissionModule;
use agentj_protocol::base::error_codes;
use agentj_protocol::permission::PermissionRequestParams;
use agentj_protocol::provider::{
    AuthClearCredentialParams, AuthClearCredentialResult, AuthDevicePollParams,
    AuthDevicePollResult, AuthDeviceStartParams, AuthDeviceStartResult, AuthSetApiKeyParams,
    AuthSetApiKeyResult, AuthStatusParams, AuthStatusResult, ModelListParams, ModelListResult,
    ProviderUsageParams, ProviderUsageResult,
};
use agentj_question::QuestionModule;
use async_trait::async_trait;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Forget the sessions a vanished connection owned. Called when the connection
    /// ends and *before* in-flight runs are drained: an approval issued later in the
    /// drain must be denied (no reachable owner) instead of being routed to a writer
    /// that is already gone, which would only surface as the approval timing out.
    pub fn clear_owner(&mut self, conn_id: &str) {
        self.session_owner.retain(|_, owner| owner != conn_id);
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

/// Optional string param for `session/run` (design 错误处理): a missing key or
/// JSON `null` means "absent"; a string is trimmed and a blank result also
/// means "absent" (same semantics as `file/list {query}`); any other JSON type
/// is `-32602` with the offending field name in `error.data`.
fn optional_trimmed(params: &Value, key: &str) -> Result<Option<String>, DispatchError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
        }
        Some(_) => Err(DispatchError::invalid_params_with_data(
            format!("{key} must be a string"),
            serde_json::json!({ "field": key }),
        )),
    }
}

/// Serialize a protocol value into a response body.
///
/// serde on our own wire types practically cannot fail, but a `panic!` inside a
/// request would drop the connection, so the impossible case becomes `-32603`.
fn to_wire<T: serde::Serialize>(value: T) -> Result<Value, DispatchError> {
    serde_json::to_value(value)
        .map_err(|e| DispatchError::internal(format!("response serialization failed: {e}")))
}

/// Method-dispatch failure carrying the JSON-RPC 2.0 standard error code
/// (design 决策 #4). `handle_request` echoes the request `id` alongside this
/// code so every error path round-trips the correlation id. `data` carries
/// parameterized error details (`error.data`, design 决策 #5) and stays off
/// the wire when None.
#[derive(Debug, Serialize)]
pub struct DispatchError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl DispatchError {
    fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }
    pub fn invalid_params_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: Some(data),
        }
    }

    /// Uninjected handle: the seven subscription methods report this (`design D4`).
    pub(crate) fn provider_service_unavailable() -> Self {
        Self::internal_with_data(
            "ProviderServiceUnavailable",
            serde_json::json!({ "reason": "provider_service_unavailable" }),
        )
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
    pub fn internal_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: Some(data),
        }
    }
}

/// Run a dispatch future behind a panic guard.
///
/// A panic in a dispatch method is a kernel bug, not a client error, but the
/// client must still get a well-formed response for its request id (JSON-RPC
/// 2.0 §5) and the serve loop must survive. The panic text is truncated and
/// carried in `error.data` so the bug can be reported without reproducing it.
async fn isolate_panic<F>(method: &str, fut: F) -> Result<Value, DispatchError>
where
    F: std::future::Future<Output = Result<Value, DispatchError>>,
{
    match std::panic::AssertUnwindSafe(fut).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            let reason = agentj_core::sync::panic_payload_message(payload.as_ref());
            tracing::error!(%reason, %method, "request dispatch panicked");
            Err(DispatchError::internal_with_data(
                "InternalPanic",
                serde_json::json!({ "method": method, "reason": reason }),
            ))
        }
    }
}

/// Subscription/service handle injected by the CLI assembly (`design D4`).
///
/// Defined here (returning `agentj-protocol` types) so `agentj-ipc` gains no dependency on
/// `agentj-auth` / `agentj-model`; the implementation lives in `agentj-cli`.
#[async_trait]
pub trait ProviderService: Send + Sync {
    async fn model_list(&self, params: ModelListParams) -> Result<ModelListResult, DispatchError>;
    async fn auth_status(
        &self,
        params: AuthStatusParams,
    ) -> Result<AuthStatusResult, DispatchError>;
    async fn usage(
        &self,
        params: ProviderUsageParams,
    ) -> Result<ProviderUsageResult, DispatchError>;
    async fn device_start(
        &self,
        params: AuthDeviceStartParams,
    ) -> Result<AuthDeviceStartResult, DispatchError>;
    async fn device_poll(
        &self,
        params: AuthDevicePollParams,
    ) -> Result<AuthDevicePollResult, DispatchError>;
    async fn set_api_key(
        &self,
        params: AuthSetApiKeyParams,
    ) -> Result<AuthSetApiKeyResult, DispatchError>;
    async fn clear_credential(
        &self,
        params: AuthClearCredentialParams,
    ) -> Result<AuthClearCredentialResult, DispatchError>;
}

/// The client-callable methods routed by `IpcServer::dispatch_request`, in the
/// same order as the route table. Pinned against `protocol/methods.json` by
/// `dispatch_table_matches_methods_json`; `initialized` (notification) and
/// `exit` (transport close) are deliberately absent — neither is dispatched.
pub(crate) const DISPATCH_METHODS: &[&str] = &[
    "initialize",
    "session/new",
    "session/run",
    "session/subscribe",
    "session/unsubscribe",
    "kernel/status",
    "session/list",
    "session/history",
    "session/status",
    "session/archive",
    "session/delete",
    "file/list",
    "model/list",
    "auth/status",
    "provider/usage",
    "auth/deviceStart",
    "auth/devicePoll",
    "auth/setApiKey",
    "auth/clearCredential",
    "shutdown",
    "permission/respond",
    "question/respond",
    "$/cancelRequest",
];

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
    /// Optional ProviderService: the seven subscription methods dispatch into it (`design D4`).
    pub provider_service: Option<Arc<dyn ProviderService>>,
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

/// Reason attached to approvals denied because their owning connection went
/// away. Mirrors the `cancelled` reason that `$/cancelRequest` uses.
pub(crate) const DISCONNECTED_REASON: &str = "disconnected";

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
            provider_service: None,
        }
    }

    pub fn create_session(&self) -> Result<SessionId, ModuleError> {
        self.session_store.create()
    }

    pub fn run_turn_stream(&self, sid: &str, input: &str) -> mpsc::Receiver<AgentEvent> {
        self.run_turn_stream_with(sid, input, RunOverrides::default())
    }

    /// Turn entry carrying per-run overrides. `session/run` dispatch and the CLI `run`
    /// path both use it; `RunOverrides::default()` reproduces the pre-0.26 behavior.
    pub fn run_turn_stream_with(
        &self,
        sid: &str,
        input: &str,
        overrides: RunOverrides,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(64);
        let sid = sid.to_string();
        let content = input.to_string();
        if self
            .kernel
            .sender()
            .send(KernelMessage::user_request_with_overrides(
                sid, content, overrides, tx,
            ))
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

    /// Serve one connection to its end: pump requests, then settle whatever the
    /// vanished client can no longer answer and drain the runs it left behind.
    ///
    /// Both settling and dropping ownership happen *before* the drain, so a run
    /// blocked on an approval — or one that asks for a second approval later in the
    /// drain — is denied at once instead of waiting for the approval timeout and
    /// keeping the session busy for a reconnecting client.
    async fn serve_connection_requests(
        self: Arc<Self>,
        mut conn: Box<dyn Connection>,
        conn_id: &str,
    ) -> Result<(), IpcError> {
        let conn_id_owned = conn_id.to_string();
        let mut pending = tokio::task::JoinSet::new();
        // Borrow the connection: it must outlive the drain below. Dropping it ends the
        // transport's write path (e.g. tokio's `OwnedWriteHalf` shuts the socket's write
        // direction down on drop), so a response produced by a request spawned just before
        // the disconnect would be lost — the peer would see EOF instead of its answer.
        let outcome = self
            .pump_requests(&mut pending, &mut conn, &conn_id_owned)
            .await;
        self.settle_owner_pending(&conn_id_owned);
        while pending.join_next().await.is_some() {}
        outcome
    }

    /// Per-connection request loop. The connection stays owned by this loop for its
    /// whole lifetime (reader half for `recv`), while the registry holds a detached
    /// `ConnectionWriter` handle — so an idle subscriber never drops out of broadcast
    /// reach, and no std RwLock guard is held across an await. Each request is spawned
    /// into its own task so a long-running `session/run` does not block
    /// `permission/respond` or `$/cancelRequest` on the same connection.
    ///
    /// Returns when the peer goes away (EOF or transport error) or a shutdown is
    /// requested; the caller settles the departing owner's pending work and drains.
    async fn pump_requests(
        self: &Arc<Self>,
        pending: &mut tokio::task::JoinSet<()>,
        conn: &mut Box<dyn Connection>,
        conn_id: &str,
    ) -> Result<(), IpcError> {
        loop {
            let Some(line) = conn.recv().await else {
                return Ok(());
            };
            let trimmed = line?.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let this = Arc::clone(self);
            let registry = Arc::clone(&self.registry);
            let cid = conn_id.to_string();
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
                return Ok(());
            }
        }
    }

    /// Route $/permissionRequest notifications to the session owner through
    /// the registry (决策 #5). stdio needs no special case: its sentinel
    /// connection is the session owner and `send_to` writes to it. A request
    /// with no reachable owner can never be answered, so it is denied at once
    /// instead of waiting out the approval timeout. Attach once per transport
    /// entry point.
    pub(crate) fn attach_permission_notifier(&self) {
        if let Some(module) = &self.permission_module {
            let registry = Arc::clone(&self.registry);
            let permission = Arc::clone(module);
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
                if owner.is_none() {
                    // 设计决策 #5: only the owner connection may answer, so with no
                    // owner at all the request can never be resolved — deny it now
                    // instead of letting the run wait out the whole timeout.
                    permission.cancel_session(session_id, DISCONNECTED_REASON);
                } else if let Some(owner_id) = owner
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

#[cfg(test)]
pub(crate) use tests::test_support;
#[cfg(test)]
mod tests;
