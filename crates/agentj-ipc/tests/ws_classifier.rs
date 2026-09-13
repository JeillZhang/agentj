use agentj_core::kernel::Kernel;
use agentj_core::module::SessionStore;
use agentj_ipc::tcp::{TcpBindOptions, TcpIpcServer};
use agentj_session::InMemoryStore;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

fn initialize_line(id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"initialize","params":{{"protocol_version":"0.4.0","client_info":{{"name":"test","version":"1"}}}}}}"#
    )
}

async fn bind_server(
    token: Option<String>,
    max_connections: usize,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let kernel = Kernel::new();
    let session: Arc<dyn SessionStore> = Arc::new(InMemoryStore::new());
    let mut server = TcpIpcServer::bind(
        "127.0.0.1:0",
        kernel,
        session,
        None,
        None,
        TcpBindOptions {
            token,
            max_connections,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let addr = server.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = server.run().await;
    });
    (addr, handle)
}

#[tokio::test]
async fn same_port_serves_ws_and_raw_tcp() {
    let (addr, handle) = bind_server(None, 16).await;

    // WS client: upgrade + initialize round-trip.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .unwrap();
    ws.send(Message::Text(initialize_line(1).into()))
        .await
        .unwrap();
    let reply = ws.next().await.unwrap().unwrap();
    let text = reply.to_text().unwrap();
    assert!(text.contains("connection_id"), "ws initialize: {text}");
    ws.close(None).await.unwrap();

    // Raw TCP client: NDJSON initialize round-trip still works (regression).
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(initialize_line(2).as_bytes()).await.unwrap();
    tcp.write_all(b"\n").await.unwrap();
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(&mut tcp);
    reader.read_line(&mut line).await.unwrap();
    assert!(line.contains("connection_id"), "tcp initialize: {line}");

    handle.abort();
}

#[tokio::test]
async fn ws_bad_query_token_is_refused_at_upgrade() {
    let (addr, handle) = bind_server(Some("secret".into()), 16).await;
    let res = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token=wrong")).await;
    assert!(res.is_err(), "bad ?token= must refuse the WS upgrade");
    handle.abort();
}

#[tokio::test]
async fn ws_initialize_without_token_fails_with_not_authorized() {
    let (addr, handle) = bind_server(Some("secret".into()), 16).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token=secret"))
        .await
        .unwrap();
    ws.send(Message::Text(initialize_line(1).into()))
        .await
        .unwrap();
    let reply = ws.next().await.unwrap().unwrap();
    let text = reply.to_text().unwrap();
    assert!(text.contains("-32806"), "missing initialize.token: {text}");
    ws.close(None).await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn ws_max_connections_rejects_excess() {
    let (addr, handle) = bind_server(None, 1).await;
    let (mut ws1, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .unwrap();
    let res = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await;
    assert!(
        res.is_err(),
        "second WS connection must be refused at max_connections=1"
    );
    ws1.close(None).await.unwrap();
    handle.abort();
}
