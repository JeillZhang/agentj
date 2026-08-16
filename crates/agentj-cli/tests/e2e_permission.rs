//! E2E permission flow test (T11): `agentj serve` with a `[permission]` config and a
//! mock provider (`kind = "mock"`) scripting a bash tool call. The run blocks on
//! `mode = "ask"`, a `$/permissionRequest` notification is emitted, and the run
//! completes after the approval is auto-denied by the 60s timeout. NOTE: the
//! `permission/respond` written to stdin is never consumed by the sequential
//! serve loop (the loop is blocked inside the run until the timeout fires), so
//! this test exercises NOTIFICATION EMISSION + TIMEOUT AUTO-DENY + RUN COMPLETION;
//! interactive respond is covered by handler-level unit tests in agentj-ipc and
//! agentj-permission. Mirrors the spawn/line-read convention of `tests/e2e_cli.rs`.
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
use std::process::{Command, Stdio};

/// Read the next JSON line (skips tracing log lines the server writes to stdout).
fn read_json_line<R: BufRead>(stdout: &mut R, line: &mut String) -> bool {
    loop {
        line.clear();
        if stdout.read_line(line).unwrap() == 0 {
            return false; // EOF
        }
        if line.trim_start().starts_with('{') {
            return true;
        }
    }
}

#[test]
fn permission_request_notified_and_timeout_denies() {
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();

    // 1. session/new.
    write!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
    )
    .unwrap();
    writeln!(stdin).unwrap();
    stdin.flush().unwrap();
    line.clear();
    assert!(
        read_json_line(&mut stdout, &mut line),
        "EOF before session/new response"
    );
    assert!(line.contains("session_id"), "session/new failed: {line}");

    // 2. session/run — the mock provider (kind="mock") streams one bash tool call
    //    that falls through to mode=ask (it does not match the fixture's "rm -rf"
    //    deny rule, which short-circuits before asking), then a final text reply.
    write!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"s1","content":"run"}}}}"#
    )
    .unwrap();
    writeln!(stdin).unwrap();
    stdin.flush().unwrap();

    // 3. Read notifications: expect $/permissionRequest; write a respond deny
    //    (never consumed by the sequential loop — kept to mirror the interactive
    //    protocol shape), then the final session/run response must arrive after
    //    the approval times out (60s) and auto-denies.
    let mut saw_permission_request = false;
    let mut final_seen = false;
    for _ in 0..30 {
        if !read_json_line(&mut stdout, &mut line) {
            break;
        }
        if line.contains("$/permissionRequest") {
            saw_permission_request = true;
            let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
            let request_id = v["params"]["request_id"].as_str().unwrap();
            let respond = format!(
                concat!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"permission/respond","#,
                    r#"params":{{"request_id":"{request_id}","decision":"deny"}}}}"#,
                ),
                request_id = request_id,
            );
            write!(stdin, "{respond}").unwrap();
            writeln!(stdin).unwrap();
            stdin.flush().unwrap();
        }
        if line.contains("\"result\"") && line.contains("\"response\"") {
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

    let _ = child.kill();
    let _ = child.wait();
}
