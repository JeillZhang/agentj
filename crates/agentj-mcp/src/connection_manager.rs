//! Long-lived MCP connection management (design §MCP 连接生命周期, decision #11).
//! Ref-counted sharing, idle-timeout reclamation, exponential-backoff reconnect,
//! cascade ghost-tool unregistration, graceful Closing→Closed.
//!
//! Transport threading (T8 finding): tokio::process pipes are runtime-affine
//! (ChildStdio = PollEvented, bound to the runtime that spawned the child), so a
//! connection's transport must be driven from ONE runtime for its whole life. Each
//! connection therefore owns a dedicated transport thread with its own current-thread
//! runtime; all callers (kernel runtime, AgentLoop spawn_blocking, or the thread
//! spawned by McpSharedTool::execute) send ops over a channel and block on the reply.

use crate::transport::McpTransport;
use agentj_core::error::ToolError;
use agentj_core::hooks::EventBus;
use agentj_core::module::{EventType, Tool, ToolRegistry, ToolSource};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Outer timeout for channel ops; the transport's own request timeout is 30s.
const OP_TIMEOUT: Duration = Duration::from_secs(35);

/// Long-lived MCP connection state (design §MCP 连接生命周期).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Ready,
    Disconnected,
    Reconnecting,
    Closing,
    Closed,
    Failed,
}

pub struct McpConnection {
    command: String,
    state: Mutex<ConnectionState>,
    refs: AtomicUsize,
    idle_since: Mutex<Option<Instant>>,
    transport: RwLock<Option<Arc<TransportHandle>>>,
    registered_tools: Mutex<Vec<String>>,
}

type Reply<T> = Sender<Result<T, String>>;

enum TransportOp {
    Connect {
        command: String,
        reply: Reply<()>,
    },
    Request {
        method: String,
        params: Value,
        reply: Reply<Value>,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

/// Handle to a connection's dedicated transport thread. Every method blocks on
/// the reply with a timeout; errors surface as `String` reasons.
struct TransportHandle {
    tx: Sender<TransportOp>,
    _thread: std::thread::JoinHandle<()>,
}

impl TransportHandle {
    fn spawn() -> Result<Self, String> {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("mcp-transport".into())
            .spawn(move || transport_loop(rx))
            .map_err(|e| format!("transport thread spawn failed: {e}"))?;
        Ok(Self {
            tx,
            _thread: thread,
        })
    }

    fn connect(&self, command: String) -> Result<(), String> {
        let (reply_tx, reply_rx) = channel();
        self.tx
            .send(TransportOp::Connect {
                command,
                reply: reply_tx,
            })
            .map_err(|e| e.to_string())?;
        reply_rx
            .recv_timeout(OP_TIMEOUT)
            .map_err(|_| "connect timed out".to_string())?
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let (reply_tx, reply_rx) = channel();
        self.tx
            .send(TransportOp::Request {
                method: method.into(),
                params,
                reply: reply_tx,
            })
            .map_err(|e| e.to_string())?;
        reply_rx
            .recv_timeout(OP_TIMEOUT)
            .map_err(|_| "request timed out".to_string())?
    }

    /// Graceful close: MCP shutdown notification, brief exit confirmation, then
    /// the thread ends (dropping the transport → kill_on_drop force-kill fallback).
    fn shutdown(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = channel();
        self.tx
            .send(TransportOp::Shutdown { reply: reply_tx })
            .map_err(|e| e.to_string())?;
        reply_rx
            .recv_timeout(OP_TIMEOUT)
            .map_err(|_| "shutdown timed out".to_string())?
    }
}

fn reply_error(op: TransportOp, msg: &str) {
    match op {
        TransportOp::Connect { reply, .. } => {
            let _ = reply.send(Err(msg.to_string()));
        }
        TransportOp::Request { reply, .. } => {
            let _ = reply.send(Err(msg.to_string()));
        }
        TransportOp::Shutdown { reply, .. } => {
            let _ = reply.send(Err(msg.to_string()));
        }
    }
}

fn transport_loop(rx: Receiver<TransportOp>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let msg = format!("transport runtime build failed: {e}");
            while let Ok(op) = rx.recv() {
                reply_error(op, &msg);
            }
            return;
        }
    };
    let mut transport: Option<McpTransport> = None;
    while let Ok(op) = rx.recv() {
        match op {
            TransportOp::Connect { command, reply } => {
                let result = rt.block_on(spawn_and_init(&command));
                match result {
                    Ok(t) => {
                        transport = Some(t);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            TransportOp::Request {
                method,
                params,
                reply,
            } => {
                let result = match &mut transport {
                    Some(t) => rt
                        .block_on(t.request(&method, params))
                        .map_err(|e| e.to_string()),
                    None => Err("transport not connected".to_string()),
                };
                let _ = reply.send(result);
            }
            TransportOp::Shutdown { reply } => {
                if let Some(t) = transport.as_mut() {
                    rt.block_on(graceful_shutdown(t));
                }
                let _ = reply.send(Ok(()));
                return;
            }
        }
    }
}

/// Spawn the server subprocess and run the initialize handshake. Must run on the
/// transport thread's runtime so the process pipes bind to that runtime.
async fn spawn_and_init(command: &str) -> Result<McpTransport, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or_else(|| "no stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
    let mut transport = McpTransport::new(stdin, stdout, child);
    transport
        .request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "agentj", "version": "0.2" },
            }),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(transport)
}

