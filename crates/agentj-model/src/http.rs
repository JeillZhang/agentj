//! Shared HTTP client and header rules for model providers (design D4).
//!
//! All providers build their `reqwest::Client` through [`client`] so the compliance
//! `User-Agent` is consistent. `x-opencode-session` is emitted only by the opencode-go
//! transports, and only when a per-conversation `session_id` is present.

use agentj_core::types::ModelOptions;

/// Compliance identity sent by every provider request (no impersonation — design D4).
pub const USER_AGENT: &str = concat!("agentj/", env!("CARGO_PKG_VERSION"));

/// A reqwest client carrying the agentj `User-Agent`.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("reqwest client")
}

/// `(header name, value)` when the request carries a per-conversation session id.
///
/// Rule: the header is emitted iff the caller configured a header name AND the
/// options carry a `session_id`.
pub fn session_header<'a>(name: &'a str, options: &'a ModelOptions) -> Option<(&'a str, &'a str)> {
    options.session_id.as_deref().map(|sid| (name, sid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::types::ModelOptions;

    #[test]
    fn session_header_present_only_when_session_id_set() {
        let opts = ModelOptions {
            session_id: Some("s1".into()),
            ..Default::default()
        };
        assert_eq!(
            session_header("x-opencode-session", &opts),
            Some(("x-opencode-session", "s1"))
        );
        assert_eq!(
            session_header("x-opencode-session", &ModelOptions::default()),
            None
        );
    }

    #[test]
    fn user_agent_is_agentj_versioned() {
        assert_eq!(USER_AGENT, concat!("agentj/", env!("CARGO_PKG_VERSION")));
    }
}
