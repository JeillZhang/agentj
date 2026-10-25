//! Web tests for the `prometheus` domain module.

use super::super::prometheus::{escape_label, prometheus_text};
use super::super::*;
use super::support::*;
use std::time::Instant;

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
    assert!(has_metric_line(&body, "agentj_web_manifest_present 1"));
    assert!(has_metric_line(&body, "agentj_web_assets_checked 2"));
    std::fs::remove_dir_all(&dist).ok();
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
    assert!(has_metric_line(
        &body,
        "agentj_login_ip_locked{ip=\"203.0.113.9\"} 1"
    ));
    assert!(body.contains("agentj_login_ip_last_seen_seconds{ip=\"203.0.113.9\"}"));
    assert!(body.contains("ip=\"10.0.0.49\""));
    assert!(!body.contains("ip=\"10.0.0.50\""), "51st series is dropped");
}

#[test]
fn prometheus_labels_escape_quotes_backslashes_and_newlines() {
    assert_eq!(escape_label("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
}

#[tokio::test]
async fn integrity_metrics_reflect_a_request_check_and_a_sweep() {
    // Its own tag: sharing `metrics-integrity` with the startup-summary test
    // made the two delete each other's dist under a parallel run.
    let dist = temp_dist("metrics-integrity-request");
    write_manifest(&dist, &[("/assets/app.js", "sha384-AAAA")]);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    get(
        crate::web::build_router(state.clone(), local),
        "/assets/app.js",
    )
    .await;
    form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    let (_s, _, body) = get(crate::web::build_router(state.clone(), local), "/metrics").await;
    for line in [
        "agentj_web_integrity_request_checked_total 1",
        "agentj_web_integrity_request_cache_entries 1",
        "agentj_web_integrity_sweeps_total 1",
    ] {
        assert!(body.lines().any(|l| l == line), "missing {line} in\n{body}");
    }
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn prometheus_text_keeps_the_existing_metrics_and_adds_the_new_ones() {
    let web = crate::web::WebConfig::new(None, None).with_trust_proxy_headers(true);
    web.record_ip_source(crate::client_ip::IpSource::XForwardedFor, false);
    web.record_ip_source(crate::client_ip::IpSource::Peer, true);
    let out = prometheus_text(&web);
    for name in [
        "agentj_login_attempts_total",
        "agentj_login_failures_total",
        "agentj_login_lockouts_total",
        "agentj_login_blocked_total",
        "agentj_login_locked_ips",
        "agentj_login_tracked_ips",
        "agentj_web_manifest_present",
        "agentj_web_assets_checked",
        "agentj_web_assets_mismatched",
        "agentj_web_assets_missing",
        "agentj_web_spot_rounds_total",
        "agentj_web_spot_checked_total",
        "agentj_web_spot_mismatched_total",
        "agentj_web_spot_missing_total",
        "agentj_login_ip_series_truncated",
    ] {
        assert!(
            out.contains(name),
            "existing metric {name} vanished:\n{out}"
        );
    }
    for name in [
        "agentj_web_integrity_request_checked_total",
        "agentj_web_integrity_request_mismatched_total",
        "agentj_web_integrity_request_missing_total",
        "agentj_web_integrity_request_cache_entries",
        "agentj_web_integrity_sweeps_total",
        "agentj_web_integrity_sweep_checked_total",
        "agentj_web_integrity_sweep_mismatched_total",
        "agentj_web_integrity_sweep_missing_total",
    ] {
        assert!(
            out.contains(&format!("# TYPE {name} ")),
            "missing TYPE for {name}:\n{out}"
        );
        assert!(
            has_metric_line(&out, &format!("{name} 0")),
            "a fresh server must publish {name} as 0:\n{out}"
        );
    }
    assert!(
        out.contains("# TYPE agentj_web_integrity_request_cache_entries gauge"),
        "the memo size is a gauge, not a counter:\n{out}"
    );
    assert!(
        has_metric_line(
            &out,
            "agentj_web_client_ip_source_total{source=\"forwarded\"} 1"
        ),
        "{out}"
    );
    assert!(
        has_metric_line(&out, "agentj_web_client_ip_source_total{source=\"peer\"} 1"),
        "{out}"
    );
    assert!(
        has_metric_line(&out, "agentj_web_proxy_header_invalid_total 1"),
        "{out}"
    );
    assert!(
        has_metric_line(&out, "agentj_web_metrics_state_restored 0"),
        "{out}"
    );
}
