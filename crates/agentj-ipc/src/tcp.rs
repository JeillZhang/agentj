//! TCP transport — multi-client JSON-RPC serving over TcpListener (design 决策 #1/#3).

use crate::{Connection, ConnectionWriter, IpcServer, TransportKind};
use agentj_core::error::IpcError;
use agentj_core::kernel::Kernel;
use agentj_core::module::SessionStore;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

fn looks_like_http(peeked: &[u8]) -> bool {
    matches!(peeked.first(), Some(b) if b.is_ascii_uppercase())
}

/// TCP connection — implements the v0.1 Connection trait (决策 #1): sync send
/// through a cloned blocking socket, async recv on the tokio read half.
pub struct TcpConnection {
    id: String,
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    /// Kept alive for the connection's lifetime: dropping the tokio write half
    /// shuts down the socket's write direction (FIN) and kills the response path.
    _writer_half: tokio::net::tcp::OwnedWriteHalf,
    /// Shared blocking writer — `ConnectionWriter` clones this Arc so broadcasts
    /// can reach the peer while the serve loop owns the reader half.
    writer: Arc<Mutex<std::net::TcpStream>>,
    peer: String,
}

/// Blocking NDJSON write through the shared socket (message + newline + flush).
fn write_tcp(writer: &Mutex<std::net::TcpStream>, msg: &str) -> Result<(), IpcError> {
    use std::io::Write;
    let mut w = writer
        .lock()
        .map_err(|_| IpcError::InvalidRequest("tcp writer lock poisoned".into()))?;
    w.write_all(msg.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

impl TcpConnection {
    pub fn new(stream: TcpStream) -> Result<Self, IpcError> {
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| "unknown".into());
        let std_stream = stream
            .into_std()
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        let writer = std_stream
            .try_clone()
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        // into_std() hands back a BLOCKING socket; from_std rejects those —
        // restore non-blocking before re-registering with the tokio reactor.
        std_stream
            .set_nonblocking(true)
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        let tokio_stream = tokio::net::TcpStream::from_std(std_stream)
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        let (reader, writer_half) = tokio_stream.into_split();
        Ok(Self {
            id: format!("tcp-{peer}"),
            reader: BufReader::new(reader),
            _writer_half: writer_half,
            writer: Arc::new(Mutex::new(writer)),
            peer,
        })
    }
}

#[async_trait]
impl Connection for TcpConnection {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        write_tcp(&self.writer, msg)
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None,
            Ok(_) => Some(Ok(line)),
            Err(e) => Some(Err(IpcError::InvalidRequest(e.to_string()))),
        }
    }
    fn peer_desc(&self) -> String {
        self.peer.clone()
    }
    fn writer(&self) -> ConnectionWriter {
        let writer = Arc::clone(&self.writer);
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer.clone(),
            Arc::new(move |msg: &str| write_tcp(&writer, msg)),
        )
    }
}

/// Bundled `TcpIpcServer::bind` options: token + max_connections + the
/// trajectory store keep the signature within the argument-count gate.
#[derive(Clone)]
pub struct TcpBindOptions {
    pub token: Option<String>,
    pub max_connections: usize,
    pub trajectory_store: Option<Arc<crate::trajectory::TrajectoryStore>>,
    pub web_dist: Option<std::path::PathBuf>,
    /// Trust `X-Forwarded-For` / `X-Real-IP` when the direct peer is loopback.
    pub trust_proxy_headers: bool,
    /// Mark issued session cookies `Secure` (TLS terminates in front of the kernel).
    pub secure_cookies: bool,
    /// Directory for the persisted login-limiter state; `None` keeps it in memory.
    pub metrics_state_dir: Option<std::path::PathBuf>,
    /// Optional ProviderService handle (`design D4`), copied onto the shared server at bind.
    pub provider_service: Option<Arc<dyn crate::ProviderService>>,
    /// `[web]` spot cadence + per-request verification knobs; `Default` keeps today's
    /// 60s / 8 / on / 60s behaviour.
    pub integrity: crate::web::IntegrityOptions,
}

impl Default for TcpBindOptions {
    fn default() -> Self {
        Self {
            token: None,
            max_connections: 16,
            trajectory_store: None,
            web_dist: None,
            trust_proxy_headers: false,
            secure_cookies: false,
            metrics_state_dir: None,
            provider_service: None,
            integrity: crate::web::IntegrityOptions::default(),
        }
    }
}

