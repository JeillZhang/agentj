//! E2E resident-kernel lifecycle test (T12): `agentj serve --port 0` binds an
//! ephemeral TCP port, prints `listening on <addr>` on stdout, serves a TCP
//! client's session/new, and exits gracefully when the last client disconnects
//! (design 决策 #3 按需常驻).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
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

/// BUG-005 regression guard: the resident TCP/WS serve path (`serve --port` /
/// `dashboard`) must wire the trajectory store so `session/history` replays a
/// completed run's todo/file_change events. Before the fix the TCP shared
/// server had `trajectory_store: None`, so history always returned
/// `{"events": []}` even though the same scenario records 8 events over stdio.
#[test]
fn e2e_tcp_session_history_replays_run_events() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("agentj-e2e-traj-{}-{}", std::process::id(), nanos));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("project");
    let sess_dir = dir.join("sessions");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&sess_dir).unwrap();
    let cfg = dir.join("workbench_config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"mock-workbench\"\n\n\
             [[model.providers]]\nname = \"mock-workbench\"\nkind = \"mock-workbench\"\
             \nbase_url = \"mock://local\"\napi_key_env = \"\"\n\n\
             [permission.tools.write]\nmode = \"allow\"\n\n\
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
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let new_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session/new",
        "params": { "project_root": project },
    });
    writeln!(writer, "{new_request}").unwrap();
    let new_line = read_socket_line(&mut reader, &[]);
    let new_response: serde_json::Value = serde_json::from_str(&new_line).unwrap();
    let sid = new_response["result"]["session_id"]
        .as_str()
        .expect("session/new must return a session id")
        .to_string();

    // One full mock-workbench run: todo call → write call → text delta + Done.
    // The initiator is auto-subscribed, so consume $/progress frames until the
    // run response (id 2) arrives.
    let run = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/run",
        "params": { "session_id": sid, "content": "run the workbench scenario" },
    });
    writeln!(writer, "{run}").unwrap();
    let mut seen: Vec<String> = Vec::new();
    let mut run_result: Option<serde_json::Value> = None;
    for _ in 0..80 {
        let line = read_socket_line(&mut reader, &seen);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        seen.push(line);
        if value["id"] == 2 {
            run_result = Some(value);
            break;
        }
    }
    let run = run_result.expect("session/run must respond; saw {seen:?}");
    assert!(
        run["result"]["content"]
            .as_str()
            .is_some_and(|c| c.contains("workbench done")),
        "run must complete with the scripted final text: {run}"
    );

    // session/history must replay the recorded trajectory (todo + file_change).
    let history = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/history",
        "params": { "session_id": sid },
    });
    writeln!(writer, "{history}").unwrap();
    let hist_line = read_socket_line(&mut reader, &seen);
    let hist: serde_json::Value = serde_json::from_str(&hist_line).unwrap();
    let events = hist["result"]["events"]
        .as_array()
        .expect("session/history must return an events array");
    assert!(
        !events.is_empty(),
        "session/history must replay run events, got: {hist_line}"
    );
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|e| e["value"]["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"todo"),
        "history must contain a todo event; got {kinds:?}"
    );
    assert!(
        kinds.contains(&"file_change"),
        "history must contain a file_change event; got {kinds:?}"
    );

    let _ = child.0.kill();
    let _ = child.0.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

