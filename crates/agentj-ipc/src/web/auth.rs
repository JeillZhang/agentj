//! Hybrid-auth primitives: loopback detection, the client-IP derivation, the
//! session cookie and origin/`next` validation, plus the `/login` + `/logout`
//! handlers and their throttling responses (design D2/D3).

use axum::response::IntoResponse as _;
use std::time::Instant;

use crate::login_limiter::Throttle;

use super::*;

/// Whether the request peer is a loopback address (hybrid auth exemption, D2).
pub fn peer_is_loopback(peer: std::net::SocketAddr) -> bool {
    crate::client_ip::is_loopback_ip(peer.ip())
}

/// Derive the client IP used for identification (design D2/D5). Tally the source
/// as a side effect. `invalid` is true only when trust applied, the request
/// carried a proxy header and nothing usable came out of it.
pub(super) fn client_ip_for(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> crate::client_ip::ClientIp {
    let trust = web.trust_proxy_headers() && peer_is_loopback(peer);
    let resolved = crate::client_ip::resolve_client_ip(peer, headers, trust);
    let invalid = trust
        && resolved.source == crate::client_ip::IpSource::Peer
        && crate::client_ip::has_proxy_header(headers);
    web.record_ip_source(resolved.source, invalid);
    resolved
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

pub(super) fn escape_html(s: &str) -> String {
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
pub(super) struct LoginQuery {
    pub(super) next: Option<String>,
}

#[derive(serde::Deserialize)]
pub(super) struct LoginForm {
    token: String,
    next: Option<String>,
}

pub(super) fn redirect_found(location: &str) -> axum::response::Response {
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
pub(super) async fn submit_login(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
    form: LoginForm,
) -> axum::response::Response {
    let now = Instant::now();
    // Identification only: the throttle key follows the derived client IP, while
    // the auth exemption below keeps the raw peer (design D5).
    let ip = client_ip_for(web, peer, headers).ip;
    if matches!(web.login_limiter.check(ip, now), Throttle::Locked { .. }) {
        let throttle = web.login_limiter.record_failure(ip, now);
        return throttle_response(throttle, form.next.as_deref());
    }
    let expected = web.token.as_deref().unwrap_or_default();
    if !expected.is_empty() && crate::secure::ct_eq(&form.token, expected) {
        web.login_limiter.record_success(ip);
        let sid = web.issue_session();
        let mut res = redirect_found(&safe_next(form.next.as_deref()));
        if let Some(cookie) = crate::secure::session_cookie_header(&sid, web.secure_cookies) {
            res.headers_mut()
                .insert(axum::http::header::SET_COOKIE, cookie);
        }
        res
    } else {
        let throttle = web.login_limiter.record_failure(ip, now);
        throttle_response(throttle, form.next.as_deref())
    }
}

pub(super) fn unauthorized_response(next: Option<&str>) -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::response::Html(login_page_html(Some("访问令牌无效"), &safe_next(next))),
    )
        .into_response()
}

/// `Locked` answers 429 with the remaining wait (whole seconds, min 1, ceil)
/// and no cookie; an `Allow` throttle is just the plain 401 page.
pub(super) fn throttle_response(
    throttle: Throttle,
    next: Option<&str>,
) -> axum::response::Response {
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
    match axum::http::HeaderValue::from_str(&secs.to_string()) {
        Ok(retry_after) => {
            res.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, retry_after);
        }
        Err(e) => tracing::error!(error = %e, "cannot build the Retry-After header"),
    }
    res
}

/// Revoke the caller's session if present and clear the cookie (`Max-Age=0`).
pub(super) async fn logout(
    web: &WebConfig,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    if let Some(sid) = session_cookie(&headers) {
        web.revoke_session(sid);
    }
    let mut res = redirect_found("/login");
    if let Some(cookie) = crate::secure::clear_cookie_header(web.secure_cookies) {
        res.headers_mut()
            .insert(axum::http::header::SET_COOKIE, cookie);
    }
    res
}
