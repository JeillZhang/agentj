//! Kernel-hosted web UI helpers (design D2/D3/D5): hybrid auth config,
//! opaque session cookies, origin/next validation and the login page.

use axum::response::IntoResponse as _;
use std::time::Instant;

use crate::login_limiter::{LoginLimiter, Throttle};

/// Cookie carrying the opaque session id issued after a successful login.
pub const SESSION_COOKIE: &str = "agentj_session";

/// Version facts written by the UI build into `dist/version.json`, so the
/// kernel can flag a dist that predates (or postdates) this server.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DistVersion {
    pub product_version: String,
    pub protocol_version: String,
    #[serde(default)]
    pub build_time: Option<String>,
}

/// Read `version.json` from the resolved dist. A missing dist or a missing file
/// is normal (legacy build) and yields no error; only a read/parse failure does.
fn read_dist_version(dist: Option<&std::path::Path>) -> (Option<DistVersion>, Option<String>) {
    let Some(dir) = dist else { return (None, None) };
    let path = dir.join("version.json");
    if !path.is_file() {
        return (None, None);
    }
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<DistVersion>(&raw) {
            Ok(version) => (Some(version), None),
            Err(e) => (None, Some(e.to_string())),
        },
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Static-web-UI runtime config: resolved dist directory plus the optional
/// access token and the in-memory set of issued sessions (D3, no persistence).
pub struct WebConfig {
    pub dist: Option<std::path::PathBuf>,
    pub token: Option<String>,
    dist_version: Option<DistVersion>,
    dist_version_error: Option<String>,
    sessions: std::sync::RwLock<std::collections::HashSet<String>>,
    login_limiter: LoginLimiter,
}

impl WebConfig {
    pub fn disabled() -> Self {
        Self::new(None, None)
    }
    /// Empty-string tokens normalize to `None` (no auth configured).
    pub fn new(dist: Option<std::path::PathBuf>, token: Option<String>) -> Self {
        let (dist_version, dist_version_error) = read_dist_version(dist.as_deref());
        Self {
            dist,
            token: token.filter(|t| !t.is_empty()),
            dist_version,
            dist_version_error,
            sessions: std::sync::RwLock::new(std::collections::HashSet::new()),
            login_limiter: LoginLimiter::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_login_limiter(mut self, limiter: LoginLimiter) -> Self {
        self.login_limiter = limiter;
        self
    }
    /// The static web UI is served only when a dist directory was resolved.
    pub fn enabled(&self) -> bool {
        self.dist.is_some()
    }
    pub fn dist_version(&self) -> Option<&DistVersion> {
        self.dist_version.as_ref()
    }
    /// Startup diagnostics: empty when aligned or no dist is configured.
    pub fn startup_warnings(&self) -> Vec<String> {
        if self.dist.is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        match (&self.dist_version, &self.dist_version_error) {
            (Some(v), _) => {
                let product = env!("CARGO_PKG_VERSION");
                if v.product_version != product {
                    out.push(format!(
                        "warning: web dist was built for agentj {} but this server is {}; rebuild the UI",
                        v.product_version, product
                    ));
                }
                let protocol = agentj_protocol::PROTOCOL_VERSION;
                if v.protocol_version != protocol {
                    out.push(format!(
                        "warning: web dist targets protocol {} but this server speaks {}; rebuild the UI",
                        v.protocol_version, protocol
                    ));
                }
            }
            (None, Some(err)) => out.push(format!(
                "warning: web dist version.json is unreadable ({err}); rebuild the UI"
            )),
            (None, None) => out.push(
                "warning: web dist has no version.json (stale build?); rebuild the UI".to_string(),
            ),
        }
        out
    }
    pub fn issue_session(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.write().expect("sessions").insert(id.clone());
        id
    }
    pub fn is_valid_session(&self, id: &str) -> bool {
        self.sessions.read().expect("sessions").contains(id)
    }
    pub fn revoke_session(&self, id: &str) {
        self.sessions.write().expect("sessions").remove(id);
    }
}

/// Whether the request peer is a loopback address (hybrid auth exemption, D2).
pub fn peer_is_loopback(peer: std::net::SocketAddr) -> bool {
    match peer.ip() {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// Extract the `agentj_session` cookie value from a `Cookie` header.
pub fn session_cookie(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("agentj_session="))
}

/// Same-origin check for the cookie channel (D4): strip the scheme from
/// `Origin` and compare the remaining authority to the `Host` header.
pub fn origin_matches_host(headers: &axum::http::HeaderMap) -> bool {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    match (host, origin) {
        (Some(h), Some(o)) => {
            o.strip_prefix("http://")
                .or_else(|| o.strip_prefix("https://"))
                == Some(h)
        }
        _ => false,
    }
}

/// Whitelist `next` redirect targets to same-site absolute paths (防开放重定向).
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.contains('\\')
                && !n.chars().any(char::is_control) =>
        {
            n.to_string()
        }
        _ => "/".to_string(),
    }
}

/// Percent-encode everything but RFC 3986 unreserved characters (and `/`).
pub fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Minimal login page rendered on the remote-access path (D2).
pub fn login_page_html(error: Option<&str>, next: &str) -> String {
    let error = error
        .map(|e| format!("<p class=\"err\">{}</p>", escape_html(e)))
        .unwrap_or_default();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentJ</title></head>\
         <body><main><h1>AgentJ</h1><p>输入访问令牌以连接到内核。</p>{error}\
         <form method=\"post\" action=\"/login\"><input type=\"hidden\" name=\"next\" value=\"{}\">\
         <input name=\"token\" type=\"password\" required autofocus><button>登录</button></form>\
         </main></body></html>",
        escape_html(next)
    )
}

#[derive(serde::Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(serde::Deserialize)]
struct LoginForm {
    token: String,
    next: Option<String>,
}

fn redirect_found(location: &str) -> axum::response::Response {
    (
        axum::http::StatusCode::FOUND,
        [(axum::http::header::LOCATION, location.to_string())],
    )
        .into_response()
}

/// Validate the submitted token (only when one is configured) and, on success,
/// issue an opaque session id in a session cookie (no Max-Age) before
/// redirecting to the whitelisted `next` path. A locked-out peer is answered
/// from the limiter before the token is even read (the failure escalates).
async fn submit_login(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    form: LoginForm,
) -> axum::response::Response {
    let now = Instant::now();
    let ip = peer.ip();
    if matches!(web.login_limiter.check(ip, now), Throttle::Locked { .. }) {
        let throttle = web.login_limiter.record_failure(ip, now);
        return throttle_response(throttle, form.next.as_deref());
    }
    let expected = web.token.as_deref().unwrap_or_default();
    if !expected.is_empty() && form.token == expected {
        web.login_limiter.record_success(ip);
        let sid = web.issue_session();
        let mut res = redirect_found(&safe_next(form.next.as_deref()));
        res.headers_mut().insert(
            axum::http::header::SET_COOKIE,
            format!("{SESSION_COOKIE}={sid}; HttpOnly; SameSite=Lax; Path=/")
                .parse()
                .expect("valid cookie header"),
        );
        res
    } else {
        let throttle = web.login_limiter.record_failure(ip, now);
        throttle_response(throttle, form.next.as_deref())
    }
}

fn unauthorized_response(next: Option<&str>) -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::response::Html(login_page_html(Some("访问令牌无效"), &safe_next(next))),
    )
        .into_response()
}