/// T21: a resident `serve --port` process exposes the five subscription methods through the
/// injected `CliProviderService`, and still reports an unsupported provider as -32602.
#[test]
fn serve_exposes_the_subscription_methods() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "agentj-e2e-subscription-{}-{}",
        std::process::id(),
        nanos
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sess_dir = dir.join("sessions");
    std::fs::create_dir_all(&sess_dir).unwrap();

    // A resolvable key is required: providers without one are skipped and an empty
    // registry makes `agentj serve` exit before it prints the listening line.
    // SAFETY: unique test-local variable name; the child inherits it.
    unsafe { std::env::set_var("AGENTJ_E2E_OPENCODE_KEY", "sk-e2e-key") };
    let cfg = dir.join("subscription_config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"opencode-go\"\n\n\
             [[model.providers]]\nname = \"opencode-go\"\nkind = \"opencode-go\"\n\
             base_url = \"https://opencode.ai/zen/go\"\n\
             api_key_env = \"AGENTJ_E2E_OPENCODE_KEY\"\n\n\
             [[model.providers]]\nname = \"deepseek\"\nkind = \"openai-compat\"\n\
             base_url = \"https://api.deepseek.com\"\n\
             api_key_env = \"AGENTJ_E2E_DEEPSEEK_KEY\"\n\n\
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
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut seen: Vec<String> = Vec::new();

    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocol_version":"0.17.0","client_info":{{"name":"e2e","version":"0.27.0"}}}}}}"#
    )
    .unwrap();
    let init = read_socket_line(&mut reader, &seen);
    seen.push(init);

    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":2,"method":"auth/status","params":{{"provider":"opencode-go"}}}}"#
    )
    .unwrap();
    let status_line = read_socket_line(&mut reader, &seen);
    seen.push(status_line.clone());
    let status: serde_json::Value = serde_json::from_str(&status_line).unwrap();
    assert!(
        status["result"].is_object(),
        "expected a result, got {status}"
    );
    assert_eq!(status["result"]["provider"], "opencode-go");

    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":3,"method":"provider/usage","params":{{"provider":"deepseek"}}}}"#
    )
    .unwrap();
    let unsupported_line = read_socket_line(&mut reader, &seen);
    let unsupported: serde_json::Value = serde_json::from_str(&unsupported_line).unwrap();
    assert_eq!(unsupported["error"]["code"], -32602);
    assert_eq!(
        unsupported["error"]["data"]["reason"],
        "unsupported_provider"
    );
}

/// T11: over a real socket, link an API key, read the account catalog, and prove the linked
/// provider is routable by the next run without restarting the kernel (design AC2/AC3).
/// The kernel's console base and `opencode-go` base URL both point at the local wiremock, so
/// the test is deterministic and offline.
#[tokio::test]
async fn serve_links_and_lists_the_account_catalog() {
    let console = wiremock::MockServer::start().await;
    mount_v2_config(&console, 200, v2_body(&["kimi-k3"])).await;
    mount_chat_stream(&console).await;
    let (_kernel, mut client) = serve_linked_kernel(&console.uri());

    let init = client.request(
        "initialize",
        serde_json::json!({
            "protocol_version": agentj_protocol::PROTOCOL_VERSION,
            "client_info": {"name": "e2e", "version": "0.29.0"},
        }),
    );
    assert!(init["result"].is_object(), "initialize failed: {init}");

    let linked = client.call(
        "auth/setApiKey",
        serde_json::json!({"provider": "opencode-go", "api_key": "sk-good"}),
    );
    assert_eq!(linked["outcome"], "verified", "{linked}");
    assert_eq!(linked["saved"], true, "{linked}");
    assert_eq!(linked["source"], "account", "{linked}");
    assert!(
        !linked.to_string().contains("sk-good"),
        "the full key never leaves the kernel: {linked}"
    );

    let listed = client.call(
        "model/list",
        serde_json::json!({"provider": "opencode-go", "refresh": true}),
    );
    assert_eq!(listed["source"], "account", "{listed}");
    assert_eq!(listed["models"][0]["id"], "opencode-go/kimi-k3", "{listed}");

    assert_run_reaches_the_linked_provider(&console, &mut client).await;
}

