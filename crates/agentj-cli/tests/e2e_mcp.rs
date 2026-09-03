use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mcp_cli_echo.sh"
    )
}

fn write_cfg(dir: &std::path::Path, name: &str, toml: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, toml).unwrap();
    p
}

#[test]
fn configured_mcp_server_connects_at_startup() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-mcp-{}", std::process::id()));
    let marker = dir.join("mcp_connected");
    let cfg = write_cfg(
        &dir,
        "mcp.toml",
        &format!(
            r#"
[model]
provider = "mock"
[[model.providers]]
name = "mock"
kind = "mock"
base_url = "mock://local"
api_key_env = ""
[session]
dir = "{}"
[[mcp.servers]]
name = "fs"
command = "sh"
args = ["{}"]
env = {{ MARKER = "{}" }}
"#,
            dir.join("sessions").display(),
            fixture(),
            marker.display(),
        ),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "mcp server did not connect at startup"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(child.stdin.take()); // EOF → graceful shutdown
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn bad_server_does_not_abort_startup() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-mcp-bad-{}", std::process::id()));
    let cfg = write_cfg(
        &dir,
        "bad.toml",
        &format!(
            r#"
[model]
provider = "mock"
[[model.providers]]
name = "mock"
kind = "mock"
base_url = "mock://local"
api_key_env = ""
[session]
dir = "{}"
[[mcp.servers]]
name = "broken"
command = "nonexistent-binary-xyz"
"#,
            dir.join("sessions").display(),
        ),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut addr = String::new();
    while Instant::now() < deadline {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if let Some(rest) = line.trim().strip_prefix("listening on ") {
            addr = rest.trim().to_string();
            break;
        }
    }
    assert!(
        !addr.is_empty(),
        "server must start despite bad mcp server (D4)"
    );
    let _ = child.kill();
    let _ = child.wait();
}
