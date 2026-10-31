//! Web tests for the `endpoints` domain module.

use super::super::*;
use super::support::*;
use std::fs;
use std::time::Duration;

#[tokio::test]
async fn version_reports_an_empty_config_warning_array_when_none_were_injected() {
    let state = test_state(WebConfig::disabled());
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (_, _, body) = get(crate::web::build_router(state, local), "/version").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["config_warnings"], serde_json::json!([]));
}

#[tokio::test]
async fn version_reports_config_warnings_verbatim() {
    let web = WebConfig::disabled().with_config_warnings(vec![
        "warning: agent.max_tool_iterations = 5000 outside 1..=1000; using 1000".to_string(),
        "warning: [web] state_dir is empty; treating it as unset".to_string(),
    ]);
    let state = test_state(web);
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, body) = get(crate::web::build_router(state, local), "/version").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let warnings = v["config_warnings"].as_array().expect("array, never null");
    assert_eq!(warnings.len(), 2);
    assert_eq!(
        warnings[0].as_str().unwrap(),
        "warning: agent.max_tool_iterations = 5000 outside 1..=1000; using 1000"
    );
}


#[tokio::test]
async fn version_omits_the_on_demand_age_before_any_sweep() {
    let dist = temp_dist("version-ondemand");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, body) = get(crate::web::build_router(state.clone(), local), "/version").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["integrity"]["on_demand"]["sweeps"], serde_json::json!(0));
    assert_eq!(v["integrity"]["on_demand"]["checked"], serde_json::json!(0));
    assert!(
        v["integrity"]["on_demand"]
            .get("last_run_age_secs")
            .is_none()
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn version_reports_on_demand_after_a_sweep() {
    let dist = temp_dist("version-ondemand-after");
    write_temp_dist_manifest(&dist);
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    let (_s, _, body) = get(crate::web::build_router(state.clone(), local), "/version").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["integrity"]["on_demand"]["sweeps"], serde_json::json!(1));
    assert_eq!(
        v["integrity"]["on_demand"]["mismatched"],
        serde_json::json!(1)
    );
    assert!(
        v["integrity"]["on_demand"]["last_run_age_secs"]
            .as_u64()
            .is_some()
    );
    assert_eq!(
        v["integrity"]["runtime"]["rounds"],
        serde_json::json!(0),
        "the counters never mix"
    );
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