/// AC3: the same kernel already serves the freshly linked provider — the run is accepted and
/// the linked key reaches the provider base URL without a restart.
async fn assert_run_reaches_the_linked_provider(
    console: &wiremock::MockServer,
    client: &mut SocketClient,
) {
    let new = client.call("session/new", serde_json::json!({}));
    let session_id = new["session_id"].as_str().expect("session id").to_string();
    let run = client.call(
        "session/run",
        serde_json::json!({
            "session_id": session_id,
            "content": "say hello",
            "provider": "opencode-go",
            "model": "opencode-go/kimi-k3",
        }),
    );
    assert!(
        run["content"].as_str().is_some_and(|c| c.contains("Hello")),
        "the linked provider must answer the run: {run}"
    );
    let chats: Vec<_> = console
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/v1/chat/completions")
        .collect();
    assert_eq!(chats.len(), 1, "the provider base URL must receive the run");
    assert_eq!(
        chats[0].headers.get("authorization").unwrap(),
        "Bearer sk-good"
    );
}

/// T12: over a real socket — `auth/clearCredential` followed by `auth/status` reports the
/// cleared state (AC1), a console 401 rejects the key and leaves no credential behind (AC2),
/// a kernel without a provider handle answers `-32603 provider_service_unavailable` (AC3),
/// and link/list/clear never rewrite `config.toml` while `auth.json` demonstrably changes
/// (AC4, positive control). Every console call goes to a local wiremock, so the test stays
/// deterministic and offline.
#[tokio::test]
async fn serve_clear_reject_unattached_and_never_touch_config() {
    let console = wiremock::MockServer::start().await;
    mount_v2_config(&console, 200, v2_body(&["kimi-k3"])).await;
    assert_link_list_clear_never_touch_config(&console);

    let revoked = wiremock::MockServer::start().await;
    mount_v2_config(
        &revoked,
        401,
        serde_json::json!({"error": "invalid_api_key"}),
    )
    .await;
    assert_rejected_key_leaves_no_credential(&revoked).await;

    assert_unattached_kernel_has_no_provider_service();
}

/// AC1 + AC4: link, list, clear and status over the socket. `config.toml` keeps its exact
/// bytes through all four steps (design D6: credentials never migrate into the config file),
/// while `auth.json` appears with the link and is gone after the clear.
fn assert_link_list_clear_never_touch_config(console: &wiremock::MockServer) {
    let (kernel, mut client) = serve_linked_kernel(&console.uri());
    let config_path = kernel.home().join("config.toml");
    let auth_path = kernel.auth_path();
    let config_before = sha256_hex(&std::fs::read(&config_path).unwrap());
    assert!(!auth_path.exists(), "the temp home starts unlinked");
    initialize(&mut client);

    let linked = client.call(
        "auth/setApiKey",
        serde_json::json!({"provider": "opencode-go", "api_key": "sk-good"}),
    );
    assert_eq!(linked["outcome"], "verified", "{linked}");
    let auth_linked = std::fs::read(&auth_path).expect("the link persists auth.json");
    assert!(!auth_linked.is_empty(), "the link wrote credential bytes");

    let listed = client.call(
        "model/list",
        serde_json::json!({"provider": "opencode-go", "refresh": true}),
    );
    assert_eq!(listed["source"], "account", "{listed}");

    let cleared = client.call(
        "auth/clearCredential",
        serde_json::json!({"provider": "opencode-go"}),
    );
    assert_eq!(cleared["cleared"], true, "{cleared}");
    let status = client.call(
        "auth/status",
        serde_json::json!({"provider": "opencode-go"}),
    );
    assert_eq!(status["authenticated"], false, "{status}");

    assert_eq!(
        sha256_hex(&std::fs::read(&config_path).unwrap()),
        config_before,
        "link/list/clear must never write config.toml"
    );
    assert!(
        !auth_path.exists(),
        "clearing the last credential removes auth.json"
    );
}