/// `Locked` answers 429 with the remaining wait (whole seconds, min 1, ceil)
/// and no cookie; an `Allow` throttle is just the plain 401 page.
fn throttle_response(throttle: Throttle, next: Option<&str>) -> axum::response::Response {
    let Throttle::Locked { retry_after } = throttle else {
        return unauthorized_response(next);
    };
    let secs = retry_after.as_millis().div_ceil(1000).max(1) as u64;
    let message = format!("尝试次数过多，请在 {secs} 秒后重试");
    let mut res = (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        axum::response::Html(login_page_html(Some(&message), &safe_next(next))),
    )
        .into_response();
    res.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        axum::http::HeaderValue::from_str(&secs.to_string()).expect("retry-after"),
    );
    res
}

/// Revoke the caller's session if present and clear the cookie (`Max-Age=0`).
async fn logout(web: &WebConfig, headers: axum::http::HeaderMap) -> axum::response::Response {
    if let Some(sid) = session_cookie(&headers) {
        web.revoke_session(sid);
    }
    let mut res = redirect_found("/login");
    res.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
            .parse()
            .expect("valid cookie header"),
    );
    res
}

/// Register the credential endpoints. They are deliberately **not** gated (T5
/// wraps only the static surface), so a signed-out browser cannot loop.
fn auth_routes(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
) -> axum::Router {
    let login_get = |axum::extract::Query(q): axum::extract::Query<LoginQuery>| async move {
        axum::response::Html(login_page_html(None, &safe_next(q.next.as_deref())))
    };
    let login_post = {
        let web = web.clone();
        move |form: axum::extract::Form<LoginForm>| {
            let web = web.clone();
            async move { submit_login(&web, peer, form.0).await }
        }
    };
    let logout_post = move |headers: axum::http::HeaderMap| {
        let web = web.clone();
        async move { logout(&web, headers).await }
    };
    router
        .route("/login", axum::routing::get(login_get).post(login_post))
        .route("/logout", axum::routing::post(logout_post))
}