async fn graceful_shutdown(t: &mut McpTransport) {
    let _ = t.request("shutdown", serde_json::json!({})).await;
    let _ = t.wait_exit(Duration::from_millis(500)).await;
}

/// Owns all long-lived MCP connections: ref-counted sharing, idle reclamation,
/// exponential-backoff reconnect, cascade ghost-tool unregistration (decision #11).
pub struct McpConnectionManager {
    registry: Arc<ToolRegistry>,
    connections: RwLock<HashMap<String, Arc<McpConnection>>>,
    idle_timeout: Duration,
    reconnect_base: Duration,
    max_reconnect: u32,
    events: Arc<EventBus>,
}

impl McpConnectionManager {
    pub fn new(
        registry: Arc<ToolRegistry>,
        idle_timeout: Duration,
        reconnect_base: Duration,
        max_reconnect: u32,
        events: Arc<EventBus>,
    ) -> Self {
        Self {
            registry,
            connections: RwLock::new(HashMap::new()),
            idle_timeout,
            reconnect_base,
            max_reconnect,
            events,
        }
    }

    pub fn state(&self, server_id: &str) -> Option<ConnectionState> {
        let guard = self.connections.read().ok()?;
        let conn = guard.get(server_id)?;
        conn.state.lock().ok().map(|s| *s)
    }

