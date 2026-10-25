//! Login, logout and hybrid-auth primitive tests.

use super::super::*;
use super::support::*;
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
fn pct_encode_escapes_query_delimiters() {
    assert_eq!(pct_encode("/a b?c&d"), "/a%20b%3Fc%26d");
}

#[tokio::test]
async fn forwarded_ip_becomes_the_throttle_key() {
    let state = test_state(trusted_web());
    for _ in 0..5 {
        login_failure(&state, LOOPBACK, Some(FORWARDED_CLIENT)).await;
    }
    // A DIFFERENT forwarded client behind the same proxy is unaffected.
    login_failure(&state, LOOPBACK, Some("203.0.113.5")).await;
    let (_, _, body) = get(
        crate::web::build_router(state, LOOPBACK.parse().unwrap()),
        "/metrics",
    )
    .await;
    // free_attempts is 3, so the 3rd failure locks the entry. Attempts 4-5
    // land while locked: each still bumps its per-IP entry's failures (5),
    // but they are tallied on Counters.blocked (2) instead of
    // Counters.failures, so only 3 failures are counted globally.
    assert!(
        has_metric_line(
            &body,
            &format!("agentj_login_ip_failures_total{{ip=\"{FORWARDED_CLIENT}\"}} 5")
        ),
        "{body}"
    );
    assert!(
        has_metric_line(&body, "agentj_login_ip_locked{ip=\"198.51.100.9\"} 1"),
        "the lockout follows the forwarded client, not 127.0.0.1:\n{body}"
    );
    assert!(
        has_metric_line(&body, "agentj_login_blocked_total 2"),
        "attempts 4-5 hit the locked entry and are counted as blocked:\n{body}"
    );
    assert!(
        has_metric_line(&body, "agentj_login_failures_total 4"),
        "3 unblocked failures for the proxy client + 1 for the other client:\n{body}"
    );
    assert!(
        has_metric_line(
            &body,
            "agentj_login_ip_failures_total{ip=\"203.0.113.5\"} 1"
        ),
        "another forwarded client keeps its own budget:\n{body}"
    );
    assert!(
        !body.contains("agentj_login_ip_failures_total{ip=\"127.0.0.1\"}"),
        "loopback must not accumulate the forwarded client's failures:\n{body}"
    );
}

#[tokio::test]
async fn remote_peers_cannot_choose_their_throttle_key() {
    let state = test_state(trusted_web());
    for _ in 0..5 {
        login_failure(&state, "203.0.113.7:41000", Some("127.0.0.1")).await;
    }
    let (_, _, body) = get(
        crate::web::build_router(state, LOOPBACK.parse().unwrap()),
        "/metrics",
    )
    .await;
    // The peer's own entry keeps incrementing its failures even while
    // locked, so the per-IP series shows all 5 attempts.
    assert!(
        has_metric_line(
            &body,
            "agentj_login_ip_failures_total{ip=\"203.0.113.7\"} 5"
        ),
        "{body}"
    );
    assert!(
        !body.contains("agentj_login_ip_failures_total{ip=\"127.0.0.1\"}"),
        "{body}"
    );
    assert!(
        has_metric_line(
            &body,
            "agentj_web_client_ip_source_total{source=\"peer\"} 5"
        ),
        "a non-loopback peer always resolves to itself:\n{body}"
    );
}

#[tokio::test]
async fn forwarded_headers_cannot_buy_loopback_exemption() {
    // D5 regression: the derived IP must never reach `auth_required`.
    let state = test_state(trusted_web());
    let spoof = [("x-forwarded-for", "127.0.0.1"), ("x-real-ip", "127.0.0.1")];
    let (status, _, _) = get_with_headers(
        crate::web::build_router(state, "203.0.113.7:41000".parse().unwrap()),
        "/metrics",
        &spoof,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_header_less_loopback_request_is_not_a_failure() {
    let state = test_state(trusted_web());
    login_failure(&state, LOOPBACK, None).await;
    let (_, _, body) = get(
        crate::web::build_router(state, LOOPBACK.parse().unwrap()),
        "/metrics",
    )
    .await;
    assert!(
        has_metric_line(&body, "agentj_web_proxy_header_invalid_total 0"),
        "{body}"
    );
    assert!(
        has_metric_line(
            &body,
            "agentj_web_client_ip_source_total{source=\"peer\"} 1"
        ),
        "{body}"
    );
}

#[tokio::test]
async fn an_unusable_chain_counts_one_invalid_header() {
    let state = test_state(trusted_web());
    login_failure(&state, LOOPBACK, Some("unknown")).await;
    let (_, _, body) = get(
        crate::web::build_router(state, LOOPBACK.parse().unwrap()),
        "/metrics",
    )
    .await;
    assert!(
        has_metric_line(&body, "agentj_web_proxy_header_invalid_total 1"),
        "{body}"
    );
    assert!(
        has_metric_line(&body, "agentj_login_ip_failures_total{ip=\"127.0.0.1\"} 1"),
        "{body}"
    );
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
