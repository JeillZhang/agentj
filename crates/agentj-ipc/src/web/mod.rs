//! Kernel-hosted web UI helpers (design D2/D3/D5): hybrid auth config, opaque
//! session cookies, origin/next validation, the login page, the statically gated
//! SPA surface and the `/metrics` + `/version` observability endpoints.
//!
//! Split by concern — [`integrity`] (asset manifest + verify cache + spot
//! checks), [`config`] (the shared [`WebConfig`]), [`persist`] (metrics-state
//! flush), [`auth`] (login/logout + primitives), [`router`] (gating + static
//! surface), [`endpoints`] (`/version`, `/integrity/verify`) and [`prometheus`]
//! (the text body). This root only declares them and re-exports the public API.

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

mod integrity;

use integrity::*;

pub use integrity::IntegrityOptions;
pub(crate) use integrity::IntegrityReport;

mod config;

pub use config::WebConfig;

mod persist;

mod auth;

pub use auth::{
    login_page_html, origin_matches_host, pct_encode, peer_is_loopback, safe_next, session_cookie,
};

mod router;

pub use router::{auth_required, build_router, gate_required};

mod endpoints;

mod prometheus;

#[cfg(test)]
mod tests;
