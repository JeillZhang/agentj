use agentj_core::kernel::Kernel;
use agentj_core::module::SessionStore;
use agentj_ipc::tcp::{TcpBindOptions, TcpIpcServer};
use agentj_session::InMemoryStore;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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

async fn bind_server_with_web(
    token: Option<String>,
    web_dist: Option<std::path::PathBuf>,
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
            max_connections: 16,
            web_dist,
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

async fn http_post(addr: std::net::SocketAddr, path: &str, body: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = String::new();
    let mut reader = tokio::io::BufReader::new(&mut stream);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            buf.push_str(&line);
            line.clear();
        }
    })
    .await;
    buf
}

fn login_cookie(response: &str) -> String {
    response
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v))
        .map(|v| v.trim().split(';').next().unwrap().to_string())
        .expect("login must set a cookie")
}

#[tokio::test]
async fn ws_cookie_channel_accepts_same_origin_and_skips_initialize_token() {
    let (addr, handle) = bind_server_with_web(Some("secret".into()), None).await;
    let resp = http_post(addr, "/login", "token=secret").await;
    assert!(
        resp.starts_with("HTTP/1.1 302"),
        "login must redirect: {resp}"
    );
    let cookie = login_cookie(&resp);
    let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    req.headers_mut()
        .insert("origin", format!("http://{addr}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::Text(initialize_line(1).into()))
        .await
        .unwrap();
    let text = ws
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .to_string();
    assert!(
        text.contains("connection_id"),
        "cookie channel needs no initialize.token: {text}"
    );
    ws.close(None).await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn ws_cookie_channel_requires_same_origin() {
    let (addr, handle) = bind_server_with_web(Some("secret".into()), None).await;
    let cookie = login_cookie(&http_post(addr, "/login", "token=secret").await);
    // no Origin -> refused
    let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    // cross-origin -> refused
    let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    req.headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    handle.abort();
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
