//! Shared fixtures for the web surface tests: filesystem dists, manifest
//! writers, a `WsServerState` builder, the `Router` request helpers and the
//! login-throttle policies. Split out so every domain test file can reuse them.
//!
//! The `super::super::*` glob below is what gives this file *and* the sibling
//! test modules that glob-import `support` the production surface of `web`.

use super::super::*;
use std::time::Duration;

use agentj_core::sync::lock_or_recover;

/// True when `body` has a whole line equal to `expected`, so an exact-value
/// check cannot be satisfied by a longer number (`" 5"` vs `" 50"`).
pub(super) fn has_metric_line(body: &str, expected: &str) -> bool {
    body.lines()
        .any(|line| line.trim_end_matches('\r') == expected)
}

pub(super) fn temp_dist(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("agentj-web-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<html>agentj-index</html>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    dir
}

pub(super) fn write_version_json(dir: &std::path::Path, product: &str, protocol: &str) {
    std::fs::write(
        dir.join("version.json"),
        format!(
            "{{\"product_version\":\"{product}\",\"protocol_version\":\"{protocol}\",\
             \"build_time\":\"t\"}}"
        ),
    )
    .unwrap();
}

/// `sha384-<base64>` digest in the SRI format the build manifest uses.
pub(super) fn sha384_b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha384::digest(bytes))
    )
}

/// Write an `asset-manifest.json` with the given `/`-prefixed entries.
pub(super) fn write_manifest(dir: &std::path::Path, entries: &[(&str, &str)]) {
    let assets: std::collections::BTreeMap<&str, &str> = entries.iter().copied().collect();
    let json = serde_json::json!({ "algorithm": "sha384", "assets": assets });
    std::fs::write(dir.join("asset-manifest.json"), json.to_string()).unwrap();
}

/// A manifest matching the bytes `temp_dist` writes by default.
pub(super) fn write_temp_dist_manifest(dir: &std::path::Path) {
    let index = sha384_b64(b"<html>agentj-index</html>");
    let app = sha384_b64(b"console.log(1)");
    write_manifest(dir, &[("/index.html", &index), ("/assets/app.js", &app)]);
}

/// Bounded wait on the spawned spot task: `interval` fires its first tick
/// immediately, so a 20ms schedule reaches the target well within 2s. Real
/// time instead of `start_paused` — tokio's `test-util` feature is not a
/// dependency of this crate (plan T4 fallback).
pub(super) async fn wait_for_spot_rounds(web: &WebConfig, target: u64) {
    let reached = tokio::time::timeout(Duration::from_secs(2), async {
        while web.runtime_snapshot().0.rounds < target {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        reached.is_ok(),
        "spot task did not reach {target} rounds within 2s"
    );
}

pub(super) fn msec_spot(ms: u64, sample: usize) -> SpotCheckConfig {
    SpotCheckConfig {
        interval: Duration::from_millis(ms),
        sample,
    }
}

/// WARN-level capture for the current thread. `#[tokio::test]` defaults to
/// the current-thread runtime, so the spawned spot task is polled on this
/// thread and its warnings land in the buffer (mirrors the `login_limiter`
/// capture helper).
#[derive(Clone, Default)]
pub(super) struct WarnCapture(pub(super) std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for WarnCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock_or_recover(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for WarnCapture {
    type Writer = WarnCapture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub(super) fn capture_warnings() -> (WarnCapture, tracing::subscriber::DefaultGuard) {
    let buf = WarnCapture::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    (buf, tracing::subscriber::set_default(sub))
}

pub(super) fn test_state(web: crate::web::WebConfig) -> std::sync::Arc<crate::ws::WsServerState> {
    let kernel = agentj_core::kernel::Kernel::new();
    let session: std::sync::Arc<dyn agentj_core::module::SessionStore> =
        std::sync::Arc::new(agentj_session::InMemoryStore::new());
    std::sync::Arc::new(crate::ws::WsServerState {
        shared: std::sync::Arc::new(crate::IpcServer::new(kernel, session)),
        token: None,
        max_connections: 16,
        active_ws: std::sync::atomic::AtomicUsize::new(0),
        web: std::sync::Arc::new(web),
    })
}

pub(super) async fn send(
    router: axum::Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
    use tower::util::ServiceExt;
    let mut builder = axum::http::Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = if body.is_empty() {
        builder.body(axum::body::Body::empty()).unwrap()
    } else {
        builder
            .header(
                axum::http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    };
    let res = router.oneshot(request).await.unwrap();
    let (status, response_headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        response_headers,
        String::from_utf8_lossy(&bytes).to_string(),
    )
}

pub(super) async fn get_with_headers(
    router: axum::Router,
    path: &str,
    headers: &[(&str, &str)],
) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
    send(router, "GET", path, headers, "").await
}

pub(super) async fn form_post_with_headers(
    router: axum::Router,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
    send(router, "POST", path, headers, body).await
}

pub(super) async fn get(
    router: axum::Router,
    path: &str,
) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
    get_with_headers(router, path, &[]).await
}

pub(super) async fn form_post(
    router: axum::Router,
    path: &str,
    body: &str,
) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
    form_post_with_headers(router, path, &[], body).await
}

pub(super) const LOOPBACK: &str = "127.0.0.1:41000";
pub(super) const FORWARDED_CLIENT: &str = "198.51.100.9";

/// T6 asserts on a *live* lock (`agentj_login_ip_locked`, `blocked_total`).
/// The shared `small_policy()` locks for only 50 ms, which turns those
/// assertions into wall-clock races, so T6 uses a same-shape policy with a
/// 10 s base lockout: every lock below is still held when `/metrics` is read.
pub(super) fn stable_policy() -> crate::login_limiter::Policy {
    crate::login_limiter::Policy {
        free_attempts: 3,
        base_lockout: std::time::Duration::from_secs(10),
        max_lockout: std::time::Duration::from_secs(900),
        idle_ttl: std::time::Duration::from_secs(60),
        max_tracked: 16,
    }
}

pub(super) fn trusted_web() -> crate::web::WebConfig {
    crate::web::WebConfig::new(None, Some("secret".into()))
        .with_login_limiter(crate::login_limiter::LoginLimiter::with_policy(
            stable_policy(),
        ))
        .with_trust_proxy_headers(true)
}

/// `None` sends **no** `x-forwarded-for` header at all; `Some("")` would send an
/// empty one, which the resolver counts as "header present but unusable".
pub(super) async fn login_failure(
    state: &std::sync::Arc<crate::ws::WsServerState>,
    peer: &str,
    forwarded: Option<&str>,
) {
    let headers: Vec<(&str, &str)> = forwarded
        .map(|value| vec![("x-forwarded-for", value)])
        .unwrap_or_default();
    let _ = form_post_with_headers(
        crate::web::build_router(state.clone(), peer.parse().unwrap()),
        "/login",
        &headers,
        "token=wrong&next=%2F",
    )
    .await;
}

/// Two failed attempts stay 401, the third locks (50ms base for fast tests).
pub(super) fn small_policy() -> crate::login_limiter::Policy {
    crate::login_limiter::Policy {
        free_attempts: 3,
        base_lockout: Duration::from_millis(50),
        max_lockout: Duration::from_millis(400),
        idle_ttl: Duration::from_secs(60),
        max_tracked: 16,
    }
}
