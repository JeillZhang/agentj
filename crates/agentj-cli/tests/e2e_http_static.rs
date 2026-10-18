//! E2E test (T8): `agentj serve --port 0 --web-dist <dist>` hosts the built Web
//! UI on the same port as the JSON-RPC/WS gateway (T3 static mount + T7 CLI
//! discovery). Covers `GET /`, the immutable `/assets` cache header, the SPA
//! fallback, the explicit `/ws` route winning over that fallback, and the
//! missing-dist 404 + startup warning. T8 extends this with the
//! `asset-manifest.json` integrity fixtures (matching manifest stays silent,
//! tampered hash warns + shows up on `/version`). Helpers are defined in-file:
//! integration test crates are separate, so the `e2e_serve.rs` helpers cannot
//! be imported.

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

/// Write the dist version facts the kernel reads at startup and republishes
/// via `/version` (mirrors the real UI build output).
fn write_version_json(dir: &std::path::Path, product: &str, protocol: &str) {
    std::fs::write(
        dir.join("version.json"),
        format!(
            "{{\"product_version\":\"{product}\",\"protocol_version\":\"{protocol}\",\
             \"build_time\":\"t\"}}"
        ),
    )
    .unwrap();
}

/// `sha384-<base64>` digest in the SRI format the asset manifest uses.
fn sha384_b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha384::digest(bytes))
    )
}

/// Write an `asset-manifest.json` with the given `/`-prefixed entries.
fn write_asset_manifest(dir: &std::path::Path, entries: &[(&str, &str)]) {
    let assets: std::collections::BTreeMap<&str, &str> = entries.iter().copied().collect();
    let json = serde_json::json!({ "algorithm": "sha384", "assets": assets });
    std::fs::write(dir.join("asset-manifest.json"), json.to_string()).unwrap();
}