    pub fn acquire(&self, server_id: &str) -> Result<Arc<McpConnection>, ToolError> {
        let conn = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
            .ok_or_else(|| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: "not connected".into(),
            })?;
        let ready = conn
            .state
            .lock()
            .ok()
            .map(|s| matches!(*s, ConnectionState::Ready))
            .unwrap_or(false);
        if !ready {
            return Err(ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: "not ready".into(),
            });
        }
        conn.refs.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut idle) = conn.idle_since.lock() {
            *idle = None;
        }
        Ok(conn)
    }

    pub fn release(&self, server_id: &str) {
        if let Some(conn) = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
        {
            let remaining = conn.refs.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
            if remaining == 0
                && let Ok(mut idle) = conn.idle_since.lock()
            {
                *idle = Some(Instant::now());
            }
        }
    }

    /// Idle reclamation: close connections idle beyond the timeout (called by a
    /// periodic background task in the CLI wiring, T11).
    pub async fn reap_idle(&self) {
        let idle: Vec<String> = self
            .connections
            .read()
            .ok()
            .map(|g| {
                g.iter()
                    .filter(|(_, c)| {
                        c.refs.load(Ordering::SeqCst) == 0
                            && c.idle_since
                                .lock()
                                .ok()
                                .map(|t| matches!(*t, Some(i) if i.elapsed() >= self.idle_timeout))
                                .unwrap_or(false)
                    })
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for id in idle {
            self.close(&id).await;
        }
    }

    // `self: &Arc<Self>` so registered tools can hold an Arc clone of the manager
    // for delegated execution (see McpSharedTool) — callers already hold Arc.
    pub async fn connect(
        self: &Arc<Self>,
        server_id: String,
        command: String,
    ) -> Result<(), ToolError> {
        self.publish(&EventType::McpConnecting(server_id.clone()));
        let conn = match self.handshake(&server_id, &command).await {
            Ok(conn) => conn,
            Err(e) => {
                self.fail(&server_id, &e.to_string()).await; // Failed tombstone
                return Err(e);
            }
        };
        if let Ok(mut map) = self.connections.write() {
            map.insert(server_id.clone(), Arc::new(conn));
        }
        if let Err(e) = self.register_tools(&server_id, &command).await {
            self.fail(&server_id, &e.to_string()).await;
            return Err(e);
        }
        if let Some(c) = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(&server_id).cloned())
            && let Ok(mut s) = c.state.lock()
        {
            *s = ConnectionState::Ready;
        }
        self.publish(&EventType::McpConnected(server_id.clone()));
        Ok(())
    }

    async fn handshake(&self, server_id: &str, command: &str) -> Result<McpConnection, ToolError> {
        let handle =
            Arc::new(
                TransportHandle::spawn().map_err(|e| ToolError::ServerUnavailable {
                    server_id: server_id.into(),
                    reason: e,
                })?,
            );
        let cmd = command.to_string();
        let connect_handle = Arc::clone(&handle);
        tokio::task::spawn_blocking(move || connect_handle.connect(cmd))
            .await
            .map_err(|e| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: e.to_string(),
            })?
            .map_err(|e| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: e,
            })?;
        Ok(McpConnection {
            command: command.into(),
            state: Mutex::new(ConnectionState::Connected),
            refs: AtomicUsize::new(1),
            idle_since: Mutex::new(None),
            transport: RwLock::new(Some(handle)),
            registered_tools: Mutex::new(vec![]),
        })
    }

    async fn register_tools(
        self: &Arc<Self>,
        server_id: &str,
        _command: &str,
    ) -> Result<(), ToolError> {
        let handle = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
            .and_then(|c| c.transport.read().ok().and_then(|g| g.clone()))
            .ok_or_else(|| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: "not connected".into(),
            })?;
        let tools_resp = tokio::task::spawn_blocking(move || {
            handle.request("tools/list", serde_json::json!({}))
        })
        .await
        .map_err(|e| ToolError::ServerUnavailable {
            server_id: server_id.into(),
            reason: e.to_string(),
        })?
        .map_err(|e| ToolError::ServerUnavailable {
            server_id: server_id.into(),
            reason: e,
        })?;
        let tools = tools_resp["tools"].as_array().cloned().unwrap_or_default();
        let mut names = vec![];
        for def in tools {
            let name = def["name"].as_str().unwrap_or("unknown").to_string();
            names.push(name.clone());
            let tool = McpSharedTool {
                server_id: server_id.into(),
                name: name.clone(),
                description: def["description"].as_str().unwrap_or("").to_string(),
                input_schema: def["inputSchema"].clone(),
                manager: Arc::clone(self), // registered tools delegate execution to the manager
            };
            let _ = self.registry.register_tool_with_source(
                Arc::new(tool),
                ToolSource::Mcp {
                    server_id: server_id.into(),
                },
                agentj_core::module::ReplacePolicy::Replace,
            );
        }
        if let Some(c) = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
            && let Ok(mut reg) = c.registered_tools.lock()
        {
            *reg = names;
        }
        Ok(())
    }

    pub async fn execute(
        self: &Arc<Self>,
        server_id: &str,
        tool: &str,
        input: Value,
    ) -> Result<Value, ToolError> {
        let conn = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
            .ok_or_else(|| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: "not connected".into(),
            })?;
        let handle = conn
            .transport
            .read()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: "transport closed".into(),
            })?;
        let params = serde_json::json!({ "name": tool, "arguments": input });
        let resp = tokio::task::spawn_blocking(move || handle.request("tools/call", params))
            .await
            .map_err(|e| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: e.to_string(),
            })?
            .map_err(|e| ToolError::ServerUnavailable {
                server_id: server_id.into(),
                reason: e,
            });
        match resp {
            Ok(r) => Ok(r["content"][0]["text"].clone()),
            Err(e) => {
                // mid-call crash → unregister ghosts, transition to Reconnecting
                self.registry.unregister_by_source(&ToolSource::Mcp {
                    server_id: server_id.into(),
                });
                self.publish(&EventType::McpDisconnected(server_id.into()));
                if let Ok(mut map) = self.connections.write() {
                    map.remove(server_id);
                }
                self.reconnect(server_id, &conn.command).await;
                Err(e)
            }
        }
    }

    async fn reconnect(self: &Arc<Self>, server_id: &str, command: &str) {
        self.publish(&EventType::McpReconnecting(server_id.into()));
        let mut delay = self.reconnect_base;
        for _ in 0..self.max_reconnect {
            tokio::time::sleep(delay).await;
            if let Ok(conn) = self.handshake(server_id, command).await {
                if let Ok(mut map) = self.connections.write() {
                    map.insert(server_id.into(), Arc::new(conn));
                }
                // tool diff (design M3): start from a clean slate so a tool that
                // disappeared from the new tools/list is not left registered
                self.registry.unregister_by_source(&ToolSource::Mcp {
                    server_id: server_id.into(),
                });
                if self.register_tools(server_id, command).await.is_ok() {
                    if let Some(c) = self
                        .connections
                        .read()
                        .ok()
                        .and_then(|g| g.get(server_id).cloned())
                        && let Ok(mut s) = c.state.lock()
                    {
                        *s = ConnectionState::Ready;
                    }
                    self.publish(&EventType::McpConnected(server_id.into()));
                    return;
                }
                // tool registration failed → drop this connection, retry
                if let Ok(mut map) = self.connections.write() {
                    map.remove(server_id);
                }
            }
            delay = (delay * 2).min(Duration::from_secs(30)); // 500ms ×2, cap 30s
        }
        self.fail(server_id, "max reconnect attempts exceeded")
            .await;
    }

    async fn close(&self, server_id: &str) {
        if let Some(conn) = self
            .connections
            .read()
            .ok()
            .and_then(|g| g.get(server_id).cloned())
        {
            if let Ok(mut s) = conn.state.lock() {
                *s = ConnectionState::Closing;
            }
            if let Some(handle) = conn.transport.read().ok().and_then(|g| g.clone()) {
                // graceful: MCP shutdown notification + brief exit confirmation on
                // the transport thread; kill_on_drop is the force-kill fallback
                let _ = handle.shutdown();
            }
            if let Ok(mut t) = conn.transport.write() {
                *t = None;
            }
            if let Ok(mut s) = conn.state.lock() {
                *s = ConnectionState::Closed;
            }
        }
        self.registry.unregister_by_source(&ToolSource::Mcp {
            server_id: server_id.into(),
        });
        self.publish(&EventType::McpClosed(server_id.into()));
    }

    async fn fail(&self, server_id: &str, reason: &str) {
        let exists = self
            .connections
            .read()
            .ok()
            .is_some_and(|g| g.contains_key(server_id));
        if exists
            && let Some(c) = self
                .connections
                .read()
                .ok()
                .and_then(|g| g.get(server_id).cloned())
            && let Ok(mut s) = c.state.lock()
        {
            *s = ConnectionState::Failed;
        } else if !exists {
            self.insert_failed(server_id).await;
        }
        self.registry.unregister_by_source(&ToolSource::Mcp {
            server_id: server_id.into(),
        });
        self.publish(&EventType::McpDisconnected(server_id.into()));
        tracing::warn!(server_id, reason, "MCP connection failed");
    }

    /// Leave a Failed tombstone so state() is observable after a failed connect/reconnect.
    async fn insert_failed(&self, server_id: &str) {
        let conn = McpConnection {
            command: String::new(),
            state: Mutex::new(ConnectionState::Failed),
            refs: AtomicUsize::new(0),
            idle_since: Mutex::new(None),
            transport: RwLock::new(None),
            registered_tools: Mutex::new(vec![]),
        };
        if let Ok(mut map) = self.connections.write() {
            map.insert(server_id.into(), Arc::new(conn));
        }
    }

    fn publish(&self, event: &EventType) {
        self.events.publish(event);
    }
}

