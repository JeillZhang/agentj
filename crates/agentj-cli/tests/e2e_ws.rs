use futures::{SinkExt, StreamExt};
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

fn initialize_line(id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"initialize","params":{{"protocol_version":"0.4.0","client_info":{{"name":"e2e","version":"1"}}}}}}"#
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_for_listen(port: u16) {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("server did not start listening on {port}");
}

#[tokio::test]
async fn serve_port_accepts_ws_clients_and_keeps_tcp_attach() {
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("agentj-e2e-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(
        &cfg,
        "[model]\nprovider = \"mock\"\n\n[[model.providers]]\nname = \"mock\"\nkind = \"mock\"\nbase_url = \"mock://local\"\napi_key_env = \"\"\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "serve",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agentj serve");
    wait_for_listen(port).await;

    // WS client (browser-equivalent): initialize over ws://.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .unwrap();
    ws.send(Message::Text(initialize_line(1).into()))
        .await
        .unwrap();
    let reply = ws.next().await.unwrap().unwrap();
    assert!(reply.to_text().unwrap().contains("connection_id"));
    ws.close(None).await.unwrap();

    // TCP attach still works (regression): raw NDJSON initialize.
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    tcp.write_all(initialize_line(2).as_bytes()).await.unwrap();
    tcp.write_all(b"\n").await.unwrap();
    let mut line = String::new();
    tokio::io::BufReader::new(&mut tcp)
        .read_line(&mut line)
        .await
        .unwrap();
    assert!(line.contains("connection_id"), "tcp attach: {line}");

    // D7: daemon stays alive after WS-only traffic (no last-disconnect shutdown).
    let mut tcp2 = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    tcp2.write_all(initialize_line(3).as_bytes()).await.unwrap();
    tcp2.write_all(b"\n").await.unwrap();
    let mut line2 = String::new();
    tokio::io::BufReader::new(&mut tcp2)
        .read_line(&mut line2)
        .await
        .unwrap();
    assert!(
        line2.contains("connection_id"),
        "daemon alive after ws-only: {line2}"
    );

    child.kill().expect("kill agentj");
    let _ = child.wait();
}
