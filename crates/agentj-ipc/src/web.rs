//! Kernel-hosted web UI helpers (design D2/D3/D5): hybrid auth config,
//! opaque session cookies, origin/next validation and the login page.

use axum::response::IntoResponse as _;
use base64::Engine as _;
use sha2::{Digest as _, Sha384};
use std::time::{Duration, Instant};

use crate::login_limiter::{Counters, IpSnapshot, LoginLimiter, Throttle};

/// Cookie carrying the opaque session id issued after a successful login.
pub const SESSION_COOKIE: &str = "agentj_session";

/// Upper bound on the per-IP series exported by `GET /metrics` (design G2):
/// the aggregate gauges stay exact, the series are ranked and capped.
pub const PER_IP_SERIES_LIMIT: usize = 50;

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

/// `dist/asset-manifest.json` written by the UI build (design D5/G5): the
/// sha384 digest of every bundle artefact, keyed by its service path.
#[derive(serde::Deserialize)]
struct AssetManifest {
    algorithm: String,
    assets: std::collections::HashMap<String, String>,
}

/// Read and validate the dist manifest. `(None, None)` means the file is absent
/// (legacy build); unreadable or invalid content yields an error string.
fn read_asset_manifest(dist: Option<&std::path::Path>) -> (Option<AssetManifest>, Option<String>) {
    let Some(dir) = dist else { return (None, None) };
    let path = dir.join("asset-manifest.json");
    if !path.is_file() {
        return (None, None);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) => return (None, Some(e.to_string())),
    };
    match serde_json::from_str::<AssetManifest>(&raw) {
        Ok(manifest) => match validate_manifest(&manifest) {
            Ok(()) => (Some(manifest), None),
            Err(reason) => (None, Some(reason)),
        },
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Every key must be a `/`-prefixed relative service path without a traversal
/// component (design D5); any violation makes the whole manifest unusable.
fn validate_manifest(manifest: &AssetManifest) -> Result<(), String> {
    if manifest.algorithm != "sha384" {
        return Err(format!("unsupported algorithm {:?}", manifest.algorithm));
    }
    for key in manifest.assets.keys() {
        let Some(rel) = key.strip_prefix('/') else {
            return Err(format!("asset key {key:?} must start with '/'"));
        };
        if rel.is_empty() || rel.contains('\\') || rel.split('/').any(|s| s == "..") {
            return Err(format!(
                "asset key {key:?} is not a canonical relative path"
            ));
        }
    }
    Ok(())
}

/// Outcome of hashing one manifest entry against the bytes on disk.
enum AssetCheck {
    Match,
    Mismatch,
    Missing,
}

/// Hash one manifest entry against the bytes on disk; shared by the startup
/// sweep and the runtime spot-check rounds.
fn check_asset(dist: &std::path::Path, key: &str, expected: &str) -> AssetCheck {
    let rel = key.strip_prefix('/').unwrap_or(key);
    match std::fs::read(dist.join(rel)) {
        Ok(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(Sha384::digest(&bytes));
            if expected.strip_prefix("sha384-") == Some(encoded.as_str()) {
                AssetCheck::Match
            } else {
                AssetCheck::Mismatch
            }
        }
        Err(_) => AssetCheck::Missing,
    }
}

/// Hash every manifest entry against the bytes on disk (design D6/D7):
/// `checked` counts entries whose file exists, and offending keys are collected
/// so the startup warning can name them.
fn verify_assets(dist: &std::path::Path, manifest: &AssetManifest) -> IntegrityReport {
    let mut report = IntegrityReport {
        manifest_present: true,
        ..Default::default()
    };
    for (key, expected) in &manifest.assets {
        match check_asset(dist, key, expected) {
            AssetCheck::Match => report.checked += 1,
            AssetCheck::Mismatch => {
                report.checked += 1;
                report.mismatched += 1;
                report.offenses.push(key.clone());
            }
            AssetCheck::Missing => {
                report.missing += 1;
                report.offenses.push(key.clone());
            }
        }
    }
    report
}

/// Read + verify the manifest once at startup (design D7). A missing dist stays
/// silent; every other failure yields a non-blocking warning (design D8). The
/// parsed manifest is handed back so runtime spot checks can reuse it (D6/D7).
fn read_integrity(
    dist: Option<&std::path::Path>,
) -> (IntegrityReport, Option<String>, Option<AssetManifest>) {
    let Some(dir) = dist else {
        return (IntegrityReport::default(), None, None);
    };
    match read_asset_manifest(Some(dir)) {
        (Some(manifest), _) => (verify_assets(dir, &manifest), None, Some(manifest)),
        (None, Some(err)) => (
            IntegrityReport::default(),
            Some(format!(
                "warning: web dist asset manifest is unusable ({err}); skipping integrity verification"
            )),
            None,
        ),
        (None, None) => (
            IntegrityReport::default(),
            Some(
                "warning: web dist has no asset-manifest.json (stale build?); rebuild the UI"
                    .to_string(),
            ),
            None,
        ),
    }
}

/// Warning for a verified manifest with failed entries (design D8): at most
/// three offending paths, sorted for deterministic output.
fn integrity_warning(report: &IntegrityReport) -> Option<String> {
    if !report.manifest_present || (report.mismatched == 0 && report.missing == 0) {
        return None;
    }
    let mut names: Vec<&str> = report.offenses.iter().map(String::as_str).collect();
    names.sort_unstable();
    let listed = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    let suffix = if names.len() > 3 { ", …" } else { "" };
    Some(format!(
        "warning: web dist asset integrity check failed: {} mismatched, {} missing after {} \
         checked ({listed}{suffix})",
        report.mismatched, report.missing, report.checked
    ))
}

/// Startup integrity summary for the mounted dist (design G6): counts only,
/// exposed through `WebConfig::integrity()`.
#[derive(Default, Clone)]
pub(crate) struct IntegrityReport {
    pub(crate) manifest_present: bool,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    /// Keys whose digest mismatched or whose file was missing.
    offenses: Vec<String>,
}

/// Runtime spot-check schedule (design D6): each round hashes `sample` manifest
/// entries, advancing a cursor over the sorted key set so the full manifest is
/// covered every `ceil(keys / sample)` rounds.
#[allow(dead_code)] // fields are read by the periodic spot-check task (plan T4)
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpotCheckConfig {
    pub(crate) interval: Duration,
    pub(crate) sample: usize,
}

impl Default for SpotCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            sample: 8,
        }
    }
}

