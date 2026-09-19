//! WebSocket transport (G8): a WS Text message is exactly one JSON-RPC
//! message (native framing, no `\n`). `Connection::send` is synchronous, so
//! outbound lines are queued to an unbounded channel drained by a writer task
//! owning the sink half — mirroring `TcpConnection`'s split-writer pattern so
//! an idle subscriber stays reachable for broadcasts (BUG-006).

use crate::{Connection, ConnectionWriter, IpcError, TransportKind};
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures::stream::SplitStream;
use futures::{SinkExt, StreamExt};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;
use tower::ServiceExt;

pub struct WsConnection {
    id: String,
    peer: String,
    stream: SplitStream<WebSocket>,
    tx: mpsc::UnboundedSender<String>,
}

impl WsConnection {
    pub fn new(socket: WebSocket, peer: String) -> Self {
        // `sink` must be mut: the writer task owns it and calls sink.send().
        let (mut sink, stream) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if sink.send(Message::Text(line.into())).await.is_err() {
                    break;
                }
            }
        });
        Self {
            id: format!("ws-{peer}"),
            peer,
            stream,
            tx,
        }
    }
}

#[async_trait::async_trait]
impl Connection for WsConnection {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::WebSocket
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        self.tx
            .send(msg.to_string())
            .map_err(|_| IpcError::InvalidRequest("ws send channel closed".into()))
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        loop {
            match self.stream.next().await {
                Some(Ok(Message::Text(text))) => return Some(Ok(text.to_string())),
                Some(Ok(Message::Close(_))) | None => return None,
                Some(Ok(_)) => continue, // ping/pong/binary ignored
                Some(Err(e)) => {
                    return Some(Err(IpcError::InvalidRequest(e.to_string())));
                }
            }
        }
    }
    fn peer_desc(&self) -> String {
        format!("ws:{}", self.peer)
    }
    fn writer(&self) -> ConnectionWriter {
        let tx = self.tx.clone();
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer.clone(),
            Arc::new(move |msg: &str| {
                tx.send(msg.to_string())
                    .map_err(|_| IpcError::InvalidRequest("ws send channel closed".into()))
            }),
        )
    }
}

/// Shared state for the WS server (G8). Holds the same Arc<IpcServer> the TCP
/// path uses so WS connections flow through the identical serve loop.
pub struct WsServerState {
    pub shared: Arc<crate::IpcServer>,
    pub token: Option<String>,
    pub max_connections: usize,
    pub active_ws: AtomicUsize,
    pub web: Arc<crate::web::WebConfig>,
}

#[derive(serde::Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

/// Build a per-connection axum Router. Connection-kind routing, the WS upgrade
/// and — when a dist was resolved — the static SPA surface all live in
/// `web::build_router` (design D1/D9: explicit `/ws` route beats the fallback).
pub fn ws_router(state: Arc<WsServerState>, peer: SocketAddr) -> Router {
    crate::web::build_router(state, peer)
}

pub(crate) async fn ws_upgrade(
    state: Arc<WsServerState>,
    query: WsQuery,
    peer: SocketAddr,
    ws: WebSocketUpgrade,
) -> Response {
    // 设计 D6 双通道 ①：upgrade 阶段 ?token= 校验。
    if state.token.is_some() && state.token.as_deref() != query.token.as_deref() {
        return (StatusCode::FORBIDDEN, "missing or invalid token").into_response();
    }
    if state.active_ws.load(Ordering::SeqCst) >= state.max_connections {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many connections").into_response();
    }
    state.active_ws.fetch_add(1, Ordering::SeqCst);
    let state = state.clone(); // clone the Arc for the upgraded task
    ws.on_upgrade(move |socket| async move {
        let conn = WsConnection::new(socket, peer.to_string());
        // 设计 D7: WS 连接走既有 serve_connection（注册表/订阅/鉴权零改动）。
        let _ = Arc::clone(&state.shared)
            .serve_connection(Box::new(conn), state.token.as_deref())
            .await;
        state.active_ws.fetch_sub(1, Ordering::SeqCst);
    })
}

/// Serve one accepted HTTP/WS connection through hyper + the axum Router
/// (设计 D5, axum 官方 serve-with-hyper 模式; upgrades required for WS).
pub async fn serve_http(stream: tokio::net::TcpStream, router: Router) {
    let io = TokioIo::new(stream);
    let svc = service_fn(move |req: hyper::Request<Incoming>| {
        let router = router.clone();
        async move {
            let req = req.map(axum::body::Body::new);
            Ok::<_, Infallible>(router.oneshot(req).await.unwrap_or_else(|e| match e {}))
        }
    });
    let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(io, svc)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::ws::{WebSocket, WebSocketUpgrade};
    use axum::routing::get;
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    /// Minimal in-process axum upgrade server that serves a WsConnection and
    /// echoes every received line back through the connection's writer (the
    /// BUG-006 idle-subscriber path).
    async fn echo_handler(ws: WebSocketUpgrade) -> axum::response::Response {
        ws.on_upgrade(|socket: WebSocket| async move {
            let conn = WsConnection::new(socket, "test-peer".into());
            let writer = conn.writer();
            let mut conn = conn;
            while let Some(Ok(line)) = conn.recv().await {
                let _ = writer.send(&format!("echo:{line}"));
            }
        })
    }

    #[tokio::test]
    async fn ws_connection_roundtrips_text_frames_via_writer() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route("/ws", get(echo_handler));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (mut socket, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();
        socket
            .send(ClientMessage::Text("hello".into()))
            .await
            .unwrap();
        let reply = socket.next().await.unwrap().unwrap();
        assert_eq!(reply.into_text().unwrap(), "echo:hello");
        socket.close(None).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn ws_connection_close_frame_yields_eof() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route("/ws", get(close_probe_handler));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (mut socket, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();
        socket.close(None).await.unwrap();
        let _ = socket.next().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        server.abort();
    }

    async fn close_probe_handler(ws: WebSocketUpgrade) -> axum::response::Response {
        ws.on_upgrade(|socket: WebSocket| async move {
            let mut conn = WsConnection::new(socket, "test-peer".into());
            assert!(conn.recv().await.is_none(), "close frame must yield EOF");
        })
    }
}
