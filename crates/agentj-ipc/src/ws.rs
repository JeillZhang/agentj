//! WebSocket transport (G8): a WS Text message is exactly one JSON-RPC
//! message (native framing, no `\n`). `Connection::send` is synchronous, so
//! outbound lines are queued to an unbounded channel drained by a writer task
//! owning the sink half — mirroring `TcpConnection`'s split-writer pattern so
//! an idle subscriber stays reachable for broadcasts (BUG-006).

use crate::{Connection, ConnectionWriter, IpcError, TransportKind};
use axum::extract::ws::{Message, WebSocket};
use futures::stream::SplitStream;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;

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