/// Multi-client JSON-RPC over TCP — ONE shared IpcServer serves every connection.
pub struct TcpIpcServer {
    listener: TcpListener,
    shared: Arc<IpcServer>,
    max_connections: usize,
    active: Arc<AtomicUsize>,
    ws_state: Arc<crate::ws::WsServerState>,
}

impl TcpIpcServer {
    pub async fn bind(
        addr: &str,
        kernel: Kernel,
        session: Arc<dyn SessionStore>,
        permission_module: Option<Arc<agentj_permission::PermissionModule>>,
        question_module: Option<Arc<agentj_question::QuestionModule>>,
        options: TcpBindOptions,
    ) -> Result<Self, IpcError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        let mut server = IpcServer::new(kernel, session);
        server.permission_module = permission_module;
        server.question_module = question_module;
        server.trajectory_store = options.trajectory_store.clone();
        server.provider_service = options.provider_service.clone();
        server.attach_permission_notifier();
        server.attach_question_notifier();
        server.set_token(options.token.clone());
        let shared = Arc::new(server);
        let state_path = options.metrics_state_dir.as_deref().map(|dir| {
            // The port is only known after the listener exists (--port 0), so the
            // state file is port-scoped here and not in the CLI (design D4).
            let port = listener
                .local_addr()
                .map(|addr| addr.port())
                .unwrap_or_default();
            crate::metrics_state::state_path_in(dir, port)
        });
        let web = Arc::new(
            crate::web::WebConfig::new(options.web_dist.clone(), options.token.clone())
                .with_trust_proxy_headers(options.trust_proxy_headers)
                .with_secure_cookies(options.secure_cookies)
                .with_metrics_state(state_path)
                .with_integrity_options(options.integrity),
        );
        web.restore_metrics_state();
        let _ = web.spawn_spot_check();
        let _ = web.spawn_metrics_flush();
        Ok(Self {
            listener,
            shared: shared.clone(),
            max_connections: options.max_connections,
            active: Arc::new(AtomicUsize::new(0)),
            ws_state: Arc::new(crate::ws::WsServerState {
                shared,
                token: options.token,
                max_connections: options.max_connections,
                active_ws: AtomicUsize::new(0),
                web,
            }),
        })
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, IpcError> {
        self.listener
            .local_addr()
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))
    }

    /// Exposes the shared server's kernel sender so the CLI can trigger
    /// graceful shutdown from a signal handler in resident mode.
    pub fn kernel_sender(&self) -> agentj_core::kernel::KernelSender {
        self.shared.kernel_sender()
    }

    pub fn shared_server(&self) -> Arc<IpcServer> {
        Arc::clone(&self.shared)
    }

    /// The web surface config bound to this server (dist version facts live here).
    pub fn web_config(&self) -> Arc<crate::web::WebConfig> {
        Arc::clone(&self.ws_state.web)
    }

    /// Accept loop: refuse connections beyond `max_connections` by closing the
    /// socket; when the LAST connection drops, signal kernel shutdown (决策 #3).
    pub async fn run(&mut self) -> Result<(), IpcError> {
        loop {
            tokio::select! {
                res = self.listener.accept() => {
                    let (stream, peer) =
                        res.map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
                    if self.active.load(Ordering::SeqCst) >= self.max_connections {
                        drop(stream); // refuse (design 错误处理)
                        continue;
                    }
                    self.active.fetch_add(1, Ordering::SeqCst);
                    let shared = Arc::clone(&self.shared);
                    let active = Arc::clone(&self.active);
                    let ws_state = Arc::clone(&self.ws_state);
                    tokio::spawn(async move {
                        let mut buf = [0u8; 16];
                        let peeked = stream.peek(&mut buf).await.unwrap_or(0);
                        if looks_like_http(&buf[..peeked]) {
                            active.fetch_sub(1, Ordering::SeqCst);
                            let router = crate::ws::ws_router(ws_state, peer);
                            crate::ws::serve_http(stream, router).await;
                        } else {
                            if let Ok(conn) = TcpConnection::new(stream) {
                                let _ = Arc::clone(&shared)
                                    .serve_connection(Box::new(conn))
                                    .await;
                            }
                            active.fetch_sub(1, Ordering::SeqCst);
                            if active.load(Ordering::SeqCst) == 0 {
                                let _ = shared.signal_shutdown();
                            }
                        }
                    });
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                    if self.shared.shutdown_requested() {
                        break;
                    }
                }
            }
        }
        self.ws_state.web.stop_spot_check();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::stub_session;
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop_module::AgentLoopModule;
    use agentj_core::kernel::Kernel;
    use agentj_core::module::*;
    use agentj_core::tool_executor::ToolExecutorModule;
    use agentj_core::turn_runner::TurnRunner;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    fn bus_kernel_and_session() -> (Kernel, Arc<dyn SessionStore>) {
        let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = store.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(agentj_core::types::ModelEvent::Delta("hello".into())),
                Ok(agentj_core::types::ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(crate::test_support::MockConfig),
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
        (kernel, session_dyn)
    }

    #[tokio::test]
    async fn two_clients_serve_independently() {
        let mut server = TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            stub_session(),
            None,
            None,
            TcpBindOptions {
                max_connections: 16,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let handle = tokio::spawn(async move { server.run().await.unwrap() });

        // Three connections held open concurrently: each gets its own session id,
        // and one client's lifecycle never affects another's. (Sequential one-shot
        // connects would race the last-disconnect-shutdown trigger.)
        let mut held = Vec::new();
        for i in 0..3 {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut reader = BufReader::new(r);
            w.write_all(
                format!(r#"{{"jsonrpc":"2.0","id":{i},"method":"session/new"}}"#).as_bytes(),
            )
            .await
            .unwrap();
            w.write_all(b"\n").await.unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(line.contains("\"session_id\""), "client {i}: {line}");
            held.push((reader, w));
        }
        drop(held);
        handle.abort();
    }

    #[tokio::test]
    async fn max_connections_refuses_third_client() {
        let mut server = TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            stub_session(),
            None,
            None,
            TcpBindOptions {
                max_connections: 2,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let handle = tokio::spawn(async move { server.run().await.unwrap() });
        let c1 = TcpStream::connect(addr).await.unwrap();
        let c2 = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await; // let accept register them
        let mut c3 = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1];
        let n = c3.read(&mut buf).await.unwrap_or(0);
        assert_eq!(n, 0, "third connection must be closed by the server");
        drop((c1, c2));
        handle.abort();
    }

    /// TCP round-trip: connect to `addr`, send one NDJSON request, read exactly
    /// one response line, and return the response plus the live stream (the
    /// caller holds the stream open so the server stays resident).
    async fn tcp_rpc(addr: std::net::SocketAddr, req: &str) -> (String, tokio::net::TcpStream) {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (r, mut w) = stream.into_split();
        let mut reader = BufReader::new(r);
        w.write_all(req.as_bytes()).await.unwrap();
        w.write_all(b"\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let stream = w.reunite(reader.into_inner()).unwrap();
        (line, stream)
    }

    async fn tcp_send(stream: tokio::net::TcpStream, req: &str) -> (String, tokio::net::TcpStream) {
        let (r, mut w) = stream.into_split();
        let mut reader = BufReader::new(r);
        w.write_all(req.as_bytes()).await.unwrap();
        w.write_all(b"\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let stream = w.reunite(reader.into_inner()).unwrap();
        (line, stream)
    }

    /// BUG-002 regression guard: `--token` must actually gate the resident TCP
    /// server. A configured token rejects wrong-token/missing-token initialize
    /// with -32806, unauthenticated connections cannot reach business methods,
    /// and a correct-token initialize unlocks session/new.
    #[tokio::test]
    async fn token_gate_rejects_unauthenticated_clients() {
        let mut server = TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            stub_session(),
            None,
            None,
            TcpBindOptions {
                token: Some("s3cret".into()),
                max_connections: 16,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let handle = tokio::spawn(async move { server.run().await.unwrap() });
        let mut held = Vec::new();

        // Wrong token → initialize must be NotAuthorized; a failed initialize
        // never grants access, so session/new stays rejected on this connection.
        let (resp, conn) = tcp_rpc(
            addr,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0","token":"WRONG"}}"#,
        )
        .await;
        assert!(
            resp.contains(r#""code":-32806"#),
            "wrong-token initialize must be NotAuthorized: {resp}"
        );
        let (resp, conn) =
            tcp_send(conn, r#"{"jsonrpc":"2.0","id":2,"method":"session/new"}"#).await;
        assert!(
            resp.contains(r#""code":-32806"#),
            "session/new after failed initialize must be rejected: {resp}"
        );
        held.push(conn);

        let (resp, conn) =
            tcp_rpc(addr, r#"{"jsonrpc":"2.0","id":3,"method":"session/new"}"#).await;
        assert!(
            resp.contains(r#""code":-32806"#),
            "session/new without initialize must be rejected: {resp}"
        );
        held.push(conn);

        let (resp, conn) = tcp_rpc(
            addr,
            r#"{"jsonrpc":"2.0","id":4,"method":"initialize","params":{"protocol_version":"0.4.0","token":"s3cret"}}"#,
        )
        .await;
        assert!(
            resp.contains(r#""connection_id""#) && !resp.contains(r#""code":-32806"#),
            "correct-token initialize must succeed: {resp}"
        );
        let (resp, conn) =
            tcp_send(conn, r#"{"jsonrpc":"2.0","id":5,"method":"session/new"}"#).await;
        assert!(
            resp.contains(r#""session_id""#),
            "session/new after successful initialize must succeed: {resp}"
        );
        held.push(conn);

        drop(held);
        handle.abort();
    }

    #[tokio::test]
    async fn web_config_exposes_the_dist_version() {
        let dir = std::env::temp_dir().join(format!("agentj-tcp-web-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            dir.join("version.json"),
            "{\"product_version\":\"9.9.9\",\"protocol_version\":\"9.9.9\"}",
        )
        .unwrap();
        let server = TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            crate::test_support::stub_session(),
            None,
            None,
            TcpBindOptions {
                web_dist: Some(dir.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let version = server.web_config().dist_version().cloned().expect("parsed");
        assert_eq!(version.product_version, "9.9.9");
        assert!(version.build_time.is_none(), "build_time is optional");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn last_connection_disconnect_triggers_graceful_shutdown() {
        let mut server = TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            stub_session(),
            None,
            None,
            TcpBindOptions {
                max_connections: 16,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let c = TcpStream::connect(addr).await.unwrap();
        drop(c);
        // run() must return (graceful shutdown) once the last connection is gone
        tokio::time::timeout(Duration::from_secs(5), server.run())
            .await
            .expect("server must shut down gracefully after the last client disconnects")
            .unwrap();
    }

    async fn tcp_write_line(w: &mut tokio::net::tcp::OwnedWriteHalf, req: &str) {
        w.write_all(req.as_bytes()).await.unwrap();
        w.write_all(b"\n").await.unwrap();
    }

    /// BUG-006 regression over real TCP: an idle subscriber (session/subscribe
    /// then no further requests) must receive the $/progress broadcast while
    /// another client runs the session.
    #[tokio::test]
    async fn idle_subscriber_receives_progress_broadcast_over_tcp() {
        let (kernel, session) = bus_kernel_and_session();
        let mut server = TcpIpcServer::bind(
            "127.0.0.1:0",
            kernel,
            session,
            None,
            None,
            TcpBindOptions {
                max_connections: 16,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let handle = tokio::spawn(async move { server.run().await.unwrap() });

        // Subscriber A: initialize + session/new + session/subscribe, then idle.
        let stream_a = TcpStream::connect(addr).await.unwrap();
        let (r_a, mut w_a) = stream_a.into_split();
        let mut reader_a = BufReader::new(r_a);
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#;
        tcp_write_line(&mut w_a, init).await;
        let mut line = String::new();
        reader_a.read_line(&mut line).await.unwrap();
        tcp_write_line(
            &mut w_a,
            r#"{"jsonrpc":"2.0","id":2,"method":"session/new"}"#,
        )
        .await;
        line.clear();
        reader_a.read_line(&mut line).await.unwrap();
        let sid = serde_json::from_str::<serde_json::Value>(&line).unwrap()["result"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        tcp_write_line(
            &mut w_a,
            &format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
        )
        .await;
        line.clear();
        reader_a.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"result\""), "subscribe ack: {line}");
        // A is now idle — its reader blocks until the broadcast arrives.

        // Initiator B: initialize + session/run on the same session.
        let stream_b = TcpStream::connect(addr).await.unwrap();
        let (r_b, mut w_b) = stream_b.into_split();
        let mut reader_b = BufReader::new(r_b);
        tcp_write_line(&mut w_b, init).await;
        line.clear();
        reader_b.read_line(&mut line).await.unwrap();
        tcp_write_line(
            &mut w_b,
            &format!(
                r#"{{"jsonrpc":"2.0","id":5,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
        )
        .await;
        // The initiator is auto-subscribed: its first line is the $/progress
        // broadcast (written during the run), then the run response.
        let mut progress = String::new();
        reader_b.read_line(&mut progress).await.unwrap();
        assert!(
            progress.contains("$/progress") && progress.contains("hello"),
            "initiator must receive $/progress first: {progress}"
        );
        line.clear();
        reader_b.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"result\""), "run response: {line}");

        // The broadcast was written to A before the run response, so the idle
        // subscriber must already have it queued on its socket.
        let mut progress_a = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader_a.read_line(&mut progress_a))
            .await
            .expect("idle subscriber must receive $/progress")
            .unwrap();
        assert!(
            progress_a.contains("$/progress") && progress_a.contains("hello"),
            "idle subscriber got: {progress_a}"
        );

        drop((w_a, w_b, reader_a, reader_b));
        handle.abort();
    }

    #[test]
    fn looks_like_http_classifies_correctly() {
        assert!(looks_like_http(b"GET /ws HTTP/1.1"));
        assert!(looks_like_http(b"POST /foo HTTP/1.1"));
        assert!(!looks_like_http(b"{\"jsonrpc\":\"2.0\"}"));
        assert!(!looks_like_http(b""));
        assert!(!looks_like_http(b"\n"));
        assert!(!looks_like_http(b"{"));
    }

    #[tokio::test]
    async fn state_path_is_derived_from_the_bound_port() {
        let dir = tempfile::tempdir().unwrap();
        let server = TcpIpcServer::bind(
            "127.0.0.1:0",
            agentj_core::kernel::Kernel::new(),
            std::sync::Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            TcpBindOptions {
                max_connections: 16,
                trust_proxy_headers: true,
                metrics_state_dir: Some(dir.path().to_path_buf()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let port = server.local_addr().unwrap().port();
        let web = server.web_config();
        assert!(web.trust_proxy_headers());
        assert_eq!(
            web.metrics_state_path(),
            Some(crate::metrics_state::state_path_in(dir.path(), port).as_path())
        );
        // The flush task exists only because a state path is configured.
        web.stop_metrics_flush();
        web.flush_metrics_state();
        let path = crate::metrics_state::state_path_in(dir.path(), port);
        assert!(path.is_file(), "the flush wrote {}", path.display());
    }

    #[tokio::test]
    async fn default_options_keep_state_and_trust_off() {
        let server = TcpIpcServer::bind(
            "127.0.0.1:0",
            agentj_core::kernel::Kernel::new(),
            std::sync::Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            TcpBindOptions::default(),
        )
        .await
        .unwrap();
        let web = server.web_config();
        assert!(!web.trust_proxy_headers());
        assert!(web.metrics_state_path().is_none());
        assert!(web.spawn_metrics_flush().is_none());
    }

    #[test]
    fn default_options_keep_spot_cadence_and_per_request_verification_on() {
        let options = TcpBindOptions::default();
        assert_eq!(options.integrity, crate::web::IntegrityOptions::default());
        assert_eq!(
            options.integrity.spot_interval,
            std::time::Duration::from_secs(60)
        );
        assert!(options.integrity.verify_on_request);
    }

    #[tokio::test]
    async fn bind_applies_the_integrity_options_to_the_web_config() {
        let server = TcpIpcServer::bind(
            "127.0.0.1:0",
            agentj_core::kernel::Kernel::new(),
            std::sync::Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            TcpBindOptions {
                integrity: crate::web::IntegrityOptions {
                    spot_interval: std::time::Duration::from_secs(7),
                    verify_on_request: false,
                    ..crate::web::IntegrityOptions::default()
                },
                ..TcpBindOptions::default()
            },
        )
        .await
        .unwrap();
        let web = server.web_config();
        assert_eq!(web.spot_interval(), std::time::Duration::from_secs(7));
        assert!(!web.verify_on_request_enabled());
        web.stop_spot_check();
    }

    #[tokio::test]
    async fn bind_applies_secure_cookies_to_the_web_config() {
        let server = TcpIpcServer::bind(
            "127.0.0.1:0",
            agentj_core::kernel::Kernel::new(),
            std::sync::Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            TcpBindOptions {
                secure_cookies: true,
                ..TcpBindOptions::default()
            },
        )
        .await
        .unwrap();
        assert!(server.web_config().secure_cookies_enabled());
    }
}