/// Cumulative runtime spot-check state (design D7): written by the periodic
/// task, read by `/version` and `/metrics`.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpotCheckReport {
    pub(crate) rounds: u64,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) last_run: Option<Instant>,
}

/// Outcome of one spot-check round. `offenses` names mismatched and missing
/// paths that were not reported by an earlier round, so the periodic task can
/// warn once per path (design D7).
#[allow(dead_code)] // fields are read by the periodic spot-check task (plan T4)
#[derive(Debug, Clone, Default)]
pub(crate) struct SpotRound {
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) offenses: Vec<String>,
}

/// Static-web-UI runtime config: resolved dist directory plus the optional
/// access token and the in-memory set of issued sessions (D3, no persistence).
pub struct WebConfig {
    pub dist: Option<std::path::PathBuf>,
    pub token: Option<String>,
    dist_version: Option<DistVersion>,
    dist_version_error: Option<String>,
    integrity: IntegrityReport,
    integrity_error: Option<String>,
    #[allow(dead_code)] // sampled by the periodic spot-check task (plan T4)
    manifest: Option<AssetManifest>,
    #[allow(dead_code)] // read by the periodic spot-check task (plan T4)
    spot: SpotCheckConfig,
    spot_report: std::sync::RwLock<SpotCheckReport>,
    #[allow(dead_code)] // recorded by the periodic spot-check task (plan T4)
    warned: std::sync::Mutex<std::collections::HashSet<String>>,
    #[allow(dead_code)] // rotated by the periodic spot-check task (plan T4)
    spot_cursor: std::sync::atomic::AtomicUsize,
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
        let (integrity, integrity_error, manifest) = read_integrity(dist.as_deref());
        Self {
            dist,
            token: token.filter(|t| !t.is_empty()),
            dist_version,
            dist_version_error,
            integrity,
            integrity_error,
            manifest,
            spot: SpotCheckConfig::default(),
            spot_report: std::sync::RwLock::new(SpotCheckReport::default()),
            warned: std::sync::Mutex::new(std::collections::HashSet::new()),
            spot_cursor: std::sync::atomic::AtomicUsize::new(0),
            sessions: std::sync::RwLock::new(std::collections::HashSet::new()),
            login_limiter: LoginLimiter::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_login_limiter(mut self, limiter: LoginLimiter) -> Self {
        self.login_limiter = limiter;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_spot_check(mut self, spot: SpotCheckConfig) -> Self {
        self.spot = spot;
        self
    }
    /// Run one spot-check round (design D6): hash up to `spot.sample` manifest
    /// entries starting at the rotating cursor over the sorted key set, fold the
    /// outcome into the shared report and return it. Offending paths recorded in
    /// `warned` by an earlier round are omitted, and the task records new ones
    /// (design D7) so persistent tampering warns once per path.
    #[allow(dead_code)] // invoked by the periodic spot-check task (plan T4)
    pub(crate) fn run_spot_round(&self) -> SpotRound {
        let (Some(dist), Some(manifest)) = (&self.dist, &self.manifest) else {
            return SpotRound::default();
        };
        let mut keys: Vec<&str> = manifest.assets.keys().map(String::as_str).collect();
        if keys.is_empty() {
            return SpotRound::default();
        }
        keys.sort_unstable();
        let sample = self.spot.sample.max(1).min(keys.len());
        let cursor = self
            .spot_cursor
            .fetch_add(sample, std::sync::atomic::Ordering::Relaxed);
        let start = cursor % keys.len();
        let mut round = SpotRound::default();
        for index in 0..sample {
            let key = keys[(start + index) % keys.len()];
            match check_asset(dist, key, &manifest.assets[key]) {
                AssetCheck::Match => round.checked += 1,
                AssetCheck::Mismatch => {
                    round.checked += 1;
                    round.mismatched += 1;
                    round.offenses.push(key.to_string());
                }
                AssetCheck::Missing => {
                    round.missing += 1;
                    round.offenses.push(key.to_string());
                }
            }
        }
        {
            let mut report = self.spot_report.write().unwrap_or_else(|e| e.into_inner());
            report.rounds += 1;
            report.checked += round.checked;
            report.mismatched += round.mismatched;
            report.missing += round.missing;
            report.last_run = Some(Instant::now());
        }
        let warned = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        round.offenses.retain(|path| !warned.contains(path));
        round
    }
    /// The runtime spot-check report plus the age of its last round in whole
    /// seconds; the age is `None` until the first round has run (design G7).
    pub(crate) fn runtime_snapshot(&self) -> (SpotCheckReport, Option<u64>) {
        let report = self
            .spot_report
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let age = report
            .last_run
            .map(|t| Instant::now().duration_since(t).as_secs());
        (report, age)
    }
    /// The static web UI is served only when a dist directory was resolved.
    pub fn enabled(&self) -> bool {
        self.dist.is_some()
    }
    pub fn dist_version(&self) -> Option<&DistVersion> {
        self.dist_version.as_ref()
    }
    pub(crate) fn integrity(&self) -> &IntegrityReport {
        &self.integrity
    }
    /// Startup diagnostics: empty when aligned or no dist is configured.
    /// Version mismatches and failed integrity checks warn without blocking.
    pub fn startup_warnings(&self) -> Vec<String> {
        if self.dist.is_none() {
            return Vec::new();
        }
        let mut out = self.version_warnings();
        if let Some(error) = &self.integrity_error {
            out.push(error.clone());
        } else if let Some(warning) = integrity_warning(self.integrity()) {
            out.push(warning);
        }
        out
    }
    fn version_warnings(&self) -> Vec<String> {
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
    web.enabled() && auth_required(web, peer, headers)
}

/// Auth check without the dist requirement (design G3): the machine surface
/// (`/metrics`) must answer even when no static UI is mounted. A token must be
/// configured, the peer must be remote and no valid session cookie presented.
pub fn auth_required(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> bool {
    web.token.is_some()
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

/// Unauthenticated version probe: reports this server's product/protocol
/// versions, the optional facts read from the mounted dist, the aggregate
/// login-limiter counters, the startup integrity summary and the runtime
/// spot-check block, so a remote client or operator can detect a stale UI and
/// observe throttling without holding a session cookie. Registered as an
/// explicit route (like `/login`), so the login gate and the SPA fallback never
/// answer it.
async fn version_handler(web: std::sync::Arc<WebConfig>) -> axum::response::Response {
    let dist = web.dist_version();
    let counters = web.login_limiter.counters(Instant::now());
    let integrity = web.integrity();
    let (runtime, last_run_age) = web.runtime_snapshot();
    let mut runtime_json = serde_json::json!({
        "rounds": runtime.rounds,
        "checked": runtime.checked,
        "mismatched": runtime.mismatched,
        "missing": runtime.missing,
    });
    if let Some(age) = last_run_age {
        runtime_json["last_run_age_secs"] = serde_json::json!(age);
    }
    let body = serde_json::json!({
        "product_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": agentj_protocol::PROTOCOL_VERSION,
        "dist_present": web.dist.is_some(),
        "dist": dist.map(|v| serde_json::json!({
            "product_version": v.product_version,
            "protocol_version": v.protocol_version,
            "build_time": v.build_time,
        })),
        "login_limiter": {
            "attempts": counters.attempts,
            "failures": counters.failures,
            "lockouts": counters.lockouts,
            "blocked": counters.blocked,
            "locked_now": counters.locked_now,
            "tracked_ips": counters.tracked_ips,
        },
        "integrity": {
            "manifest_present": integrity.manifest_present,
            "checked": integrity.checked,
            "mismatched": integrity.mismatched,
            "missing": integrity.missing,
            "runtime": runtime_json,
        },
    });
    let mut res = axum::Json(body).into_response();
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    res
}

/// Auth-gated Prometheus scrape endpoint (design G2/D1): the machine surface
/// answers `401` without a redirect, so scrapers never chase the login page.
async fn metrics_handler(
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    if auth_required(&web, peer, &headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            [
                ("content-type", "text/plain"),
                ("cache-control", "no-store"),
            ],
            "unauthorized\n",
        )
            .into_response();
    }
    (
        [
            ("content-type", "text/plain; version=0.0.4; charset=utf-8"),
            ("cache-control", "no-store"),
        ],
        prometheus_text(&web),
    )
        .into_response()
}

/// Prometheus text exposition (design G2): limiter aggregates, the ranked
/// per-IP series capped at [`PER_IP_SERIES_LIMIT`], the startup integrity
/// summary and the runtime spot-check counters.
fn prometheus_text(web: &WebConfig) -> String {
    let now = Instant::now();
    let counters = web.login_limiter.counters(now);
    let series = web.login_limiter.snapshot(now);
    let (runtime, _) = web.runtime_snapshot();
    let mut out = String::new();
    push_login_aggregates(&mut out, &counters);
    push_login_ip_series(&mut out, &series, now);
    push_integrity_summary(&mut out, web.integrity());
    push_spot_summary(&mut out, &runtime);
    out
}

fn push_login_aggregates(out: &mut String, counters: &Counters) {
    push_metric(
        out,
        "agentj_login_attempts_total",
        "counter",
        "Login attempts that reached the throttler.",
        counters.attempts,
    );
    push_metric(
        out,
        "agentj_login_failures_total",
        "counter",
        "Token check failures (locked-out attempts excluded).",
        counters.failures,
    );
    push_metric(
        out,
        "agentj_login_lockouts_total",
        "counter",
        "Lockouts newly triggered (extensions excluded).",
        counters.lockouts,
    );
    push_metric(
        out,
        "agentj_login_blocked_total",
        "counter",
        "Requests rejected while their source IP was locked out.",
        counters.blocked,
    );
    push_metric(
        out,
        "agentj_login_locked_ips",
        "gauge",
        "Source IPs currently locked out.",
        counters.locked_now,
    );
    push_metric(
        out,
        "agentj_login_tracked_ips",
        "gauge",
        "Source IPs currently tracked by the throttler.",
        counters.tracked_ips,
    );
}

fn push_login_ip_series(out: &mut String, snapshot: &[IpSnapshot], now: Instant) {
    let truncated = snapshot.len() > PER_IP_SERIES_LIMIT;
    let shown = &snapshot[..snapshot.len().min(PER_IP_SERIES_LIMIT)];
    push_ip_family(
        out,
        "agentj_login_ip_failures_total",
        "counter",
        "Token check failures per source IP (top series by lock state and failures).",
        shown,
        |s| u64::from(s.failures),
    );
    push_ip_family(
        out,
        "agentj_login_ip_locked",
        "gauge",
        "Whether the source IP is currently locked out (1) or not (0).",
        shown,
        |s| u64::from(s.locked),
    );
    push_ip_family(
        out,
        "agentj_login_ip_retry_after_seconds",
        "gauge",
        "Remaining lockout for the source IP in whole seconds (0 when unlocked).",
        shown,
        |s| s.retry_after.as_secs(),
    );
    push_ip_family(
        out,
        "agentj_login_ip_last_seen_seconds",
        "gauge",
        "Seconds since the source IP last reached the throttler.",
        shown,
        |s| now.duration_since(s.last_seen).as_secs(),
    );
    push_metric(
        out,
        "agentj_login_ip_series_truncated",
        "gauge",
        "Whether the per-IP series were capped at the Top-50 limit (1) or not (0).",
        u64::from(truncated),
    );
}

fn push_integrity_summary(out: &mut String, report: &IntegrityReport) {
    push_metric(
        out,
        "agentj_web_manifest_present",
        "gauge",
        "Whether the dist asset manifest was present and usable (1) or not (0).",
        u64::from(report.manifest_present),
    );
    push_metric(
        out,
        "agentj_web_assets_checked",
        "gauge",
        "Startup integrity check: assets hashed.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_assets_mismatched",
        "gauge",
        "Startup integrity check: digest mismatches.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_assets_missing",
        "gauge",
        "Startup integrity check: manifest entries missing on disk.",
        report.missing,
    );
}

/// Runtime spot-check counters (design G2), cumulative across rounds.
fn push_spot_summary(out: &mut String, report: &SpotCheckReport) {
    push_metric(
        out,
        "agentj_web_spot_rounds_total",
        "counter",
        "Web asset spot-check rounds completed.",
        report.rounds,
    );
    push_metric(
        out,
        "agentj_web_spot_checked_total",
        "counter",
        "Web asset spot-check: files hashed across rounds.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_spot_mismatched_total",
        "counter",
        "Web asset spot-check: digest mismatches across rounds.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_spot_missing_total",
        "counter",
        "Web asset spot-check: manifest entries missing on disk across rounds.",
        report.missing,
    );
}

fn push_metric(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    out.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"
    ));
}

/// Family header followed by one labelled sample per shown IP; the text format
/// wants each family's samples contiguous, so callers loop one family at a time.
fn push_ip_family(
    out: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    series: &[IpSnapshot],
    value: impl Fn(&IpSnapshot) -> u64,
) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
    for snap in series {
        out.push_str(&format!(
            "{name}{{ip=\"{}\"}} {}\n",
            escape_label(&snap.ip.to_string()),
            value(snap)
        ));
    }
}

/// Escape a Prometheus label value: `\`, `"` and newlines (text format 0.0.4).
fn escape_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// Build the HTTP router for one accepted connection: the `/ws` upgrade route
/// plus — when a dist was resolved — the statically gated SPA surface. `/ws`,
/// `/login`, `/logout`, `/version` and `/metrics` are never behind the static
/// gate (no redirect loop; the probes stay reachable for remote peers and the
/// metrics endpoint gates itself with a plain `401`).
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
    let version_route = {
        let web = state.web.clone();
        move || {
            let web = web.clone();
            async move { version_handler(web).await }
        }
    };
    let metrics_route = {
        let web = state.web.clone();
        move |headers: axum::http::HeaderMap| {
            let web = web.clone();
            async move { metrics_handler(web, peer, headers).await }
        }
    };
    let mut router = axum::Router::new()
        .route("/ws", ws_route)
        .route("/version", axum::routing::get(version_route))
        .route("/metrics", axum::routing::get(metrics_route));
    router = auth_routes(router, state.web.clone(), peer);
    if let Some(dist) = state.web.dist.clone() {
        router = static_surface(router, state.web.clone(), peer, &dist);
    }
    router
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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

