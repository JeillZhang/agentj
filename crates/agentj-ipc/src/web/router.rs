//! The HTTP router: the `/ws` upgrade plus — when a dist was resolved — the
//! statically gated SPA surface, with the hybrid-auth gate outermost and the
//! per-request integrity check innermost (design D2/D5/T5).

use super::auth::{
    LoginForm, LoginQuery, login_page_html, logout, pct_encode, peer_is_loopback, redirect_found,
    safe_next, session_cookie, submit_login,
};
use super::endpoints::{integrity_verify_handler, metrics_handler, version_handler};
use super::*;

/// Register the credential endpoints. They are deliberately **not** gated (T5
/// wraps only the static surface), so a signed-out browser cannot loop.
pub(super) fn auth_routes(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
) -> axum::Router {
    let login_get = |axum::extract::Query(q): axum::extract::Query<LoginQuery>| async move {
        axum::response::Html(login_page_html(None, &safe_next(q.next.as_deref())))
    };
    let login_post = {
        let web = web.clone();
        move |headers: axum::http::HeaderMap, form: axum::extract::Form<LoginForm>| {
            let web = web.clone();
            async move { submit_login(&web, peer, &headers, form.0).await }
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
    // D5: the loopback exemption is decided by the RAW TCP peer only. The
    // derived client IP (`client_ip::resolve_client_ip`) must never reach here —
    // otherwise a request header could grant kernel access.
    web.token.is_some()
        && !peer_is_loopback(peer)
        && !session_cookie(headers).is_some_and(|sid| web.is_valid_session(sid))
}

/// Login-gate middleware for one static mount. `nest_service` strips the mount
/// prefix before the inner service (and this layer) runs, so `mount` re-attaches
/// it — the redirect must carry the full same-origin path.
pub(super) async fn gate(
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

/// Per-request integrity gate for one static mount (D9: fail-open — the
/// verdict never changes the response, the served bytes are checked and
/// counted). `mount_prefix` re-attaches what `nest_service` stripped so the
/// manifest key can be resolved, and `fallback_key` is the SPA fallback's
/// document for non-asset paths.
pub(super) async fn verify_asset(
    web: std::sync::Arc<WebConfig>,
    mount_prefix: &'static str,
    fallback_key: Option<&'static str>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if web.verify_on_request {
        let key = web.manifest.as_ref().and_then(|manifest| {
            verify_key(manifest, mount_prefix, fallback_key, req.uri().path())
        });
        if let Some(key) = key {
            web.verify_cached_asset(&key);
        }
    }
    next.run(req).await
}

/// The `Cache-Control` override both static mounts wrap around their service —
/// the only difference between the SPA and the hashed-asset mount.
pub(super) fn cache_layer(
    value: &'static str,
) -> tower_http::set_header::SetResponseHeaderLayer<axum::http::HeaderValue> {
    tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static(value),
    )
}

/// Mount the static SPA surface (D6: cache headers + login gate wrapped around
/// each service individually — `Router::layer` would leak onto `/ws`). The root
/// mount is `fallback_service` (unknown paths -> index.html, 200) because
/// `nest_service("/")` panics.
pub(super) fn static_surface(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    dist: &std::path::Path,
) -> axum::Router {
    use tower::Layer as _;
    let spa = tower_http::services::ServeDir::new(dist).fallback(
        tower_http::services::ServeFile::new(dist.join("index.html")),
    );
    let spa = cache_layer("no-cache").layer(spa);
    let assets = cache_layer("public, max-age=31536000, immutable")
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
    let verify_assets = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { verify_asset(web, "/assets", None, req, next).await }
        })
    };
    let verify_spa = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { verify_asset(web, "", Some("/index.html"), req, next).await }
        })
    };
    // Gate outermost, verify innermost: an unauthorised request must not hash.
    router
        .nest_service("/assets", gate_assets.layer(verify_assets.layer(assets)))
        .fallback_service(gate_spa.layer(verify_spa.layer(spa)))
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
    let integrity_verify_route = {
        let web = state.web.clone();
        move |headers: axum::http::HeaderMap| {
            let web = web.clone();
            async move { integrity_verify_handler(web, peer, headers).await }
        }
    };
    let mut router = axum::Router::new()
        .route("/ws", ws_route)
        .route("/version", axum::routing::get(version_route))
        .route("/metrics", axum::routing::get(metrics_route))
        .route(
            "/integrity/verify",
            axum::routing::post(integrity_verify_route),
        );
    router = auth_routes(router, state.web.clone(), peer);
    if let Some(dist) = state.web.dist.clone() {
        router = static_surface(router, state.web.clone(), peer, &dist);
    }
    router
}
