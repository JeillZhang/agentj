use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mcp_cli_echo.sh"
    )
}

fn lingering_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mcp_lingering.sh"
    )
}

/// True if `pid` still names a live process (kill -0 semantics).
fn process_alive(pid: i32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Poll for the lingering MCP subprocess's pid file.
fn wait_for_pid_file(path: &std::path::Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(pid) = content.trim().parse::<i32>()
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "MCP subprocess never wrote its pid file"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Reaps a spawned process on every exit path (including test panics).
struct AutoReap(std::process::Child);

impl Drop for AutoReap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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

/// BUG-001: the no-subcommand path exits without close_all, orphaning the MCP
/// subprocess that the startup connect loop spawned.
#[test]
fn no_subcommand_closes_mcp_subprocess() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-mcp-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pid_file = dir.join("lingering.pid");
    let cfg = write_cfg(
        &dir,
        "none.toml",
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
env = {{ PID_FILE = "{}" }}
"#,
            dir.join("sessions").display(),
            lingering_fixture(),
            pid_file.display(),
        ),
    );
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "no-subcommand agentj exited non-zero");
    let pid = wait_for_pid_file(&pid_file);
    assert!(
        !process_alive(pid),
        "BUG-001: MCP subprocess {pid} leaked after no-subcommand exit"
    );
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

/// BUG-001: the run --attach path (a client process that connects to a resident
/// daemon) exits without close_all, orphaning its own MCP subprocess.
#[test]
fn run_attach_closes_mcp_subprocess() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-mcp-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let daemon_cfg = write_cfg(
        &dir,
        "daemon.toml",
        &format!(
            r#"
[model]
provider = "mock"
[[model.providers]]
name = "mock"
kind = "mock"
base_url = "mock://local"
api_key_env = ""
[permission.tools.bash]
mode = "allow"
[session]
dir = "{}"
"#,
            dir.join("sessions").display(),
        ),
    );
    let pid_file = dir.join("lingering.pid");
    let client_cfg = write_cfg(
        &dir,
        "client.toml",
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
env = {{ PID_FILE = "{}" }}
"#,
            dir.join("sessions").display(),
            lingering_fixture(),
            pid_file.display(),
        ),
    );
    let mut daemon = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args([
                "--config",
                daemon_cfg.to_str().unwrap(),
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
    let stdout = daemon.0.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
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
    assert!(!addr.is_empty(), "daemon did not print a listening address");
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args([
            "--config",
            client_cfg.to_str().unwrap(),
            "run",
            "--attach",
            &format!("tcp://{addr}"),
            "hi",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "attach client exited non-zero");
    let pid = wait_for_pid_file(&pid_file);
    assert!(
        !process_alive(pid),
        "BUG-001: MCP subprocess {pid} leaked after run --attach exit"
    );
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}