/// AC2: a console 401 rejects the key before anything is stored — no `auth.json` appears and
/// `config.toml` stays byte-identical (design D2). The single upstream call carrying the
/// candidate key proves the rejection came from that 401, not from local validation.
async fn assert_rejected_key_leaves_no_credential(console: &wiremock::MockServer) {
    let (kernel, mut client) = serve_linked_kernel(&console.uri());
    let config_path = kernel.home().join("config.toml");
    let config_before = sha256_hex(&std::fs::read(&config_path).unwrap());
    initialize(&mut client);

    let rejected = client.call(
        "auth/setApiKey",
        serde_json::json!({"provider": "opencode-go", "api_key": "sk-bad"}),
    );
    assert_eq!(rejected["outcome"], "rejected", "{rejected}");
    assert_eq!(rejected["saved"], false, "{rejected}");
    assert!(
        !rejected.to_string().contains("sk-bad"),
        "the rejected key never leaves the kernel: {rejected}"
    );

    let attempts: Vec<_> = console
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/api/v2/config")
        .collect();
    assert_eq!(
        attempts.len(),
        1,
        "the candidate key is probed exactly once"
    );
    assert_eq!(
        attempts[0].headers.get("authorization").unwrap(),
        "Bearer sk-bad"
    );

    assert!(
        !kernel.auth_path().exists(),
        "a rejected link leaves no credential file"
    );
    assert_eq!(
        sha256_hex(&std::fs::read(&config_path).unwrap()),
        config_before,
        "a rejected link must never write config.toml"
    );
}

/// AC3: the handle-less kernel answers the subscription method with `-32603` and the
/// documented `data.reason` instead of a fabricated result (design D4).
fn assert_unattached_kernel_has_no_provider_service() {
    let (_server, mut client) = unattached_kernel();
    let response = client.request(
        "auth/setApiKey",
        serde_json::json!({"provider": "opencode-go", "api_key": "sk-bad"}),
    );
    assert_eq!(response["error"]["code"], -32603, "{response}");
    assert_eq!(
        response["error"]["data"]["reason"], "provider_service_unavailable",
        "{response}"
    );
    assert!(
        !response.to_string().contains("sk-bad"),
        "the key never reaches an unattached kernel's answer: {response}"
    );
}

/// A per-test tempdir holding `config.toml`, the session store and `XDG_DATA_HOME`, so
/// `auth.json` never touches the developer's real store. Removed on drop.
struct TempHome(PathBuf);

impl TempHome {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("agentj-e2e-{tag}-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One NDJSON JSON-RPC client over the resident server's TCP socket. Reads skip
/// `$/progress` notifications until the response carrying the request id arrives.
struct SocketClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl SocketClient {
    fn connect(addr: &str) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        Self {
            writer: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
            next_id: 1,
        }
    }

    /// Send one request and return its `result`; a JSON-RPC error fails the test.
    fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let response = self.request(method, params);
        if let Some(error) = response.get("error") {
            panic!("{method} failed: {error}");
        }
        response["result"].clone()
    }

    /// Send one request and return the whole response envelope (error included).
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.writer, "{request}").unwrap();
        let mut seen: Vec<String> = Vec::new();
        loop {
            let line = read_socket_line(&mut self.reader, &seen);
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
            seen.push(line);
        }
    }
}

/// A served kernel plus the handles that keep it and its temp home alive: the child is
/// reaped on drop, the stdout pipe stays open (a closed pipe would fail the server's
/// later prints), and the tempdir (config + sessions + `auth.json`) is removed last.
struct ServedKernel {
    _child: AutoReap,
    _server_stdout: BufReader<std::process::ChildStdout>,
    _home: TempHome,
}

impl ServedKernel {
    /// The temp home holding `config.toml`, the session store and `auth.json`.
    fn home(&self) -> &Path {
        self._home.path()
    }

    /// `$XDG_DATA_HOME/agentj/auth.json` — the credential file the kernel writes (T12).
    fn auth_path(&self) -> PathBuf {
        self.home().join("agentj").join("auth.json")
    }
}

