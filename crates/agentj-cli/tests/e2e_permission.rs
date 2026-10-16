//! E2E permission flow test (T11): `agentj serve` with a `[permission]` config and a
//! mock provider (`kind = "mock"`) scripting a bash tool call. The run blocks on
//! `mode = "ask"`, a `$/permissionRequest` notification is emitted, and the run
//! completes after the approval is auto-denied by the permission timeout. NOTE: the
//! `permission/respond` written to stdin is never consumed by the sequential
//! serve loop (the loop is blocked inside the run until the timeout fires), so
//! this test exercises NOTIFICATION EMISSION + TIMEOUT AUTO-DENY + RUN COMPLETION;
//! interactive respond is covered by handler-level unit tests in agentj-ipc and
//! agentj-permission. Mirrors the spawn/line-read convention of `tests/e2e_cli.rs`.
//!
//! Two properties keep this test from dominating (or hanging) the workspace suite:
//! the fixture sets `[permission] timeout_ms = 800` (design default: 60s) so the
//! auto-deny lands quickly, and every read goes through `next_json` with a
//! wall-clock deadline. It previously blocked forever: `session/run` was issued
//! with a hardcoded session id that `session/new` never returns, so the server
//! answered `session error: session 's1' is inactive` and the raw `read_line`
//! then waited on stdout that is never closed — `cargo test --workspace` never
//! terminated.
//!
//! Adaptations vs. the plan brief (documented in task-11-report.md):
//! - The server does not dispatch an `initialize` method yet, so there is no
//!   handshake to perform — start directly with `session/new` (same as e2e_cli.rs).
//! - The mock scripts a bash call that falls through to `mode = "ask"` (it does NOT
//!   match the fixture's `rm -rf` deny rule — deny short-circuits before asking).
//! - The session/run response carries only Deltas (the ToolResult event is not
//!   surfaced by the IPC dispatch), so the final assertion checks the run completed
//!   after the denial rather than grepping the denied text out of the response line.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Whole-test budget: covers process startup, the run, and the 800ms auto-deny.
const DEADLINE: Duration = Duration::from_secs(20);

/// One JSON-RPC line, or EOF — the reader thread's only outputs.
enum Event {
    Line(String),
    Eof,
}

/// Drain the server's stdout on a thread so the main body can wait with a
/// deadline instead of blocking a raw `read_line` forever.
fn spawn_line_reader<R: BufRead + Send + 'static>(mut stdout: R) -> Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match stdout.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Event::Eof);
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    // Skip tracing log lines the server writes to stdout.
                    if trimmed.starts_with('{')
                        && tx.send(Event::Line(trimmed.to_string())).is_err()
                    {
                        return;
                    }
                }
            }
        }
    });
    rx
}

/// Next JSON line, failing the test (instead of blocking) once `deadline` passes.
fn next_json(rx: &Receiver<Event>, deadline: Instant, what: &str) -> String {
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Event::Line(line)) => line,
        Ok(Event::Eof) => panic!("server closed stdout while waiting for {what}"),
        Err(RecvTimeoutError::Timeout) => {
            panic!("timed out after {DEADLINE:?} waiting for {what}")
        }
        Err(RecvTimeoutError::Disconnected) => panic!("stdout reader died waiting for {what}"),
    }
}

/// Session id from a `session/new` response line.
fn session_id_from(response: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(response).unwrap();
    value["result"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no session_id in {response}"))
        .to_string()
}

/// Kill the served kernel on scope exit so a failed assertion never leaks it.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn permission_request_notified_and_timeout_denies() {
    let deadline = Instant::now() + DEADLINE;
    let dir = std::env::temp_dir().join(format!("agentj-e2e-perm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("permission_deny.toml");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/permission_deny.toml"
        ),
        &cfg,
    )
    .unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let mut stdin = child.0.stdin.take().unwrap();
    let lines = spawn_line_reader(BufReader::new(child.0.stdout.take().unwrap()));

    // 1. session/new — keep the real id: session/run rejects ids the store does
    //    not know, and a hardcoded id is what used to make this test hang.
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
    )
    .unwrap();
    stdin.flush().unwrap();
    let sid = session_id_from(&next_json(&lines, deadline, "session/new response"));

    // 2. session/run — the mock provider (kind="mock") streams one bash tool call
    //    that falls through to mode=ask (it does not match the fixture's "rm -rf"
    //    deny rule, which short-circuits before asking), then a final text reply.
    let run = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/run",
        "params": { "session_id": sid, "content": "run" },
    });
    writeln!(stdin, "{run}").unwrap();
    stdin.flush().unwrap();

    // 3. Read notifications: expect $/permissionRequest; write a respond deny
    //    (never consumed by the sequential loop — kept to mirror the interactive
    //    protocol shape), then the final session/run response must arrive after
    //    the approval timeout (fixture: 800ms) auto-denies.
    let mut saw_permission_request = false;
    let mut final_seen = false;
    while Instant::now() < deadline {
        let line = next_json(
            &lines,
            deadline,
            "the run to request permission or complete",
        );
        if line.contains("$/permissionRequest") {
            saw_permission_request = true;
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            let request_id = v["params"]["request_id"].as_str().unwrap();
            let respond = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "permission/respond",
                "params": { "request_id": request_id, "decision": "deny" },
            });
            writeln!(stdin, "{respond}").unwrap();
            stdin.flush().unwrap();
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
            && v.get("id").and_then(|id| id.as_u64()) == Some(3)
        {
            assert!(
                v.get("error").is_none(),
                "session/run failed before the auto-deny: {line}"
            );
            final_seen = true;
            break;
        }
    }
    assert!(
        saw_permission_request,
        "no $/permissionRequest notification received"
    );
    assert!(
        final_seen,
        "final session/run response did not arrive after the denial"
    );
}
