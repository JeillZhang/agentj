//! Web tests for the `router` domain module.

use super::support::*;

#[tokio::test]
async fn metrics_passes_tokenless_servers_and_valid_cookies() {
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let tokenless = test_state(crate::web::WebConfig::new(None, None));
    let (status, _, _) = get(crate::web::build_router(tokenless, remote), "/metrics").await;
    assert_eq!(status, axum::http::StatusCode::OK, "no token: open");

    let web = crate::web::WebConfig::new(None, Some("secret".into()));
    let sid = web.issue_session();
    let state = test_state(web);
    use tower::util::ServiceExt;
    let res = crate::web::build_router(state, remote)
        .oneshot(
            axum::http::Request::builder()
                .uri("/metrics")
                .header(axum::http::header::COOKIE, format!("agentj_session={sid}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn remote_peer_without_cookie_cannot_read_the_config_warning_count() {
    let web = crate::web::WebConfig::new(None, Some("secret".into())).with_config_warnings(vec![
        "warning: agent.max_tool_iterations = 5000 outside 1..=1000; using 1000".to_string(),
    ]);
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let (status, _, body) = get(
        crate::web::build_router(test_state(web), remote),
        "/metrics",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert!(!body.contains("agentj_web_config_warnings"), "{body}");
}

#[tokio::test]
async fn serves_index_assets_and_spa_fallback_with_cache_headers() {
    let dist = temp_dist("static");
    let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
    let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
    let (s, h, b) = get(crate::web::build_router(state.clone(), peer), "/").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    assert!(b.contains("agentj-index"));
    assert_eq!(
        h.get(axum::http::header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
    let (s, h, _) = get(
        crate::web::build_router(state.clone(), peer),
        "/assets/app.js",
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK);
    assert!(
        h.get(axum::http::header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let (s, _, b) = get(crate::web::build_router(state, peer), "/unknown-route").await;
    assert_eq!(s, axum::http::StatusCode::OK, "SPA fallback must be 200");
    assert!(b.contains("agentj-index"));
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn missing_dist_returns_404() {
    let state = test_state(crate::web::WebConfig::disabled());
    let peer: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
    let (s, _, _) = get(crate::web::build_router(state, peer), "/").await;
    assert_eq!(s, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn remote_peer_without_cookie_is_redirected_to_login() {
    let dist = temp_dist("gate");
    let state = test_state(crate::web::WebConfig::new(
        Some(dist.clone()),
        Some("secret".into()),
    ));
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let (s, h, _) = get(crate::web::build_router(state.clone(), remote), "/").await;
    assert_eq!(s, axum::http::StatusCode::FOUND);
    assert_eq!(
        h.get(axum::http::header::LOCATION).unwrap(),
        "/login?next=/"
    );
    let (s, _, _) = get(crate::web::build_router(state.clone(), remote), "/login").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn loopback_and_cookie_paths_pass_the_gate() {
    let dist = temp_dist("gate-pass");
    let web = crate::web::WebConfig::new(Some(dist.clone()), Some("secret".into()));
    let sid = web.issue_session();
    let state = test_state(web);
    let local: std::net::SocketAddr = "127.0.0.1:5555".parse().unwrap();
    let (s, _, _) = get(crate::web::build_router(state.clone(), local), "/").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    use tower::util::ServiceExt;
    let res = crate::web::build_router(state, remote)
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
                .header(axum::http::header::COOKIE, format!("agentj_session={sid}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn forged_cookie_and_asset_requests_are_gated() {
    let dist = temp_dist("gate-forged");
    let state = test_state(crate::web::WebConfig::new(
        Some(dist.clone()),
        Some("secret".into()),
    ));
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    use tower::util::ServiceExt;
    let res = crate::web::build_router(state, remote)
        .oneshot(
            axum::http::Request::builder()
                .uri("/assets/app.js")
                .header(axum::http::header::COOKIE, "agentj_session=forged")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        axum::http::StatusCode::FOUND,
        "forged cookie must redirect"
    );
    assert_eq!(
        res.headers().get(axum::http::header::LOCATION).unwrap(),
        "/login?next=/assets/app.js"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn no_token_remote_peer_passes_without_login() {
    let dist = temp_dist("gate-notoken");
    let state = test_state(crate::web::WebConfig::new(Some(dist.clone()), None));
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let (s, _, _) = get(crate::web::build_router(state, remote), "/").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    std::fs::remove_dir_all(&dist).ok();
}
