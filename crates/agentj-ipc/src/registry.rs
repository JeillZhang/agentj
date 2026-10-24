//! Connection abstraction and multi-client registry.
//!
//! Split out of `lib.rs`: the transport-agnostic `Connection` trait, its
//! send-only `ConnectionWriter` handle, and the `ConnectionRegistry` that
//! routes messages, broadcasts and session ownership. Everything here is a
//! verbatim move: signatures, visibility and routing semantics are unchanged.

use crate::TransportKind;
use agentj_core::error::IpcError;
use agentj_core::types::SessionId;
use async_trait::async_trait;
use std::sync::Arc;

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
    pub(crate) conns: std::collections::HashMap<String, Box<dyn Connection>>,
    /// Send-only handles for connections whose serve loop owns the reader half
    /// (TCP). Registered for the connection's lifetime so broadcasts reach idle
    /// subscribers; removed on disconnect.
    pub(crate) writers: std::collections::HashMap<String, ConnectionWriter>,
    session_owner: std::collections::HashMap<SessionId, String>,
    pub(crate) pending_runs: std::collections::HashSet<SessionId>,
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
    /// Owner of a session, or `None` when nobody claimed it. Takes `&str` so the
    /// lookup borrows the caller's id instead of allocating a `SessionId`.
    pub fn session_owner(&self, session: &str) -> Option<&String> {
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
