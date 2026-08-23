//! TCP transport — multi-client JSON-RPC serving over TcpListener (design 决策 #1/#3).

use crate::{Connection, IpcServer, TransportKind};
use agentj_core::agent_loop::AgentLoop;
use agentj_core::error::IpcError;
use agentj_core::kernel::Kernel;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// TCP connection — implements the v0.1 Connection trait (决策 #1): sync send
/// through a cloned blocking socket, async recv on the tokio read half.
pub struct TcpConnection {
    id: String,
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    /// Kept alive for the connection's lifetime: dropping the tokio write half
    /// shuts down the socket's write direction (FIN) and kills the response path.
    _writer_half: tokio::net::tcp::OwnedWriteHalf,
    writer: Mutex<std::net::TcpStream>,
    peer: String,
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
            writer: Mutex::new(writer),
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
        use std::io::Write;
        let mut w = self
            .writer
            .lock()
            .map_err(|_| IpcError::InvalidRequest("tcp writer lock poisoned".into()))?;
        w.write_all(msg.as_bytes())?;
        w.write_all(b"\n")?;
        w.flush()?;
        Ok(())
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
}

/// Multi-client JSON-RPC over TCP — ONE shared IpcServer serves every connection.
pub struct TcpIpcServer {
    listener: TcpListener,
    shared: Arc<IpcServer>,
    token: Option<String>,
    max_connections: usize,
    active: Arc<AtomicUsize>,
}

impl TcpIpcServer {
    pub async fn bind(
        addr: &str,
        agent_loop: AgentLoop,
        kernel: Kernel,
        token: Option<String>,
        max_connections: usize,
    ) -> Result<Self, IpcError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        Ok(Self {
            listener,
            shared: Arc::new(IpcServer::new(agent_loop, kernel)),
            token,
            max_connections,
            active: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, IpcError> {
        self.listener
            .local_addr()
            .map_err(|e| IpcError::InvalidRequest(e.to_string()))
    }

    /// Accept loop: refuse connections beyond `max_connections` by closing the
    /// socket; when the LAST connection drops, signal kernel shutdown (决策 #3).
    pub async fn run(&mut self) -> Result<(), IpcError> {
        loop {
            tokio::select! {
                res = self.listener.accept() => {
                    let (stream, _) =
                        res.map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
                    if self.active.load(Ordering::SeqCst) >= self.max_connections {
                        drop(stream); // refuse (design 错误处理)
                        continue;
                    }
                    self.active.fetch_add(1, Ordering::SeqCst);
                    let shared = Arc::clone(&self.shared);
                    let token = self.token.clone();
                    let active = Arc::clone(&self.active);
                    tokio::spawn(async move {
                        if let Ok(conn) = TcpConnection::new(stream) {
                            // A per-connection error ends only that connection.
                            let _ =
                                shared.serve_connection(Box::new(conn), token.as_deref()).await;
                        }
                        active.fetch_sub(1, Ordering::SeqCst);
                        if active.load(Ordering::SeqCst) == 0 {
                            let _ = shared.signal_shutdown();
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::stub_agent_loop;
    use agentj_core::kernel::Kernel;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn two_clients_serve_independently() {
        let mut server =
            TcpIpcServer::bind("127.0.0.1:0", stub_agent_loop(), Kernel::new(), None, 16)
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
        let mut server =
            TcpIpcServer::bind("127.0.0.1:0", stub_agent_loop(), Kernel::new(), None, 2)
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

    #[tokio::test]
    async fn last_connection_disconnect_triggers_graceful_shutdown() {
        let mut server =
            TcpIpcServer::bind("127.0.0.1:0", stub_agent_loop(), Kernel::new(), None, 16)
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
}
