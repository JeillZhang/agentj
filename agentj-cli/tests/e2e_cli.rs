use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn e2e_serve_responds_to_jsonrpc_session_new() {
    // Temp config — avoids touching any real config.toml
    let dir = std::env::temp_dir().join(format!("agentj-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(&cfg, "[model]\nprovider = \"mock\"\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#).unwrap();
    } // drop closes stdin → serve_stdio exits on EOF → kernel graceful shutdown

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#""session_id""#), "expected session_id in output, got: {stdout}");
}

// design #13: SIGTERM must trigger the same graceful shutdown chain as stdin EOF.
// Unix-only (SIGTERM/kill semantics); cfg(unix) keeps the test portable.
#[cfg(unix)]
#[test]
fn e2e_serve_exits_gracefully_on_sigterm() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-sig-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(&cfg, "[model]\nprovider = \"mock\"\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // stdin stays OPEN — only the signal should end the process.
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300)); // let serve loop start

    // Send SIGTERM (kill -TERM). unsafe: raw pid syscall — safe here, child is ours.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "expected graceful exit 0, got: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#""session_id""#), "expected session_id in output, got: {stdout}");
}
