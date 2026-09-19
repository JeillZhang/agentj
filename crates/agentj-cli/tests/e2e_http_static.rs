//! E2E test (T8): `agentj serve --port 0 --web-dist <dist>` hosts the built Web
//! UI on the same port as the JSON-RPC/WS gateway (T3 static mount + T7 CLI
//! discovery). Covers `GET /`, the immutable `/assets` cache header, the SPA
//! fallback, the explicit `/ws` route winning over that fallback, and the
//! missing-dist 404 + startup warning. Helpers are defined in-file: integration
//! test crates are separate, so the `e2e_serve.rs` helpers cannot be imported.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Build a throwaway dist dir with an index marker and one asset file.
fn temp_dist(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("agentj-cli-e2e-web-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<html>agentj-e2e-index</html>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    dir
}

/// Issue a raw HTTP/1.1 GET and return the raw response text. The 5s read
/// timeout bounds a stalled or closed server instead of hanging the test.
fn http_get(addr: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf
}

/// Read server stdout lines until the resident server prints
/// `listening on <addr>`; returns the address (pattern mirrored from
/// tests/e2e_serve.rs; panics on EOF / timeout / absence).
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

/// Reaps the spawned server on every exit path (including test panics).
struct AutoReap(std::process::Child);

impl Drop for AutoReap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The shared deny-all permission fixture path (mirrors e2e_serve.rs).
fn serve_config() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/permission_deny.toml"
    )
}

#[test]
fn serve_serves_web_dist_with_spa_fallback_and_cache_headers() {
    let dist = temp_dist("e2e");
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args([
                "--config",
                serve_config(),
                "serve",
                "--port",
                "0",
                "--web-dist",
                dist.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let addr = read_listening_addr(&mut BufReader::new(child.0.stdout.take().unwrap()));

    let root = http_get(&addr, "/");
    assert!(root.starts_with("HTTP/1.1 200"), "GET /: {root}");
    assert!(root.contains("agentj-e2e-index"), "index body: {root}");

    let js = http_get(&addr, "/assets/app.js");
    assert!(
        js.to_ascii_lowercase()
            .contains("cache-control: public, max-age=31536000, immutable"),
        "assets must be immutably cached: {js}"
    );

    let spa = http_get(&addr, "/does-not-exist");
    assert!(
        spa.starts_with("HTTP/1.1 200") && spa.contains("agentj-e2e-index"),
        "SPA fallback must be 200 + index body: {spa}"
    );

    let ws = http_get(&addr, "/ws");
    assert!(
        !ws.contains("agentj-e2e-index"),
        "explicit /ws route must win over the SPA fallback: {ws}"
    );
    assert!(
        ws.starts_with("HTTP/1.1 4"),
        "/ws without upgrade headers expects a 4xx: {ws}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn serve_without_dist_returns_404_and_warns() {
    // No --web-dist and no AGENTJ_WEB_DIST: the discovery candidates relative to
    // the crate cwd / exe dir do not exist, so the static surface stays off.
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args(["--config", serve_config(), "serve", "--port", "0"])
            .env_remove("AGENTJ_WEB_DIST")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = child.0.stderr.take().unwrap();
    let addr = read_listening_addr(&mut BufReader::new(child.0.stdout.take().unwrap()));

    let root = http_get(&addr, "/");
    assert!(
        root.starts_with("HTTP/1.1 404"),
        "static surface absent must be 404: {root}"
    );

    // The warning is written before the accept loop; kill the child to close
    // stderr and read the already-emitted line without blocking.
    let _ = child.0.kill();
    let _ = child.0.wait();
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    assert!(
        err.contains("web dist not found"),
        "stderr must warn about the missing dist: {err}"
    );
}
