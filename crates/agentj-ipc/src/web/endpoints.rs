//! The JSON/text endpoints mounted by the router: `/version` (server + dist
//! version, limiter counters, integrity summaries) and `POST /integrity/verify`
//! (the manual full sweep), both gated like `/metrics` (design G3/G6/G7).

use axum::response::IntoResponse as _;
use std::time::Instant;

use super::prometheus::prometheus_text;
use super::*;

/// Runtime spot-check block: rounds/checked/mismatched/missing plus the age of the
/// last sweep when one has run.
fn runtime_json(web: &WebConfig) -> serde_json::Value {
    let (runtime, last_run_age) = web.runtime_snapshot();
    let mut json = serde_json::json!({
        "rounds": runtime.rounds,
        "checked": runtime.checked,
        "mismatched": runtime.mismatched,
        "missing": runtime.missing,
    });
    if let Some(age) = last_run_age {
        json["last_run_age_secs"] = serde_json::json!(age);
    }
    json
}

/// On-demand verification block (a manual sweep over the whole manifest).
fn on_demand_json(web: &WebConfig) -> serde_json::Value {
    let (on_demand, on_demand_age) = web.on_demand_snapshot();
    let mut json = serde_json::json!({
        "sweeps": on_demand.sweeps,
        "checked": on_demand.checked,
        "mismatched": on_demand.mismatched,
        "missing": on_demand.missing,
    });
    if let Some(age) = on_demand_age {
        json["last_run_age_secs"] = serde_json::json!(age);
    }
    json
}

/// The `/version` body: product/protocol versions, the dist facts, the throttling
/// counters and the integrity summary (manifest + static + the two runtime blocks).
fn version_payload(web: &WebConfig) -> serde_json::Value {
    let dist = web.dist_version();
    let counters = web.login_limiter.counters(Instant::now());
    let integrity = web.integrity();
    serde_json::json!({
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
            "runtime": runtime_json(web),
            "on_demand": on_demand_json(web),
        },
    })
}

/// Unauthenticated version probe: reports this server's product/protocol
/// versions, the optional facts read from the mounted dist, the aggregate
/// login-limiter counters, the startup integrity summary and the runtime
/// spot-check block, so a remote client or operator can detect a stale UI and
/// observe throttling without holding a session cookie. Registered as an
/// explicit route (like `/login`), so the login gate and the SPA fallback never
/// answer it.
pub(super) async fn version_handler(web: std::sync::Arc<WebConfig>) -> axum::response::Response {
    let mut res = axum::Json(version_payload(&web)).into_response();
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    res
}

/// Auth-gated Prometheus scrape endpoint (design G2/D1): the machine surface
/// answers `401` without a redirect, so scrapers never chase the login page.
pub(super) async fn metrics_handler(
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

/// Manual full-sweep endpoint (design G3): same gate as `/metrics` — a rejected
/// remote peer gets `401` and no hashing work at all. The response is never
/// cached: it reports the sweep it just ran.
pub(super) async fn integrity_verify_handler(
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
    let outcome = web.run_full_integrity_sweep();
    let body = serde_json::json!({
        "manifest_present": outcome.manifest_present,
        "checked": outcome.checked,
        "mismatched": outcome.mismatched,
        "missing": outcome.missing,
        "offenses": outcome.offenses,
        "truncated": outcome.truncated,
        "duration_ms": outcome.duration_ms,
    })
    .to_string();
    (
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
        ],
        body,
    )
        .into_response()
}