    /// `sha384-<base64>` digest in the SRI format the build manifest uses.
    fn sha384_b64(bytes: &[u8]) -> String {
        use base64::Engine as _;
        use sha2::Digest as _;
        format!(
            "sha384-{}",
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha384::digest(bytes))
        )
    }

    /// Write an `asset-manifest.json` with the given `/`-prefixed entries.
    fn write_manifest(dir: &std::path::Path, entries: &[(&str, &str)]) {
        let assets: std::collections::BTreeMap<&str, &str> = entries.iter().copied().collect();
        let json = serde_json::json!({ "algorithm": "sha384", "assets": assets });
        std::fs::write(dir.join("asset-manifest.json"), json.to_string()).unwrap();
    }

    /// A manifest matching the bytes `temp_dist` writes by default.
    fn write_temp_dist_manifest(dir: &std::path::Path) {
        let index = sha384_b64(b"<html>agentj-index</html>");
        let app = sha384_b64(b"console.log(1)");
        write_manifest(dir, &[("/index.html", &index), ("/assets/app.js", &app)]);
    }

    #[test]
    fn startup_warnings_cover_aligned_missing_and_mismatched_dists() {
        let aligned = temp_dist("ver-aligned");
        write_version_json(
            &aligned,
            env!("CARGO_PKG_VERSION"),
            agentj_protocol::PROTOCOL_VERSION,
        );
        write_temp_dist_manifest(&aligned);
        let web = WebConfig::new(Some(aligned.clone()), None);
        assert!(web.dist_version().is_some());
        assert!(
            web.startup_warnings().is_empty(),
            "{:?}",
            web.startup_warnings()
        );

        let mismatched = temp_dist("ver-mismatch");
        write_version_json(&mismatched, "0.0.1-test", "0.0.1");
        write_temp_dist_manifest(&mismatched);
        let warnings = WebConfig::new(Some(mismatched.clone()), None).startup_warnings();
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("0.0.1-test") && w.contains(env!("CARGO_PKG_VERSION")))
        );
        assert!(warnings.iter().any(|w| w.contains("protocol")));

        let legacy = temp_dist("ver-legacy"); // no version.json
        write_temp_dist_manifest(&legacy);
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

    #[test]
    fn integrity_report_counts_mismatches_and_missing() {
        let dir = temp_dist("integrity");
        fs::write(dir.join("index.html"), b"<html></html>").unwrap();
        let good = sha384_b64(b"<html></html>");
        write_manifest(
            &dir,
            &[
                ("/index.html", &good),
                ("/assets/missing.js", "sha384-AAAA"),
            ],
        );
        let cfg = WebConfig::new(Some(dir), None);
        let report = cfg.integrity();
        assert_eq!(
            (report.manifest_present, report.checked, report.missing),
            (true, 1, 1)
        );
        assert!(
            cfg.startup_warnings()
                .iter()
                .any(|w| w.contains("integrity"))
        );
    }

    #[test]
    fn manifest_with_parent_dir_key_is_unusable() {
        let dir = temp_dist("integrity-bad");
        write_manifest(&dir, &[("/../etc/passwd", "sha384-AAAA")]);
        let cfg = WebConfig::new(Some(dir), None);
        assert!(!cfg.integrity().manifest_present);
        assert!(
            cfg.startup_warnings()
                .iter()
                .any(|w| w.contains("asset manifest")),
            "{:?}",
            cfg.startup_warnings()
        );
    }

    #[test]
    fn missing_manifest_warns_but_keeps_serving() {
        let dir = temp_dist("no-manifest");
        fs::write(dir.join("index.html"), b"<html></html>").unwrap();
        let cfg = WebConfig::new(Some(dir), None);
        assert!(!cfg.integrity().manifest_present);
        assert!(
            cfg.startup_warnings()
                .iter()
                .any(|w| w.contains("asset-manifest"))
        );
    }

    #[test]
    fn non_slash_key_is_unusable() {
        let dir = temp_dist("integrity-noslash");
        write_manifest(&dir, &[("assets/a.js", "sha384-AAAA")]);
        assert!(!WebConfig::new(Some(dir), None).integrity().manifest_present);
    }

    #[test]
    fn tampered_asset_is_mismatched_and_named_in_the_warning() {
        let dir = temp_dist("integrity-tampered");
        fs::write(dir.join("index.html"), b"<html></html>").unwrap();
        let stale = sha384_b64(b"<html>original</html>");
        let app = sha384_b64(b"console.log(1)");
        write_manifest(&dir, &[("/index.html", &stale), ("/assets/app.js", &app)]);
        let cfg = WebConfig::new(Some(dir), None);
        let report = cfg.integrity();
        assert_eq!(
            (report.manifest_present, report.checked, report.mismatched),
            (true, 2, 1)
        );
        let warnings = cfg.startup_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("integrity") && w.contains("/index.html")),
            "{warnings:?}"
        );
    }

    #[test]
    fn unsupported_manifest_algorithm_is_unusable() {
        let dir = temp_dist("integrity-algorithm");
        fs::write(
            dir.join("asset-manifest.json"),
            "{\"algorithm\":\"sha256\",\"assets\":{\"/index.html\":\"sha256-AAAA\"}}",
        )
        .unwrap();
        let cfg = WebConfig::new(Some(dir), None);
        assert!(!cfg.integrity().manifest_present);
        assert!(
            cfg.startup_warnings()
                .iter()
                .any(|w| w.contains("asset manifest")),
            "{:?}",
            cfg.startup_warnings()
        );
    }

    #[test]
    fn integrity_warning_lists_at_most_three_offending_paths() {
        let dir = temp_dist("integrity-many");
        write_manifest(
            &dir,
            &[
                ("/assets/a.js", "sha384-AAAA"),
                ("/assets/b.js", "sha384-AAAA"),
                ("/assets/c.js", "sha384-AAAA"),
                ("/assets/d.js", "sha384-AAAA"),
            ],
        );
        let cfg = WebConfig::new(Some(dir), None);
        assert_eq!(cfg.integrity().missing, 4);
        let warning = cfg
            .startup_warnings()
            .into_iter()
            .find(|w| w.contains("integrity"))
            .expect("integrity warning");
        for listed in ["/assets/a.js", "/assets/b.js", "/assets/c.js"] {
            assert!(warning.contains(listed), "{warning}");
        }
        assert!(
            !warning.contains("/assets/d.js"),
            "at most three names: {warning}"
        );
    }

    #[test]
    fn dist_absent_keeps_the_integrity_report_empty_and_silent() {
        let cfg = WebConfig::disabled();
        let report = cfg.integrity();
        assert_eq!(
            (
                report.manifest_present,
                report.checked,
                report.mismatched,
                report.missing
            ),
            (false, 0, 0, 0)
        );
        assert!(cfg.startup_warnings().is_empty());
    }

    #[test]
    fn spot_round_rotates_samples_and_counts_mismatches_once() {
        let dist = temp_dist("spot-core");
        std::fs::write(dist.join("assets/b.js"), "b").unwrap();
        std::fs::write(dist.join("assets/c.js"), "c").unwrap();
        write_manifest(
            &dist,
            &[
                ("/assets/app.js", &sha384_b64(b"console.log(1)")),
                ("/assets/b.js", &sha384_b64(b"b")),
                ("/assets/c.js", &sha384_b64(b"c")),
            ],
        );
        let web = WebConfig::new(Some(dist.clone()), None).with_spot_check(SpotCheckConfig {
            interval: Duration::from_secs(60),
            sample: 1,
        });
        assert_eq!(web.run_spot_round().checked, 1); // /assets/app.js
        std::fs::write(dist.join("assets/b.js"), "tampered").unwrap();
        let round = web.run_spot_round(); // /assets/b.js
        assert_eq!(round.mismatched, 1);
        assert_eq!(round.offenses, vec!["/assets/b.js".to_string()]);
        std::fs::remove_file(dist.join("assets/c.js")).unwrap();
        assert_eq!(web.run_spot_round().missing, 1); // /assets/c.js deleted
        assert_eq!(web.run_spot_round().checked, 1); // wraps to /assets/app.js
        let (report, age) = web.runtime_snapshot();
        assert_eq!(
            (
                report.rounds,
                report.checked,
                report.mismatched,
                report.missing
            ),
            (4, 3, 1, 1)
        );
        assert!(report.last_run.is_some());
        assert!(age.is_some());
        let text = prometheus_text(&web);
        assert!(text.contains("agentj_web_spot_rounds_total 4"));
        assert!(text.contains("agentj_web_spot_checked_total 3"));
        web.warned
            .lock()
            .unwrap()
            .insert("/assets/b.js".to_string());
        let repeated = web.run_spot_round(); // /assets/b.js again
        assert_eq!(repeated.mismatched, 1);
        assert!(repeated.offenses.is_empty());
        std::fs::remove_dir_all(&dist).ok();
    }

    #[tokio::test]
    async fn version_endpoint_reports_server_and_dist_versions() {
        let dist = temp_dist("version-route");
        std::fs::write(
            dist.join("version.json"),
            "{\"product_version\":\"0.0.1-test\",\"protocol_version\":\"0.0.1\",\"build_time\":\"2026-09-20T00:00:00Z\"}",
        ).unwrap();
        let state = test_state(crate::web::WebConfig::new(
            Some(dist.clone()),
            Some("secret".into()),
        ));
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let (s, h, b) = get(crate::web::build_router(state, remote), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert_eq!(
            h.get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(
            h.get(axum::http::header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        assert!(
            !b.contains("agentj-index"),
            "/version must not fall back to the SPA: {b}"
        );
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["product_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["protocol_version"], agentj_protocol::PROTOCOL_VERSION);
        assert_eq!(v["dist_present"], true);
        assert_eq!(v["dist"]["product_version"], "0.0.1-test");
        assert_eq!(v["dist"]["build_time"], "2026-09-20T00:00:00Z");
        std::fs::remove_dir_all(&dist).ok();
    }

    #[tokio::test]
    async fn version_endpoint_without_dist_reports_null_dist() {
        let state = test_state(crate::web::WebConfig::disabled());
        let peer: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let (s, _, b) = get(crate::web::build_router(state, peer), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["dist_present"], false);
        assert!(v["dist"].is_null());
    }

    #[tokio::test]
    async fn version_reports_login_limiter_counters() {
        let web = crate::web::WebConfig::new(None, Some("secret".into())).with_login_limiter(
            crate::login_limiter::LoginLimiter::with_policy(small_policy()),
        );
        let state = test_state(web);
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        for _ in 0..3 {
            let _ = form_post(
                crate::web::build_router(state.clone(), remote),
                "/login",
                "token=wrong&next=%2F",
            )
            .await;
        }
        let (s, _, b) = get(crate::web::build_router(state, remote), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["login_limiter"]["attempts"], 3);
        assert_eq!(v["login_limiter"]["failures"], 3);
        assert_eq!(
            v["login_limiter"]["lockouts"], 1,
            "the third failure starts the lockout"
        );
        assert_eq!(v["login_limiter"]["blocked"], 0);
    }

    #[tokio::test]
    async fn version_reports_blocked_attempts_while_locked() {
        // A long lockout makes the blocked snapshot deterministic: the second
        // request is answered from the limiter and counted as blocked.
        let policy = crate::login_limiter::Policy {
            free_attempts: 1,
            base_lockout: Duration::from_secs(600),
            max_lockout: Duration::from_secs(900),
            ..small_policy()
        };
        let web = crate::web::WebConfig::new(None, Some("secret".into()))
            .with_login_limiter(crate::login_limiter::LoginLimiter::with_policy(policy));
        let state = test_state(web);
        let remote: std::net::SocketAddr = "203.0.113.8:41000".parse().unwrap();
        for _ in 0..2 {
            let (s, _, _) = form_post(
                crate::web::build_router(state.clone(), remote),
                "/login",
                "token=wrong",
            )
            .await;
            assert_eq!(s, axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
        let (s, _, b) = get(crate::web::build_router(state, remote), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["login_limiter"]["attempts"], 2);
        assert_eq!(v["login_limiter"]["failures"], 1);
        assert_eq!(v["login_limiter"]["lockouts"], 1);
        assert_eq!(v["login_limiter"]["blocked"], 1);
        assert_eq!(v["login_limiter"]["locked_now"], 1);
        assert_eq!(v["login_limiter"]["tracked_ips"], 1);
    }

    #[tokio::test]
    async fn version_reports_asset_integrity_summary() {
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let dir = temp_dist("version-integrity");
        fs::write(dir.join("index.html"), b"<html></html>").unwrap();
        write_manifest(&dir, &[("/index.html", &sha384_b64(b"<html></html>"))]);
        let state = test_state(crate::web::WebConfig::new(Some(dir.clone()), None));
        let (s, _, b) = get(crate::web::build_router(state, remote), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["integrity"]["manifest_present"], true);
        assert!(v["integrity"]["checked"].as_u64().unwrap() > 0);
        assert_eq!(v["integrity"]["mismatched"], 0);
        assert_eq!(v["integrity"]["missing"], 0);
        std::fs::remove_dir_all(&dir).ok();

        let bare = temp_dist("version-integrity-bare");
        let state = test_state(crate::web::WebConfig::new(Some(bare.clone()), None));
        let (s, _, b) = get(crate::web::build_router(state, remote), "/version").await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["integrity"]["manifest_present"], false);
        assert_eq!(v["integrity"]["checked"], 0);
        std::fs::remove_dir_all(&bare).ok();
    }

    #[tokio::test]
    async fn version_omits_last_run_age_before_any_round() {
        let state = test_state(crate::web::WebConfig::new(None, None));
        let local: std::net::SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (_, _, body) = get(crate::web::build_router(state, local), "/version").await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let runtime = &v["integrity"]["runtime"];
        assert!(runtime.is_object(), "runtime block missing: {body}");
        assert_eq!(runtime["rounds"], 0);
        assert!(runtime.get("last_run_age_secs").is_none());
    }

    #[tokio::test]
    async fn version_reports_spot_runtime_after_a_round() {
        let dist = temp_dist("version-runtime");
        write_manifest(&dist, &[("/assets/app.js", &sha384_b64(b"console.log(1)"))]);
        let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
        let local: std::net::SocketAddr = "127.0.0.1:4444".parse().unwrap();
        state.web.run_spot_round();
        let (_, _, body) = get(crate::web::build_router(state, local), "/version").await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let runtime = &v["integrity"]["runtime"];
        assert_eq!(runtime["rounds"], 1);
        assert_eq!(runtime["checked"], 1);
        assert_eq!(runtime["mismatched"], 0);
        assert_eq!(runtime["missing"], 0);
        assert!(runtime["last_run_age_secs"].as_u64().is_some());
        assert_eq!(v["integrity"]["checked"], 1); // startup fields unchanged
        std::fs::remove_dir_all(&dist).ok();
    }

    #[tokio::test]
    async fn metrics_gates_remote_peers_and_formats_prometheus() {
        let state = test_state(crate::web::WebConfig::new(None, Some("s3cret".into())));
        let remote: std::net::SocketAddr = "203.0.113.7:5555".parse().unwrap();
        let (status, headers, body) =
            get(crate::web::build_router(state.clone(), remote), "/metrics").await;
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(
            headers.get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(!body.contains("agentj_login_attempts_total"));

        let local: std::net::SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (status, headers, body) = get(crate::web::build_router(state, local), "/metrics").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(
            headers
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/plain")
        );
        assert_eq!(
            headers.get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(body.contains("# TYPE agentj_login_attempts_total counter"));
    }

    #[tokio::test]
    async fn metrics_exposes_the_startup_integrity_summary() {
        let dist = temp_dist("metrics-integrity");
        write_temp_dist_manifest(&dist);
        let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
        let local: std::net::SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (status, _, body) = get(crate::web::build_router(state, local), "/metrics").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(body.contains("agentj_web_manifest_present 1"));
        assert!(body.contains("agentj_web_assets_checked 2"));
        std::fs::remove_dir_all(&dist).ok();
    }

    #[tokio::test]
    async fn metrics_passes_tokenless_servers_and_valid_cookies() {
        let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
        let tokenless = test_state(crate::web::WebConfig::new(None, None));
        let (status, _, _) = get(crate::web::build_router(tokenless, remote), "/metrics").await;
        assert_eq!(status, axum::http::StatusCode::OK, "no token: open");

        let web = crate::web::WebConfig::new(None, Some("secret".into()));
        let sid = web.issue_session();
        let state = test_state(web);
        use tower::util::ServiceExt;
        let res = crate::web::build_router(state, remote)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/metrics")
                    .header(axum::http::header::COOKIE, format!("agentj_session={sid}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_truncates_per_ip_series_after_locked_and_failure_ranking() {
        let policy = crate::login_limiter::Policy {
            free_attempts: 2,
            ..crate::login_limiter::Policy::default()
        };
        let limiter = crate::login_limiter::LoginLimiter::with_policy(policy);
        let t0 = Instant::now();
        for i in 1..=50u8 {
            let ip: std::net::IpAddr = format!("10.0.0.{i}").parse().unwrap();
            let _ = limiter.record_failure(ip, t0);
        }
        let locked: std::net::IpAddr = "203.0.113.9".parse().unwrap();
        let _ = limiter.record_failure(locked, t0);
        let _ = limiter.record_failure(locked, t0); // second failure locks
        let web = crate::web::WebConfig::new(None, None).with_login_limiter(limiter);
        let state = test_state(web);
        let local: std::net::SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (status, _, body) = get(crate::web::build_router(state, local), "/metrics").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(body.contains("agentj_login_ip_series_truncated 1"));
        assert_eq!(
            body.matches("agentj_login_ip_failures_total{").count(),
            PER_IP_SERIES_LIMIT
        );
        let locked_at = body.find("ip=\"203.0.113.9\"").expect("locked series");
        let busy_at = body.find("ip=\"10.0.0.1\"").expect("allowed series");
        assert!(locked_at < busy_at, "locked IPs rank first");
        assert!(body.contains("agentj_login_ip_locked{ip=\"203.0.113.9\"} 1"));
        assert!(body.contains("agentj_login_ip_last_seen_seconds{ip=\"203.0.113.9\"}"));
        assert!(body.contains("ip=\"10.0.0.49\""));
        assert!(!body.contains("ip=\"10.0.0.50\""), "51st series is dropped");
    }

    #[test]
    fn prometheus_labels_escape_quotes_backslashes_and_newlines() {
        assert_eq!(escape_label("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
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
