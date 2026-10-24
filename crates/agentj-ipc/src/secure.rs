//! Login-surface security primitives: constant-time secret comparison and the
//! single source of truth for the session cookie's attributes.
//!
//! Both used to be inline in `web.rs` (a `String ==` compare and two hand-written
//! `format!` cookie strings). Keeping them here makes the attributes impossible to
//! diverge (login vs. logout) and lets the comparison be unit-tested on its own.

use axum::http::HeaderValue;

use crate::web::SESSION_COOKIE;

/// Compare two secrets without short-circuiting on the first differing byte.
///
/// Length is not treated as secret (tokens are fixed-format), but a length
/// mismatch is never skipped: the byte fold always runs over the full common
/// prefix, so timing does not reveal *where* the comparison failed.
pub(crate) fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let len_eq = a.len() == b.len();
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    len_eq & (diff == 0)
}

/// `Set-Cookie` for a freshly issued session (`Secure` only when the deployment
/// terminates TLS, i.e. when `secure` is set).
pub(crate) fn session_cookie_header(sid: &str, secure: bool) -> Option<HeaderValue> {
    cookie_header(sid, secure, None)
}

/// `Set-Cookie` that clears the session cookie. Attributes must match the
/// issuing one (including `Secure`), otherwise browsers keep the stale cookie.
pub(crate) fn clear_cookie_header(secure: bool) -> Option<HeaderValue> {
    cookie_header("", secure, Some(0))
}

fn cookie_header(sid: &str, secure: bool, max_age: Option<u32>) -> Option<HeaderValue> {
    let mut value = format!("{SESSION_COOKIE}={sid}; HttpOnly; SameSite=Lax; Path=/");
    if secure {
        value.push_str("; Secure");
    }
    if let Some(secs) = max_age {
        value.push_str(&format!("; Max-Age={secs}"));
    }
    match HeaderValue::from_str(&value) {
        Ok(header) => Some(header),
        Err(e) => {
            tracing::error!(error = %e, "cannot build the session cookie header");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_only_equal_strings() {
        assert!(ct_eq("abc", "abc"));
        assert!(ct_eq("", ""));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "ab"));
        assert!(!ct_eq("ab", "abc"));
        assert!(!ct_eq("", "a"));
        assert!(!ct_eq("token-prefix-longer", "token-prefix"));
    }

    #[test]
    fn session_cookie_attributes() {
        let insecure = session_cookie_header("sid123", false).expect("valid header");
        assert_eq!(
            insecure.to_str().unwrap(),
            "agentj_session=sid123; HttpOnly; SameSite=Lax; Path=/"
        );
        let secure = session_cookie_header("sid123", true).expect("valid header");
        assert_eq!(
            secure.to_str().unwrap(),
            "agentj_session=sid123; HttpOnly; SameSite=Lax; Path=/; Secure"
        );
    }

    #[test]
    fn clear_cookie_mirrors_the_session_attributes() {
        let insecure = clear_cookie_header(false).expect("valid header");
        assert_eq!(
            insecure.to_str().unwrap(),
            "agentj_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
        );
        let secure = clear_cookie_header(true).expect("valid header");
        assert_eq!(
            secure.to_str().unwrap(),
            "agentj_session=; HttpOnly; SameSite=Lax; Path=/; Secure; Max-Age=0"
        );
    }
}