/// A tool backed by a long-lived connection (registered with ToolSource::Mcp).
/// Execution DELEGATES to McpConnectionManager::execute so the reconnect/unavailable
/// semantics are reachable from the agent path (AgentLoop: ctx.tools.get + execute).
struct McpSharedTool {
    server_id: String,
    name: String,
    description: String,
    input_schema: Value,
    manager: Arc<McpConnectionManager>,
}

impl Tool for McpSharedTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }
    fn execute(&self, input: Value) -> Result<Value, ToolError> {
        let manager = Arc::clone(&self.manager);
        let server_id = self.server_id.clone();
        let name = self.name.clone();
        // Tool::execute is sync; drive the async manager call on a thread with its own
        // runtime (block_on inside an existing runtime panics). Transport IO itself
        // runs on the connection's dedicated transport thread, so this fresh runtime
        // only drives the channel-based manager call — no cross-runtime pipe usage.
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| ToolError::ExecutionFailed {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
            rt.block_on(manager.execute(&server_id, &name, input))
        });
        handle.join().map_err(|_| ToolError::ExecutionFailed {
            name: self.name.clone(),
            reason: "MCP call thread panicked".into(),
        })?
    }
}

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
        assert_eq!(
            tools[0].source,
            ToolSource::Mcp {
                server_id: "srv1".into()
            }
        );
    }

    #[tokio::test]
    async fn execute_round_trips_tool_call() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        let out = mgr
            .execute("srv1", "echo", serde_json::json!({"text": "hello"}))
            .await
            .unwrap();
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
        let crash = format!(
            "{}/tests/fixtures/mcp_crash_after_2.sh",
            env!("CARGO_MANIFEST_DIR")
        );
        mgr.connect("srv1".into(), crash).await.unwrap();
        // the fixture serves exactly 2 tool calls, then exits; calls 1-2 succeed, call 3
        // hits a dead server → ServerUnavailable → reconnect (fresh process) → tool works again
        assert!(
            mgr.execute("srv1", "echo", serde_json::json!({"text": "a"}))
                .await
                .is_ok()
        );
        assert!(
            mgr.execute("srv1", "echo", serde_json::json!({"text": "b"}))
                .await
                .is_ok()
        );
        let result = mgr
            .execute("srv1", "echo", serde_json::json!({"text": "c"}))
            .await;
        assert!(
            matches!(result, Err(ToolError::ServerUnavailable { server_id, .. }) if server_id == "srv1")
        );
        let out = mgr
            .execute("srv1", "echo", serde_json::json!({"text": "d"}))
            .await;
        assert!(out.is_ok(), "tool must work after reconnect, got: {out:?}");
    }

    #[tokio::test]
    async fn release_to_zero_with_idle_timeout_closes_and_unregisters() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_millis(50));
        mgr.connect("srv1".into(), echo_script()).await.unwrap();
        assert_eq!(registry.tools().len(), 1);
        mgr.acquire("srv1").unwrap(); // refcount 2
        mgr.release("srv1"); // 1
        mgr.release("srv1"); // 0 → idle timer starts
        tokio::time::sleep(Duration::from_millis(150)).await;
        mgr.reap_idle().await;
        assert_eq!(mgr.state("srv1"), Some(ConnectionState::Closed));
        assert!(
            registry.tools().is_empty(),
            "ghost tools must be unregistered on close"
        );
    }

    #[tokio::test]
    async fn reconnect_exhaustion_lands_in_failed() {
        let registry = Arc::new(ToolRegistry::new());
        let mgr = manager(registry.clone(), Duration::from_secs(60));
        // /nonexistent script → every reconnect attempt fails; after max_reconnect → Failed
        mgr.connect("srv1".into(), "/nonexistent/mcp_server.sh".into())
            .await
            .unwrap_err();
        assert_eq!(mgr.state("srv1"), Some(ConnectionState::Failed));
    }
}
