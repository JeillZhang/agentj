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

/// Read server stdout lines until the resident server prints `listening on
/// <addr>`; returns the address (panics on EOF / timeout / absence).
fn read_listening_addr<R: BufRead>(reader: &mut R) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.trim().strip_prefix("listening on ") {
                    return rest.trim().to_string();
                }
            }
            Err(_) => break,
        }
    }
    panic!("server did not print a listening address");
}

/// Read one NDJSON line from the server. Bounded by the socket's read timeout
/// (set by the caller): a stalled or closed server fails fast with the lines
/// seen so far instead of hanging the test.
fn read_socket_line(reader: &mut BufReader<TcpStream>, seen: &[String]) -> String {
    let mut line = String::new();
    let count = reader
        .read_line(&mut line)
        .unwrap_or_else(|e| panic!("timed out waiting for a server line ({e}); saw {seen:?}"));
    assert!(count > 0, "server closed the connection; saw {seen:?}");
    line.trim_end().to_string()
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
    let addr = read_listening_addr(&mut reader);

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

/// T26: end-to-end ask round-trip over TCP. The `mock-ask` provider scripts one
/// `ask` tool call (question + two options). The owner connection receives
/// `$/askQuestion`, answers `opt-a` via `question/respond`, and the run then
/// completes with the ask tool result carrying `opt-a`.
#[test]
fn e2e_tcp_ask_roundtrip() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentj-e2e-ask-{}-{}", std::process::id(), nanos));
    let _ = std::fs::remove_dir_all(&dir);
    let sess_dir = dir.join("sessions");
    std::fs::create_dir_all(&sess_dir).unwrap();
    let cfg = dir.join("mock_ask_config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"mock-ask\"\n\n\
             [[model.providers]]\nname = \"mock-ask\"\nkind = \"mock-ask\"\
             \nbase_url = \"mock://local\"\napi_key_env = \"\"\n\n\
             [session]\ndir = \"{}\"\n",
            sess_dir.display()
        ),
    )
    .unwrap();
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args(["--config", cfg.to_str().unwrap(), "serve", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let mut server_reader = BufReader::new(stdout);
    let addr = read_listening_addr(&mut server_reader);

    let stream = TcpStream::connect(&addr).unwrap();
    // The ask has no timeout: the client must answer, so every read is bounded
    // by this read timeout and fails the test fast instead of hanging forever.
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
    )
    .unwrap();
    let new_line = read_socket_line(&mut reader, &[]);
    let new_response: serde_json::Value = serde_json::from_str(&new_line).unwrap();
    let sid = new_response["result"]["session_id"]
        .as_str()
        .expect("session/new must return a session id")
        .to_string();

    let run = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"ask the owner"}}}}"#
    );
    writeln!(writer, "{run}").unwrap();

    // While the run blocks on ask, the owner connection gets $/askQuestion.
    let mut seen: Vec<String> = Vec::new();
    let mut ask: Option<serde_json::Value> = None;
    for _ in 0..40 {
        let line = read_socket_line(&mut reader, &seen);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        seen.push(line);
        if value["method"] == "$/askQuestion" {
            ask = Some(value);
            break;
        }
    }
    let ask =
        ask.unwrap_or_else(|| panic!("no $/askQuestion within the line budget; saw {seen:?}"));
    let question_id = ask["params"]["question_id"]
        .as_str()
        .expect("$/askQuestion must carry question_id")
        .to_string();
    assert!(
        !question_id.is_empty(),
        "$/askQuestion question_id must not be empty: {ask}"
    );
    let options = ask["params"]["options"]
        .as_array()
        .expect("$/askQuestion must carry options");
    assert_eq!(options.len(), 2, "ask must deliver two options: {ask}");

    let respond = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"question/respond","params":{{"question_id":"{question_id}","answers":["opt-a"],"skipped":false}}}}"#
    );
    writeln!(writer, "{respond}").unwrap();

    // Answering unblocks the tool: expect the respond ack, the ask tool result
    // carrying opt-a, and finally the run response. Ordering between the ack
    // and the run response is not fixed, so keep reading until both are seen.
    let mut lines: Vec<String> = Vec::new();
    let mut ack = false;
    let mut tool_result = false;
    let mut run_result: Option<serde_json::Value> = None;
    for _ in 0..80 {
        let line = read_socket_line(&mut reader, &lines);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["id"] == 3 && value["result"]["accepted"] == true {
            ack = true;
        }
        if value["method"] == "$/progress"
            && value["params"]["value"]["kind"] == "tool_result"
            && value["params"]["value"]["tool_call_id"] == "call_ask"
        {
            let content = value["params"]["value"]["content"]
                .as_str()
                .unwrap_or_default();
            assert!(
                content.contains("opt-a"),
                "ask tool result must carry opt-a, got {content}"
            );
            tool_result = true;
        }
        lines.push(line);
        if value["id"] == 2 {
            run_result = Some(value);
            if ack {
                break;
            }
        }
    }
    assert!(ack, "question/respond must be accepted:true; saw {lines:?}");
    assert!(tool_result, "missing ask tool_result frame; saw {lines:?}");
    let run = run_result.expect("session/run must respond after the ask was answered");
    assert!(
        run["result"]["content"]
            .as_str()
            .is_some_and(|c| c.contains("ask answered")),
        "run must complete with the scripted final text: {run}"
    );

    let _ = child.0.kill();
    let _ = child.0.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