/// A manifest hashing exactly the two files `temp_dist` writes.
fn write_matching_manifest(dir: &std::path::Path) {
    let index = sha384_b64(b"<html>agentj-e2e-index</html>");
    let app = sha384_b64(b"console.log(1)");
    write_asset_manifest(dir, &[("/index.html", &index), ("/assets/app.js", &app)]);
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

/// Issue a raw HTTP/1.1 POST with an empty body; same read-timeout guard as
/// `http_get`.
fn http_post(addr: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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

/// Serve `<dist>` with the shared deny-all fixture, run `probe` once the
/// server prints its address, then return the child's stderr (read after the
/// kill, so the already-emitted warnings do not block).
fn run_serve_and_capture(dist: &std::path::Path, probe: impl FnOnce(&str)) -> String {
    run_serve_with_config(serve_config(), dist, probe)
}

/// `run_serve_and_capture` with an explicit fixture, so tests can drive `[web]`
/// knobs through the real config file.
fn run_serve_with_config(config: &str, dist: &std::path::Path, probe: impl FnOnce(&str)) -> String {
    let mut child = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args([
                "--config",
                config,
                "serve",
                "--port",
                "0",
                "--web-dist",
                dist.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = child.0.stderr.take().unwrap();
    let mut reader = BufReader::new(child.0.stdout.take().unwrap());
    let addr = read_listening_addr(&mut reader);
    probe(&addr);
    let _ = child.0.kill();
    let _ = child.0.wait();
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    err
}

const VERIFY_OFF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/web_verify_off.toml"
);
const FAST_SPOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/web_fast_spot.toml"
);

#[test]
fn serve_serves_web_dist_with_spa_fallback_and_cache_headers() {
    let dist = temp_dist("e2e");
    write_version_json(
        &dist,
        env!("CARGO_PKG_VERSION"),
        agentj_protocol::PROTOCOL_VERSION,
    );
    write_matching_manifest(&dist);
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
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = child.0.stderr.take().unwrap();
    // Hold the reader for the whole test: dropping it would close the stdout
    // pipe while the server may still print its remaining startup banner.
    let mut reader = BufReader::new(child.0.stdout.take().unwrap());
    let addr = read_listening_addr(&mut reader);

    let root = http_get(&addr, "/");
    assert!(root.starts_with("HTTP/1.1 200"), "GET /: {root}");
    assert!(root.contains("agentj-e2e-index"), "index body: {root}");

    let js = http_get(&addr, "/assets/app.js");
    assert!(js.starts_with("HTTP/1.1 200"), "assets: {js}");
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

    let ver = http_get(&addr, "/version");
    assert!(ver.starts_with("HTTP/1.1 200"), "/version: {ver}");
    assert!(
        ver.contains(env!("CARGO_PKG_VERSION")),
        "/version body: {ver}"
    );

    // The warning is written before the accept loop; kill the child to close
    // stderr and read the already-emitted output without blocking.
    let _ = child.0.kill();
    let _ = child.0.wait();
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    assert!(
        !err.contains("no version.json"),
        "aligned dist must not warn: {err}"
    );
    assert!(
        !err.contains("was built for"),
        "aligned dist must not warn: {err}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn serve_without_dist_returns_404_and_warns() {
    // Under `cargo test` the cwd is this crate dir, so no discovery candidate
    // resolves (the repo-root agentj-ui/apps/web/dist is not reachable).
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
    // Hold the reader until the child is reaped (see the first test).
    let mut reader = BufReader::new(child.0.stdout.take().unwrap());
    let addr = read_listening_addr(&mut reader);

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

#[test]
fn serve_warns_when_dist_version_mismatches() {
    let dist = temp_dist("e2e-mismatch");
    write_version_json(&dist, "0.0.0-test", agentj_protocol::PROTOCOL_VERSION);
    write_matching_manifest(&dist);
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
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = child.0.stderr.take().unwrap();
    // Hold the reader until the child is reaped (see the first test).
    let mut reader = BufReader::new(child.0.stdout.take().unwrap());
    let addr = read_listening_addr(&mut reader);

    let ver = http_get(&addr, "/version");
    assert!(
        ver.starts_with("HTTP/1.1 200"),
        "server keeps serving: {ver}"
    );

    // The warning is written before the accept loop; kill the child to close
    // stderr and read the already-emitted line without blocking.
    let _ = child.0.kill();
    let _ = child.0.wait();
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    assert!(
        err.contains("0.0.0-test"),
        "mismatch warning must name the dist version: {err}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn warns_when_manifest_hash_does_not_match() {
    let dist = temp_dist("e2e-integrity-tampered");
    write_version_json(
        &dist,
        env!("CARGO_PKG_VERSION"),
        agentj_protocol::PROTOCOL_VERSION,
    );
    write_asset_manifest(&dist, &[("/index.html", "sha384-AAAA")]);

    let err = run_serve_and_capture(&dist, |addr| {
        let ver = http_get(addr, "/version");
        assert!(ver.contains("\"manifest_present\":true"), "/version: {ver}");
        assert!(ver.contains("\"mismatched\":1"), "/version: {ver}");
    });
    assert!(
        err.contains("integrity"),
        "tampered manifest hash must warn on stderr: {err}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn silent_when_manifest_matches() {
    let dist = temp_dist("e2e-integrity-matching");
    write_version_json(
        &dist,
        env!("CARGO_PKG_VERSION"),
        agentj_protocol::PROTOCOL_VERSION,
    );
    write_matching_manifest(&dist);

    let err = run_serve_and_capture(&dist, |addr| {
        let ver = http_get(addr, "/version");
        assert!(ver.starts_with("HTTP/1.1 200"), "/version: {ver}");
        assert!(ver.contains("\"manifest_present\":true"), "/version: {ver}");
        assert!(ver.contains("\"mismatched\":0"), "/version: {ver}");
    });
    assert!(
        !err.contains("integrity"),
        "matching manifest must not warn: {err}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn manifest_with_root_level_public_asset_verifies_silently() {
    let dist = temp_dist("public-root"); // index.html + assets/app.js
    std::fs::write(dist.join("favicon.svg"), "<svg/>").unwrap();
    write_asset_manifest(
        &dist,
        &[
            ("/index.html", &sha384_b64(b"<html>agentj-e2e-index</html>")),
            ("/assets/app.js", &sha384_b64(b"console.log(1)")),
            ("/favicon.svg", &sha384_b64(b"<svg/>")),
        ],
    );
    let stderr = run_serve_and_capture(&dist, |addr| {
        let ver = http_get(addr, "/version");
        assert!(
            ver.contains("\"checked\":3"),
            "checked must cover root assets: {ver}"
        );
    });
    assert!(
        !stderr.contains("integrity"),
        "unexpected warning: {stderr}"
    );

    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn e1_per_request_check_is_visible_on_metrics() {
    let dist = temp_dist("e1");
    write_matching_manifest(&dist);
    let err = run_serve_and_capture(&dist, |addr| {
        assert!(http_get(addr, "/assets/app.js").contains("200 OK"));
        let metrics = http_get(addr, "/metrics");
        assert!(
            metrics.contains("agentj_web_integrity_request_checked_total 1\n"),
            "the served asset must be hashed exactly once: {metrics}"
        );
    });
    assert!(
        !err.contains("integrity check failed"),
        "unexpected warning: {err}"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn e2_manual_sweep_reports_a_mismatch() {
    let dist = temp_dist("e2");
    write_asset_manifest(&dist, &[("/assets/app.js", "sha384-not-the-real-digest")]);
    run_serve_and_capture(&dist, |addr| {
        let body = http_post(addr, "/integrity/verify");
        assert!(body.contains("200 OK"), "{body}");
        let payload = body.split("\r\n\r\n").nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(json["manifest_present"], serde_json::json!(true));
        assert_eq!(json["mismatched"], serde_json::json!(1));
    });
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn e3_verify_on_request_false_suppresses_the_check() {
    let dist = temp_dist("e3");
    write_matching_manifest(&dist);
    run_serve_with_config(VERIFY_OFF, &dist, |addr| {
        assert!(http_get(addr, "/assets/app.js").contains("200 OK"));
        let metrics = http_get(addr, "/metrics");
        assert!(
            metrics.contains("agentj_web_integrity_request_checked_total 0\n"),
            "verify_on_request = false must suppress hashing: {metrics}"
        );
    });
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn e4_spot_cadence_comes_from_the_config_file() {
    let dist = temp_dist("e4");
    write_matching_manifest(&dist);
    let err = run_serve_with_config(FAST_SPOT, &dist, |addr| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let body = http_get(addr, "/version");
            if let Some(payload) = body.split("\r\n\r\n").nth(1) {
                let json: serde_json::Value = serde_json::from_str(payload).unwrap();
                // `tokio::time::interval` ticks once immediately, so `>= 1` proves
                // nothing about the cadence; a second round inside 15s is what
                // distinguishes the 5s fixture from the 60s default.
                if json["integrity"]["runtime"]["rounds"].as_u64().unwrap_or(0) >= 2 {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        panic!("the runtime spot-check never reached a second round within 15s");
    });
    assert!(
        !err.contains("integrity check failed"),
        "unexpected warning: {err}"
    );
    std::fs::remove_dir_all(&dist).ok();
}
