//! E2E resident-kernel lifecycle test (T12): `agentj serve --port 0` binds an
//! ephemeral TCP port, prints `listening on <addr>` on stdout, serves a TCP
//! client's session/new, and exits gracefully when the last client disconnects
//! (design 决策 #3 按需常驻).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn e2e_serve_run_result_includes_usage() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("agentj-e2e-usage-{}-{}", std::process::id(), nanos));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sess_dir = dir.join("sessions");
    std::fs::create_dir_all(&sess_dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"mock\"\n\n\
             [[model.providers]]\nname = \"mock\"\nkind = \"mock\"\
             \nbase_url = \"mock://local\"\napi_key_env = \"\"\n\n\
             [permission.tools.bash]\nmode = \"allow\"\n\n\
             [session]\ndir = \"{}\"\n",
            sess_dir.display()
        ),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"s1","content":"hi"}}}}"#
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#""usage""#),
        "expected usage in run result, got: {stdout}"
    );
    assert!(
        stdout.contains(r#""prompt_tokens":100"#),
        "expected prompt_tokens 100, got: {stdout}"
    );
    assert!(
        stdout.contains(r#""completion_tokens":20"#),
        "expected completion_tokens 20, got: {stdout}"
    );
}

/// Reaps the spawned server on every exit path (including test panics).
struct AutoReap(std::process::Child);

impl Drop for AutoReap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn serve_port_resident_kernel_lifecycle() {
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args([
                "--config",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/permission_deny.toml"
                ),
                "serve",
                "--port",
                "0",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // The resident server prints its bound address before accepting clients.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut addr = String::new();
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.trim().strip_prefix("listening on ") {
                    addr = rest.trim().to_string();
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert!(!addr.is_empty(), "server did not print a listening address");

    // One TCP client performs a full session/new round-trip.
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .unwrap();
    stream.write_all(b"\n").unwrap();
    let mut response = String::new();
    let mut client_reader = BufReader::new(stream.try_clone().unwrap());
    client_reader.read_line(&mut response).unwrap();
    assert!(
        response.contains("\"session_id\""),
        "session/new failed: {response}"
    );

    // Dropping the last connection triggers the graceful shutdown chain.
    drop(client_reader);
    drop(stream);

    let exit_deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < exit_deadline {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "server exited with {status}");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server did not exit after the last client disconnected");
}