/// Whether the hybrid login gate applies to this request (design D2): a dist is
/// served, a token is configured, the peer is remote and no valid session
/// cookie was presented. Loopback peers and token-less servers stay open.
pub fn gate_required(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> bool {
    web.enabled()
        && web.token.is_some()
        && !peer_is_loopback(peer)
        && !session_cookie(headers).is_some_and(|sid| web.is_valid_session(sid))
}

/// Login-gate middleware for one static mount. `nest_service` strips the mount
/// prefix before the inner service (and this layer) runs, so `mount` re-attaches
/// it — the redirect must carry the full same-origin path.
async fn gate(
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    mount: &'static str,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if gate_required(&web, peer, req.headers()) {
        let full_path = format!("{mount}{}", req.uri().path());
        return redirect_found(&format!("/login?next={}", pct_encode(&full_path)));
    }
    next.run(req).await
}

/// Mount the static SPA surface (D6: cache headers + login gate wrapped around
/// each service individually — `Router::layer` would leak onto `/ws`). The root
/// mount is `fallback_service` (unknown paths -> index.html, 200) because
/// `nest_service("/")` panics.
fn static_surface(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    dist: &std::path::Path,
) -> axum::Router {
    use tower::Layer as _;
    let spa = tower_http::services::ServeDir::new(dist).fallback(
        tower_http::services::ServeFile::new(dist.join("index.html")),
    );
    let spa = tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    )
    .layer(spa);
    let assets = tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    )
    .layer(tower_http::services::ServeDir::new(dist.join("assets")));
    let gate_assets = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { gate(web, peer, "/assets", req, next).await }
        })
    };
    let gate_spa = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { gate(web, peer, "", req, next).await }
        })
    };
    router
        .nest_service("/assets", gate_assets.layer(assets))
        .fallback_service(gate_spa.layer(spa))
}

