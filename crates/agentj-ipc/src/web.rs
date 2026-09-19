//! Kernel-hosted web UI helpers (design D2/D3/D5): hybrid auth config,
//! opaque session cookies, origin/next validation and the login page.

/// Cookie carrying the opaque session id issued after a successful login.
pub const SESSION_COOKIE: &str = "agentj_session";

/// Static-web-UI runtime config: resolved dist directory plus the optional
/// access token and the in-memory set of issued sessions (D3, no persistence).
pub struct WebConfig {
    pub dist: Option<std::path::PathBuf>,
    pub token: Option<String>,
    sessions: std::sync::RwLock<std::collections::HashSet<String>>,
}

impl WebConfig {
    pub fn disabled() -> Self {
        Self::new(None, None)
    }
    /// Empty-string tokens normalize to `None` (no auth configured).
    pub fn new(dist: Option<std::path::PathBuf>, token: Option<String>) -> Self {
        Self {
            dist,
            token: token.filter(|t| !t.is_empty()),
            sessions: std::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }
    /// The static web UI is served only when a dist directory was resolved.
    pub fn enabled(&self) -> bool {
        self.dist.is_some()
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
    peer.ip().is_loopback()
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
        Some(n) if n.starts_with('/') && !n.starts_with("//") => n.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_next_whitelists_same_site_paths_only() {
        assert_eq!(safe_next(None), "/");
        assert_eq!(safe_next(Some("/ok?x=1")), "/ok?x=1");
        assert_eq!(safe_next(Some("//evil")), "/");
        assert_eq!(safe_next(Some("https://evil")), "/");
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
}