/// T11 harness: a kernel whose `XDG_DATA_HOME` is a tempdir (so `auth.json` lands in the
/// temp home) and whose console base and `opencode-go` base URL point at `console_uri`
/// (a local wiremock). The default provider is the keyless `mock`, so startup never probes
/// the real network and never needs a credential.
fn serve_linked_kernel(console_uri: &str) -> (ServedKernel, SocketClient) {
    let home = TempHome::new("link");
    let sessions = home.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let cfg = home.path().join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"mock\"\nconsole_base_url = \"{console_uri}\"\n\n\
             [[model.providers]]\nname = \"mock\"\nkind = \"mock\"\n\
             base_url = \"mock://local\"\napi_key_env = \"\"\n\n\
             [[model.providers]]\nname = \"opencode-go\"\nkind = \"opencode-go\"\n\
             base_url = \"{console_uri}\"\napi_key_env = \"AGENTJ_E2E_LINK_UNSET_KEY\"\n\n\
             [session]\ndir = \"{}\"\n",
            sessions.display()
        ),
    )
    .unwrap();
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args(["--config", cfg.to_str().unwrap(), "serve", "--port", "0"])
            .env("XDG_DATA_HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let mut server_stdout = BufReader::new(stdout);
    let addr = read_listening_addr(&mut server_stdout);
    let client = SocketClient::connect(&addr);
    (
        ServedKernel {
            _child: child,
            _server_stdout: server_stdout,
            _home: home,
        },
        client,
    )
}

/// The account catalog a console key can read: one go-scoped provider carrying `ids`.
fn v2_body(ids: &[&str]) -> serde_json::Value {
    let models: serde_json::Map<String, serde_json::Value> = ids
        .iter()
        .map(|id| ((*id).to_string(), serde_json::json!({})))
        .collect();
    serde_json::json!({"providers": {"opencode": {
        "name": "opencode",
        "settings": {"baseURL": "https://opencode.ai/zen/go"},
        "models": models,
    }}})
}

/// Mount the console catalog endpoint (`GET /api/v2/config`).
async fn mount_v2_config(server: &wiremock::MockServer, status: u16, body: serde_json::Value) {
    wiremock::Mock::given(wiremock::matchers::path("/api/v2/config"))
        .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

/// A text-only SSE completion for the linked provider's chat endpoint.
async fn mount_chat_stream(server: &wiremock::MockServer) {
    let sse = concat!(
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":",
        "[{\"delta\":{\"content\":\"Hello\"},\"index\":0}]}\n\n",
        "data: {\"id\":\"2\",\"object\":\"chat.completion.chunk\",\"choices\":",
        "[{\"delta\":{\"content\":\" world\"},\"index\":0}]}\n\n",
        "data: [DONE]\n\n",
    );
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(sse)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(server)
        .await;
}

/// T12 harness: a real TCP kernel whose server was built without a `provider_service` — the
/// handle-less shape `run` builds and `run --attach` targets (design D4). It runs its own
/// runtime on a dedicated thread, otherwise the blocking `SocketClient` would starve the
/// accept loop; the thread ends with the last connection's shutdown signal.
fn unattached_kernel() -> (std::thread::JoinHandle<()>, SocketClient) {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let kernel = agentj_core::kernel::Kernel::new();
            let session: Arc<dyn agentj_core::module::SessionStore> =
                Arc::new(agentj_session::InMemoryStore::new());
            let mut server = agentj_ipc::tcp::TcpIpcServer::bind(
                "127.0.0.1:0",
                kernel,
                session,
                None,
                None,
                agentj_ipc::tcp::TcpBindOptions::default(),
            )
            .await
            .expect("bind the unattached kernel");
            let addr = server.local_addr().expect("local addr").to_string();
            tx.send(addr).expect("the client waits for the address");
            let _ = server.run().await;
        });
    });
    let addr = rx
        .recv()
        .expect("the unattached kernel reports its address");
    (thread, SocketClient::connect(&addr))
}

/// The TCP handshake the resident kernel expects before business methods.
fn initialize(client: &mut SocketClient) {
    let init = client.request(
        "initialize",
        serde_json::json!({
            "protocol_version": agentj_protocol::PROTOCOL_VERSION,
            "client_info": {"name": "e2e", "version": env!("CARGO_PKG_VERSION")},
        }),
    );
    assert!(init["result"].is_object(), "initialize failed: {init}");
}

/// SHA-256 hex of a file's bytes — the "config.toml untouched" proof for T12.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