/// Build the HTTP router for one accepted connection: the `/ws` upgrade route
/// plus — when a dist was resolved — the statically gated SPA surface. `/ws`,
/// `/login` and `/logout` are never behind the gate (no redirect loop).
pub fn build_router(
    state: std::sync::Arc<crate::ws::WsServerState>,
    peer: std::net::SocketAddr,
) -> axum::Router {
    let ws_route = axum::routing::get({
        let state = state.clone();
        move |axum::extract::Query(q): axum::extract::Query<crate::ws::WsQuery>,
              ws: axum::extract::WebSocketUpgrade,
              headers: axum::http::HeaderMap| {
            let state = state.clone();
            async move { crate::ws::ws_upgrade(state, q, peer, ws, headers).await }
        }
    });
    let mut router = axum::Router::new().route("/ws", ws_route);
    router = auth_routes(router, state.web.clone(), peer);
    if let Some(dist) = state.web.dist.clone() {
        router = static_surface(router, state.web.clone(), peer, &dist);
    }
    router
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn safe_next_whitelists_same_site_paths_only() {
        assert_eq!(safe_next(None), "/");
        assert_eq!(safe_next(Some("/ok?x=1")), "/ok?x=1");
        assert_eq!(safe_next(Some("//evil")), "/");
        assert_eq!(safe_next(Some("https://evil")), "/");
        // Browsers normalize `\` to `/` for special schemes, so `/\evil` would
        // become `//evil` (open redirect); control chars are rejected too.
        assert_eq!(safe_next(Some("/\\evil")), "/");
        assert_eq!(safe_next(Some("/\tevil")), "/");
    }
    #[test]
    fn loopback_detection_covers_v4_mapped_peers() {
        assert!(peer_is_loopback("127.0.0.1:5555".parse().unwrap()));
        assert!(peer_is_loopback("[::1]:5555".parse().unwrap()));
        assert!(peer_is_loopback("[::ffff:127.0.0.1]:5555".parse().unwrap()));
        assert!(!peer_is_loopback("[::ffff:8.8.8.8]:1".parse().unwrap()));
        assert!(!peer_is_loopback("203.0.113.7:41000".parse().unwrap()));
    }
    #[test]
    fn sessions_roundtrip_and_empty_token_normalizes_to_none() {
        let web = WebConfig::new(None, Some(String::new()));
        assert!(web.token.is_none());
        let sid = web.issue_session();
        assert!(web.is_valid_session(&sid));
        web.revoke_session(&sid);
        assert!(!web.is_valid_session(&sid));
    }
    #[test]
    fn pct_encode_escapes_query_delimiters() {
        assert_eq!(pct_encode("/a b?c&d"), "/a%20b%3Fc%26d");
    }

    fn temp_dist(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentj-web-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<html>agentj-index</html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
        dir
    }

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

    #[test]
    fn startup_warnings_cover_aligned_missing_and_mismatched_dists() {
        let aligned = temp_dist("ver-aligned");
        write_version_json(
            &aligned,
            env!("CARGO_PKG_VERSION"),
            agentj_protocol::PROTOCOL_VERSION,
        );
        let web = WebConfig::new(Some(aligned.clone()), None);
        assert!(web.dist_version().is_some());
        assert!(
            web.startup_warnings().is_empty(),
            "{:?}",
            web.startup_warnings()
        );

        let mismatched = temp_dist("ver-mismatch");
        write_version_json(&mismatched, "0.0.1-test", "0.0.1");
        let warnings = WebConfig::new(Some(mismatched.clone()), None).startup_warnings();
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("0.0.1-test") && w.contains(env!("CARGO_PKG_VERSION")))
        );
        assert!(warnings.iter().any(|w| w.contains("protocol")));

        let legacy = temp_dist("ver-legacy"); // no version.json
        let warnings = WebConfig::new(Some(legacy.clone()), None).startup_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("version.json"));

        let broken = temp_dist("ver-broken");
        std::fs::write(broken.join("version.json"), "{not json").unwrap();
        let warnings = WebConfig::new(Some(broken.clone()), None).startup_warnings();
        assert!(warnings[0].contains("unreadable"), "{warnings:?}");

        assert!(WebConfig::disabled().startup_warnings().is_empty());
        for d in [aligned, mismatched, legacy, broken] {
            std::fs::remove_dir_all(&d).ok();
        }
    }

    fn test_state(web: crate::web::WebConfig) -> std::sync::Arc<crate::ws::WsServerState> {
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

    async fn get(
        router: axum::Router,
        path: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        use tower::util::ServiceExt;
        let res = router
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, headers, String::from_utf8_lossy(&body).to_string())
    }

    async fn form_post(
        router: axum::Router,
        path: &str,
        body: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        use tower::util::ServiceExt;
        let res = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, headers, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn serves_index_assets_and_spa_fallback_with_cache_headers() {
        let dist = temp_dist("static");
        let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
        let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, h, b) = get(crate::web::build_router(state.clone(), peer), "/").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert!(b.contains("agentj-index"));
        assert_eq!(
            h.get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        let (s, h, _) = get(
            crate::web::build_router(state.clone(), peer),
            "/assets/app.js",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert!(
            h.get(axum::http::header::CACHE_CONTROL)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("immutable")
        );
        let (s, _, b) = get(crate::web::build_router(state, peer), "/unknown-route").await;
        assert_eq!(s, axum::http::StatusCode::OK, "SPA fallback must be 200");
        assert!(b.contains("agentj-index"));
        std::fs::remove_dir_all(&dist).ok();
    }

    #[tokio::test]
    async fn missing_dist_returns_404() {
        let state = test_state(crate::web::WebConfig::disabled());
        let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, _, _) = get(crate::web::build_router(state, peer), "/").await;
        assert_eq!(s, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn login_sets_session_cookie_and_redirects_to_next() {
        let web = crate::web::WebConfig::new(None, Some("secret".into()));
        let state = test_state(web);
        let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, h, _) = form_post(
            crate::web::build_router(state, peer),
            "/login",
            "token=secret&next=%2Ffoo",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::FOUND);
        assert_eq!(h.get(axum::http::header::LOCATION).unwrap(), "/foo");
        let cookie = h
            .get(axum::http::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            cookie.contains("agentj_session=")
                && cookie.contains("HttpOnly")
                && cookie.contains("SameSite=Lax")
                && cookie.contains("Path=/")
        );
        assert!(!cookie.contains("Max-Age"), "session cookie: {cookie}");
    }

    #[tokio::test]
    async fn login_with_wrong_token_is_401_and_logout_revokes() {
        let state = test_state(crate::web::WebConfig::new(None, Some("secret".into())));
        let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, h, _) = form_post(
            crate::web::build_router(state.clone(), peer),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::UNAUTHORIZED);
        assert!(h.get(axum::http::header::SET_COOKIE).is_none());
        let sid = state.web.issue_session();
        use tower::util::ServiceExt;
        let res = crate::web::build_router(state.clone(), peer)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/logout")
                    .header(axum::http::header::COOKIE, format!("agentj_session={sid}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::FOUND);
        assert!(!state.web.is_valid_session(&sid));
    }

    #[tokio::test]
    async fn login_page_renders_form_with_hidden_next() {
        let state = test_state(crate::web::WebConfig::new(None, Some("secret".into())));
        let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, _, body) = get(crate::web::build_router(state, peer), "/login?next=%2Ffoo").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert!(body.contains("action=\"/login\""), "form action: {body}");
        assert!(body.contains("value=\"/foo\""), "hidden next: {body}");
    }

    #[tokio::test]
    async fn remote_peer_without_cookie_is_redirected_to_login() {
        let dist = temp_dist("gate");
        let state = test_state(crate::web::WebConfig::new(
            Some(dist.clone()),
            Some("secret".into()),
        ));
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let (s, h, _) = get(crate::web::build_router(state.clone(), remote), "/").await;
        assert_eq!(s, axum::http::StatusCode::FOUND);
        assert_eq!(
            h.get(axum::http::header::LOCATION).unwrap(),
            "/login?next=/"
        );
        let (s, _, _) = get(crate::web::build_router(state.clone(), remote), "/login").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        std::fs::remove_dir_all(&dist).ok();
    }
    #[tokio::test]
    async fn loopback_and_cookie_paths_pass_the_gate() {
        let dist = temp_dist("gate-pass");
        let web = crate::web::WebConfig::new(Some(dist.clone()), Some("secret".into()));
        let sid = web.issue_session();
        let state = test_state(web);
        let local: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (s, _, _) = get(crate::web::build_router(state.clone(), local), "/").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        use tower::util::ServiceExt;
        let res = crate::web::build_router(state, remote)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header(axum::http::header::COOKIE, format!("agentj_session={sid}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        std::fs::remove_dir_all(&dist).ok();
    }
    #[tokio::test]
    async fn forged_cookie_and_asset_requests_are_gated() {
        let dist = temp_dist("gate-forged");
        let state = test_state(crate::web::WebConfig::new(
            Some(dist.clone()),
            Some("secret".into()),
        ));
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        use tower::util::ServiceExt;
        let res = crate::web::build_router(state, remote)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/assets/app.js")
                    .header(axum::http::header::COOKIE, "agentj_session=forged")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            axum::http::StatusCode::FOUND,
            "forged cookie must redirect"
        );
        assert_eq!(
            res.headers().get(axum::http::header::LOCATION).unwrap(),
            "/login?next=/assets/app.js"
        );
        std::fs::remove_dir_all(&dist).ok();
    }
    #[tokio::test]
    async fn no_token_remote_peer_passes_without_login() {
        let dist = temp_dist("gate-notoken");
        let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let (s, _, _) = get(crate::web::build_router(state, remote), "/").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        std::fs::remove_dir_all(&dist).ok();
    }

    /// Two failed attempts stay 401, the third locks (50ms base for fast tests).
    fn small_policy() -> crate::login_limiter::Policy {
        crate::login_limiter::Policy {
            free_attempts: 3,
            base_lockout: Duration::from_millis(50),
            max_lockout: Duration::from_millis(400),
            idle_ttl: Duration::from_secs(60),
            max_tracked: 16,
        }
    }

    #[tokio::test]
    async fn fifth_failure_is_429_and_locked_requests_skip_token_check() {
        let state = test_state(crate::web::WebConfig::new(None, Some("secret".into())));
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        for _ in 0..4 {
            let (s, h, _) = form_post(
                crate::web::build_router(state.clone(), remote),
                "/login",
                "token=nope",
            )
            .await;
            assert_eq!(s, axum::http::StatusCode::UNAUTHORIZED);
            assert!(h.get(axum::http::header::SET_COOKIE).is_none());
        }
        let (s, h, body) = form_post(
            crate::web::build_router(state.clone(), remote),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(h.get(axum::http::header::RETRY_AFTER).unwrap(), "30");
        assert!(h.get(axum::http::header::SET_COOKIE).is_none());
        assert!(body.contains("尝试次数过多"), "throttle page: {body}");
        let (s, _, _) = form_post(
            crate::web::build_router(state, remote),
            "/login",
            "token=secret",
        )
        .await;
        assert_eq!(
            s,
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "lockout must skip token check"
        );
    }

    #[tokio::test]
    async fn lockout_expires_and_success_resets_the_counter() {
        let web = crate::web::WebConfig::new(None, Some("secret".into())).with_login_limiter(
            crate::login_limiter::LoginLimiter::with_policy(small_policy()),
        );
        let state = test_state(web);
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let _ = form_post(
            crate::web::build_router(state.clone(), remote),
            "/login",
            "token=nope",
        )
        .await;
        let _ = form_post(
            crate::web::build_router(state.clone(), remote),
            "/login",
            "token=nope",
        )
        .await;
        let (s, _, _) = form_post(
            crate::web::build_router(state.clone(), remote),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::TOO_MANY_REQUESTS);
        tokio::time::sleep(Duration::from_millis(60)).await;
        let (s, h, _) = form_post(
            crate::web::build_router(state.clone(), remote),
            "/login",
            "token=secret",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::FOUND, "lockout must expire");
        assert!(h.get(axum::http::header::SET_COOKIE).is_some());
        for _ in 0..2 {
            let (s, _, _) = form_post(
                crate::web::build_router(state.clone(), remote),
                "/login",
                "token=nope",
            )
            .await;
            assert_eq!(
                s,
                axum::http::StatusCode::UNAUTHORIZED,
                "success cleared the counter"
            );
        }
    }

    #[tokio::test]
    async fn loopback_failures_are_counted_too() {
        let state = test_state(crate::web::WebConfig::new(None, Some("secret".into())));
        let local: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
        for _ in 0..6 {
            let _ = form_post(
                crate::web::build_router(state.clone(), local),
                "/login",
                "token=nope",
            )
            .await;
        }
        let (s, _, _) = form_post(
            crate::web::build_router(state, local),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn retry_after_rounds_up_and_never_drops_below_one() {
        let policy = crate::login_limiter::Policy {
            free_attempts: 1,
            base_lockout: Duration::from_millis(1500),
            ..Default::default()
        };
        let web = crate::web::WebConfig::new(None, Some("secret".into()))
            .with_login_limiter(crate::login_limiter::LoginLimiter::with_policy(policy));
        let state = test_state(web);
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let (s, h, _) = form_post(
            crate::web::build_router(state, remote),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            h.get(axum::http::header::RETRY_AFTER).unwrap(),
            "2",
            "1500ms must ceil to 2s"
        );

        let policy = crate::login_limiter::Policy {
            free_attempts: 1,
            base_lockout: Duration::from_millis(50),
            ..Default::default()
        };
        let web = crate::web::WebConfig::new(None, Some("secret".into()))
            .with_login_limiter(crate::login_limiter::LoginLimiter::with_policy(policy));
        let state = test_state(web);
        let remote: std::net::SocketAddr = "203.0.113.8:41000".parse().unwrap();
        let (_, h, _) = form_post(
            crate::web::build_router(state, remote),
            "/login",
            "token=nope",
        )
        .await;
        assert_eq!(
            h.get(axum::http::header::RETRY_AFTER).unwrap(),
            "1",
            "sub-second locks floor to 1s"
        );
    }
}
