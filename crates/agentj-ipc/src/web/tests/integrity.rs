//! Web tests for this domain module.

use super::super::prometheus::prometheus_text;
use super::super::*;
use super::support::*;
use agentj_core::sync::lock_or_recover;
use std::fs;
use std::time::Duration;

#[test]
fn integrity_options_defaults_match_the_documented_values() {
    let opts = IntegrityOptions::default();
    assert_eq!(opts.spot_interval, Duration::from_secs(60));
    assert_eq!(opts.spot_sample, 8);
    assert!(opts.verify_on_request);
    assert_eq!(opts.verify_cache_ttl, Duration::from_secs(60));
}

#[tokio::test]
async fn per_request_check_hashes_the_served_asset() {
    let dist = temp_dist("req-asset");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, _) = get(
        crate::web::build_router(state.clone(), local),
        "/assets/app.js",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(state.web.request_integrity_snapshot().checked, 1);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn per_request_check_skips_unknown_asset_paths() {
    let dist = temp_dist("req-unknown");
    let digest = sha384_b64(b"console.log(1)");
    write_manifest(&dist, &[("/app.js", digest.as_str())]); // no /assets prefix
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    // Not in the manifest and not on disk: the v0.19.0 404, and never the SPA fallback.
    let (missing, _, body) = get(
        crate::web::build_router(state.clone(), local),
        "/assets/typo.js",
    )
    .await;
    assert_eq!(missing, axum::http::StatusCode::NOT_FOUND);
    assert!(
        !body.contains("agentj"),
        "the assets face must not fall back to index.html"
    );
    // On disk but keyed differently in the manifest: served, still unhashed.
    let (served, _, _) = get(
        crate::web::build_router(state.clone(), local),
        "/assets/app.js",
    )
    .await;
    assert_eq!(served, axum::http::StatusCode::OK);
    assert_eq!(state.web.request_integrity_snapshot().checked, 0);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn per_request_check_verifies_the_spa_fallback_key() {
    let dist = temp_dist("req-spa");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, _) = get(crate::web::build_router(state.clone(), local), "/sessions").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(state.web.request_integrity_snapshot().checked, 1);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn per_request_check_is_off_when_configured_off() {
    let dist = temp_dist("req-off");
    write_temp_dist_manifest(&dist);
    let web = WebConfig::new(Some(dist.clone()), None).with_integrity_options(IntegrityOptions {
        verify_on_request: false,
        ..IntegrityOptions::default()
    });
    let state = test_state(web);
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, _) = get(
        crate::web::build_router(state.clone(), local),
        "/assets/app.js",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(state.web.request_integrity_snapshot().checked, 0);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn per_request_check_never_runs_for_a_rejected_remote_request() {
    let dist = temp_dist("req-remote");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), Some("s3cret".into())));
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let (status, headers, _) = get(
        crate::web::build_router(state.clone(), remote),
        "/assets/app.js",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::FOUND);
    assert!(headers.get(axum::http::header::LOCATION).is_some());
    assert_eq!(state.web.request_integrity_snapshot().checked, 0);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn per_request_check_is_a_noop_without_a_manifest() {
    let dist = temp_dist("req-no-manifest");
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, _) = get(
        crate::web::build_router(state.clone(), local),
        "/assets/app.js",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(state.web.request_integrity_snapshot().checked, 0);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_reports_a_full_sweep() {
    let dist = temp_dist("sweep");
    write_temp_dist_manifest(&dist);
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, headers, body) = form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        headers
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(
        headers.get(axum::http::header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["manifest_present"], serde_json::json!(true));
    assert_eq!(v["mismatched"], serde_json::json!(1));
    assert_eq!(v["offenses"], serde_json::json!(["/assets/app.js"]));
    assert_eq!(v["truncated"], serde_json::json!(false));
    assert!(v["duration_ms"].as_u64().is_some());
    assert_eq!(state.web.on_demand_snapshot().0.sweeps, 1);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_counts_a_missing_file_without_incrementing_checked() {
    let dist = temp_dist("sweep-missing");
    let good = sha384_b64(b"console.log(1)");
    let gone = sha384_b64(b"absent");
    write_manifest(
        &dist,
        &[
            ("/assets/app.js", good.as_str()),
            ("/assets/gone.js", gone.as_str()),
        ],
    );
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (_s, _, body) = form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["checked"], serde_json::json!(1));
    assert_eq!(v["mismatched"], serde_json::json!(0));
    assert_eq!(v["missing"], serde_json::json!(1));
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_is_gated_like_metrics() {
    let dist = temp_dist("sweep-gate");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), Some("s3cret".into())));
    let remote: std::net::SocketAddr = "203.0.113.7:41000".parse().unwrap();
    let (status, headers, body) = form_post(
        crate::web::build_router(state.clone(), remote),
        "/integrity/verify",
        "",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(body, "unauthorized\n");
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
    assert!(
        headers.get(axum::http::header::LOCATION).is_none(),
        "no redirect"
    );
    assert_eq!(state.web.on_demand_snapshot().0.sweeps, 0);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_rejects_get_with_405() {
    let dist = temp_dist("sweep-405");
    write_temp_dist_manifest(&dist);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (status, _, _) = get(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::METHOD_NOT_ALLOWED);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_without_a_manifest_does_not_count_a_sweep() {
    let dist = temp_dist("sweep-no-manifest");
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (_s, _, body) = form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["manifest_present"], serde_json::json!(false));
    assert_eq!(v["checked"], serde_json::json!(0));
    assert_eq!(
        state.web.on_demand_snapshot().0.sweeps,
        0,
        "no work, no sweep count"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_counts_an_empty_manifest_as_present_and_swept() {
    let dist = temp_dist("sweep-empty");
    write_manifest(&dist, &[]);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    for _ in 0..2 {
        let (_s, _, body) = form_post(
            crate::web::build_router(state.clone(), local),
            "/integrity/verify",
            "",
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["manifest_present"], serde_json::json!(true));
        assert_eq!(v["checked"], serde_json::json!(0));
    }
    assert_eq!(state.web.on_demand_snapshot().0.sweeps, 2);
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn integrity_verify_truncates_offenses_but_keeps_counts_exact() {
    let dist = temp_dist("sweep-truncate");
    let keys: Vec<String> = (0..55).map(|i| format!("/assets/missing-{i}.js")).collect();
    let digest = sha384_b64(b"absent");
    let entries: Vec<(&str, &str)> = keys.iter().map(|k| (k.as_str(), digest.as_str())).collect();
    write_manifest(&dist, &entries);
    let state = test_state(WebConfig::new(Some(dist.clone()), None));
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (_s, _, body) = form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let offenses = v["offenses"].as_array().unwrap();
    assert_eq!(offenses.len(), INTEGRITY_OFFENSES_LIMIT);
    assert_eq!(v["truncated"], serde_json::json!(true));
    assert_eq!(v["missing"], serde_json::json!(55), "counts stay exact");
    let paths: Vec<&str> = offenses.iter().filter_map(|o| o.as_str()).collect();
    assert_eq!(paths.len(), offenses.len());
    assert!(
        paths.windows(2).all(|pair| pair[0] <= pair[1]),
        "offenses are sorted: {paths:?}"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn a_swept_offense_is_not_rewarned_by_a_spot_round() {
    let dist = temp_dist("sweep-dedup");
    let wrong = sha384_b64(b"not the real digest");
    write_manifest(&dist, &[("/assets/app.js", wrong.as_str())]);
    let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
    let (buf, _guard) = capture_warnings();

    // Control: without a preceding sweep the round reports the offense.
    let fresh = WebConfig::new(Some(dist.clone()), None).with_spot_check(msec_spot(20, 1));
    assert_eq!(
        fresh.run_spot_round().offenses,
        vec!["/assets/app.js".to_string()]
    );

    // After a sweep has warned once, the shared `warned` set suppresses it.
    let state =
        test_state(WebConfig::new(Some(dist.clone()), None).with_spot_check(msec_spot(20, 1)));
    let (status, _, _) = form_post(
        crate::web::build_router(state.clone(), local),
        "/integrity/verify",
        "",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let round = state.web.run_spot_round();
    assert_eq!(round.mismatched, 1, "the round still counts the offense");
    assert!(
        round.offenses.is_empty(),
        "no second warning for a known path"
    );
    let out = String::from_utf8(lock_or_recover(&buf.0).clone()).unwrap_or_default();
    assert_eq!(out.matches("/assets/app.js").count(), 1, "{out}");
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn manifest_key_for_reattaches_the_stripped_mount_prefix() {
    assert_eq!(manifest_key_for("/assets", "/app.js"), "/assets/app.js");
    assert_eq!(manifest_key_for("", "/index.html"), "/index.html");
}

#[test]
fn verify_key_prefers_the_exact_key_then_the_fallback() {
    let dist = temp_dist("verify-key");
    let digest = sha384_b64(b"console.log(1)");
    write_manifest(
        &dist,
        &[
            ("/assets/app.js", digest.as_str()),
            ("/index.html", digest.as_str()),
        ],
    );
    let web = WebConfig::new(Some(dist.clone()), None);
    let manifest = web.manifest.as_ref().expect("usable manifest");
    assert_eq!(
        verify_key(manifest, "/assets", None, "/app.js").as_deref(),
        Some("/assets/app.js"),
    );
    assert_eq!(verify_key(manifest, "/assets", None, "/typo.js"), None);
    assert_eq!(
        verify_key(manifest, "", Some("/index.html"), "/sessions").as_deref(),
        Some("/index.html"),
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn verify_cached_asset_memoizes_within_an_absolute_ttl() {
    let dist = temp_dist("memo");
    write_temp_dist_manifest(&dist);
    let web = WebConfig::new(Some(dist.clone()), None).with_integrity_options(IntegrityOptions {
        verify_cache_ttl: Duration::from_secs(60),
        ..IntegrityOptions::default()
    });
    web.verify_cached_asset("/assets/app.js");
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    web.verify_cached_asset("/assets/app.js");
    assert_eq!(web.request_integrity_snapshot().checked, 1);
    assert_eq!(web.request_integrity_snapshot().mismatched, 0);
    assert!(
        web.verify_cache_verdict("/assets/app.js")
            .is_some_and(|v| v == AssetCheck::Match)
    );
}

#[test]
fn verify_cached_asset_rechecks_after_the_absolute_ttl_expires() {
    let dist = temp_dist("ttl");
    write_temp_dist_manifest(&dist);
    // `with_raw_verify_cache_ttl` bypasses the 1s production clamp so this stays fast.
    let web = WebConfig::new(Some(dist.clone()), None)
        .with_raw_verify_cache_ttl(Duration::from_millis(1));
    web.verify_cached_asset("/assets/app.js");
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    std::thread::sleep(Duration::from_millis(5));
    web.verify_cached_asset("/assets/app.js");
    let snapshot = web.request_integrity_snapshot();
    assert_eq!((snapshot.checked, snapshot.mismatched), (2, 1));
}

#[test]
fn verify_cache_entries_stay_within_the_bound() {
    let dist = temp_dist("bound");
    let keys: Vec<String> = (0..VERIFY_CACHE_MAX + 8)
        .map(|i| format!("/assets/generated-{i}.js"))
        .collect();
    let digest = sha384_b64(b"x");
    let entries: Vec<(&str, &str)> = keys
        .iter()
        .map(|key| (key.as_str(), digest.as_str()))
        .collect();
    write_manifest(&dist, &entries);
    // Sub-second TTL: every insertion already sees expired entries, so the
    // retain-then-clear branch inside `store_verify_entry` runs.
    let web = WebConfig::new(Some(dist.clone()), None)
        .with_raw_verify_cache_ttl(Duration::from_millis(1));
    for key in &keys {
        web.verify_cached_asset(key);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        web.request_integrity_snapshot().cache_entries <= VERIFY_CACHE_MAX,
        "the memo must stay bounded"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn verify_cached_asset_warns_once_per_path_across_expiries() {
    let dist = temp_dist("warn-once");
    write_temp_dist_manifest(&dist);
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    let web = WebConfig::new(Some(dist.clone()), None)
        .with_raw_verify_cache_ttl(Duration::from_millis(1));
    let (buf, _guard) = capture_warnings();
    for _ in 0..3 {
        web.verify_cached_asset("/assets/app.js");
        std::thread::sleep(Duration::from_millis(3));
    }
    let out = String::from_utf8(lock_or_recover(&buf.0).clone()).unwrap_or_default();
    assert_eq!(
        out.matches("web asset integrity check failed").count(),
        1,
        "{out}"
    );
    assert_eq!(out.matches("/assets/app.js").count(), 1, "{out}");
    let snapshot = web.request_integrity_snapshot();
    assert_eq!(
        (snapshot.checked, snapshot.mismatched),
        (3, 3),
        "warning de-duplication must not touch the counters"
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn verify_cached_asset_survives_poisoned_locks() {
    let dist = temp_dist("poison");
    write_temp_dist_manifest(&dist);
    let web = std::sync::Arc::new(WebConfig::new(Some(dist.clone()), None));
    let poisoned = std::sync::Arc::clone(&web);
    let _ = std::thread::spawn(move || {
        let _cache = lock_or_recover(&poisoned.verify_cache);
        let _warned = lock_or_recover(&poisoned.warned);
        panic!("poison both locks");
    })
    .join();
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    web.verify_cached_asset("/assets/app.js"); // must not panic
    assert_eq!(web.request_integrity_snapshot().mismatched, 1);
    assert_eq!(
        web.verify_cache_verdict("/assets/app.js"),
        Some(AssetCheck::Mismatch),
        "a poisoned memo stays readable and usable"
    );
    web.verify_cached_asset("/assets/app.js"); // still a hit, still no panic
    assert_eq!(web.request_integrity_snapshot().mismatched, 1);
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn verify_cached_asset_counts_a_missing_key_without_counting_a_check() {
    let dist = temp_dist("counts-missing");
    let gone = sha384_b64(b"absent");
    write_manifest(&dist, &[("/assets/gone.js", gone.as_str())]);
    let web = WebConfig::new(Some(dist.clone()), None);
    web.verify_cached_asset("/assets/gone.js");
    let s = web.request_integrity_snapshot();
    assert_eq!((s.checked, s.mismatched, s.missing), (0, 0, 1));
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn verify_cached_asset_counts_a_fresh_mismatch_as_a_check() {
    let dist = temp_dist("counts-mismatch");
    write_temp_dist_manifest(&dist);
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    let web = WebConfig::new(Some(dist.clone()), None);
    web.verify_cached_asset("/assets/app.js");
    let s = web.request_integrity_snapshot();
    assert_eq!((s.checked, s.mismatched, s.missing), (1, 1, 0));
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn concurrent_cold_cache_checks_stay_correct() {
    let dist = temp_dist("concurrent");
    write_temp_dist_manifest(&dist);
    std::fs::write(dist.join("assets/app.js"), "tampered").unwrap();
    let web = std::sync::Arc::new(WebConfig::new(Some(dist.clone()), None));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let web = std::sync::Arc::clone(&web);
            std::thread::spawn(move || web.verify_cached_asset("/assets/app.js"))
        })
        .collect();
    for handle in handles {
        handle.join().expect("no thread may panic");
    }
    let snapshot = web.request_integrity_snapshot();
    assert!(
        snapshot.mismatched >= 1,
        "at least one hash reports the mismatch"
    );
    assert_eq!(
        snapshot.cache_entries, 1,
        "one key -> exactly one cache entry"
    );
    assert_eq!(
        web.verify_cache_verdict("/assets/app.js"),
        Some(AssetCheck::Mismatch)
    );
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn integrity_report_counts_mismatches_and_missing() {
    let dir = temp_dist("integrity");
    fs::write(dir.join("index.html"), b"<html></html>").unwrap();
    let good = sha384_b64(b"<html></html>");
    write_manifest(
        &dir,
        &[
            ("/index.html", &good),
            ("/assets/missing.js", "sha384-AAAA"),
        ],
    );
    let cfg = WebConfig::new(Some(dir), None);
    let report = cfg.integrity();
    assert_eq!(
        (report.manifest_present, report.checked, report.missing),
        (true, 1, 1)
    );
    assert!(
        cfg.startup_warnings()
            .iter()
            .any(|w| w.contains("integrity"))
    );
}

#[test]
fn manifest_with_parent_dir_key_is_unusable() {
    let dir = temp_dist("integrity-bad");
    write_manifest(&dir, &[("/../etc/passwd", "sha384-AAAA")]);
    let cfg = WebConfig::new(Some(dir), None);
    assert!(!cfg.integrity().manifest_present);
    assert!(
        cfg.startup_warnings()
            .iter()
            .any(|w| w.contains("asset manifest")),
        "{:?}",
        cfg.startup_warnings()
    );
}

#[test]
fn missing_manifest_warns_but_keeps_serving() {
    let dir = temp_dist("no-manifest");
    fs::write(dir.join("index.html"), b"<html></html>").unwrap();
    let cfg = WebConfig::new(Some(dir), None);
    assert!(!cfg.integrity().manifest_present);
    assert!(
        cfg.startup_warnings()
            .iter()
            .any(|w| w.contains("asset-manifest"))
    );
}

#[test]
fn non_slash_key_is_unusable() {
    let dir = temp_dist("integrity-noslash");
    write_manifest(&dir, &[("assets/a.js", "sha384-AAAA")]);
    assert!(!WebConfig::new(Some(dir), None).integrity().manifest_present);
}

#[test]
fn tampered_asset_is_mismatched_and_named_in_the_warning() {
    let dir = temp_dist("integrity-tampered");
    fs::write(dir.join("index.html"), b"<html></html>").unwrap();
    let stale = sha384_b64(b"<html>original</html>");
    let app = sha384_b64(b"console.log(1)");
    write_manifest(&dir, &[("/index.html", &stale), ("/assets/app.js", &app)]);
    let cfg = WebConfig::new(Some(dir), None);
    let report = cfg.integrity();
    assert_eq!(
        (report.manifest_present, report.checked, report.mismatched),
        (true, 2, 1)
    );
    let warnings = cfg.startup_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("integrity") && w.contains("/index.html")),
        "{warnings:?}"
    );
}

#[test]
fn unsupported_manifest_algorithm_is_unusable() {
    let dir = temp_dist("integrity-algorithm");
    fs::write(
        dir.join("asset-manifest.json"),
        "{\"algorithm\":\"sha256\",\"assets\":{\"/index.html\":\"sha256-AAAA\"}}",
    )
    .unwrap();
    let cfg = WebConfig::new(Some(dir), None);
    assert!(!cfg.integrity().manifest_present);
    assert!(
        cfg.startup_warnings()
            .iter()
            .any(|w| w.contains("asset manifest")),
        "{:?}",
        cfg.startup_warnings()
    );
}

#[test]
fn integrity_warning_lists_at_most_three_offending_paths() {
    let dir = temp_dist("integrity-many");
    write_manifest(
        &dir,
        &[
            ("/assets/a.js", "sha384-AAAA"),
            ("/assets/b.js", "sha384-AAAA"),
            ("/assets/c.js", "sha384-AAAA"),
            ("/assets/d.js", "sha384-AAAA"),
        ],
    );
    let cfg = WebConfig::new(Some(dir), None);
    assert_eq!(cfg.integrity().missing, 4);
    let warning = cfg
        .startup_warnings()
        .into_iter()
        .find(|w| w.contains("integrity"))
        .expect("integrity warning");
    for listed in ["/assets/a.js", "/assets/b.js", "/assets/c.js"] {
        assert!(warning.contains(listed), "{warning}");
    }
    assert!(
        !warning.contains("/assets/d.js"),
        "at most three names: {warning}"
    );
}

#[test]
fn dist_absent_keeps_the_integrity_report_empty_and_silent() {
    let cfg = WebConfig::disabled();
    let report = cfg.integrity();
    assert_eq!(
        (
            report.manifest_present,
            report.checked,
            report.mismatched,
            report.missing
        ),
        (false, 0, 0, 0)
    );
    assert!(cfg.startup_warnings().is_empty());
}

#[test]
fn spot_round_rotates_samples_and_counts_mismatches_once() {
    let dist = temp_dist("spot-core");
    std::fs::write(dist.join("assets/b.js"), "b").unwrap();
    std::fs::write(dist.join("assets/c.js"), "c").unwrap();
    write_manifest(
        &dist,
        &[
            ("/assets/app.js", &sha384_b64(b"console.log(1)")),
            ("/assets/b.js", &sha384_b64(b"b")),
            ("/assets/c.js", &sha384_b64(b"c")),
        ],
    );
    let web = WebConfig::new(Some(dist.clone()), None).with_spot_check(SpotCheckConfig {
        interval: Duration::from_secs(60),
        sample: 1,
    });
    assert_eq!(web.run_spot_round().checked, 1); // /assets/app.js
    std::fs::write(dist.join("assets/b.js"), "tampered").unwrap();
    let round = web.run_spot_round(); // /assets/b.js
    assert_eq!(round.mismatched, 1);
    assert_eq!(round.offenses, vec!["/assets/b.js".to_string()]);
    std::fs::remove_file(dist.join("assets/c.js")).unwrap();
    assert_eq!(web.run_spot_round().missing, 1); // /assets/c.js deleted
    assert_eq!(web.run_spot_round().checked, 1); // wraps to /assets/app.js
    let (report, age) = web.runtime_snapshot();
    assert_eq!(
        (
            report.rounds,
            report.checked,
            report.mismatched,
            report.missing
        ),
        (4, 3, 1, 1)
    );
    assert!(report.last_run.is_some());
    assert!(age.is_some());
    let text = prometheus_text(&web);
    assert!(has_metric_line(&text, "agentj_web_spot_rounds_total 4"));
    assert!(has_metric_line(&text, "agentj_web_spot_checked_total 3"));
    lock_or_recover(&web.warned).insert("/assets/b.js".to_string());
    let repeated = web.run_spot_round(); // /assets/b.js again
    assert_eq!(repeated.mismatched, 1);
    assert!(repeated.offenses.is_empty());
    std::fs::remove_dir_all(&dist).ok();
}

#[tokio::test]
async fn spot_task_runs_rounds_on_the_interval() {
    let dist = temp_dist("spot-task");
    write_temp_dist_manifest(&dist);
    let web = std::sync::Arc::new(
        WebConfig::new(Some(dist.clone()), None).with_spot_check(msec_spot(20, 1)),
    );
    assert!(
        web.spawn_spot_check().is_some(),
        "a usable manifest must spawn the periodic task"
    );
    wait_for_spot_rounds(&web, 1).await;
    assert!(web.runtime_snapshot().0.rounds >= 1);
    web.stop_spot_check();
    std::fs::remove_dir_all(&dist).ok();
}

#[test]
fn spawn_spot_check_is_a_noop_without_a_usable_manifest() {
    let no_dist = std::sync::Arc::new(WebConfig::new(None, None));
    assert!(no_dist.spawn_spot_check().is_none());
    let empty_dist = temp_dist("spot-empty-manifest");
    write_manifest(&empty_dist, &[]);
    let empty = std::sync::Arc::new(WebConfig::new(Some(empty_dist.clone()), None));
    assert!(empty.spawn_spot_check().is_none());
    assert_eq!(empty.runtime_snapshot().0.rounds, 0);
    let broken_dist = temp_dist("spot-broken-manifest");
    std::fs::write(broken_dist.join("asset-manifest.json"), "{not json").unwrap();
    let broken = std::sync::Arc::new(WebConfig::new(Some(broken_dist.clone()), None));
    assert!(broken.spawn_spot_check().is_none());
    for dir in [empty_dist, broken_dist] {
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[tokio::test]
async fn spot_task_warns_once_per_offending_path_across_rounds() {
    let dist = temp_dist("spot-dedup");
    write_manifest(&dist, &[("/assets/app.js", "sha384-AAAA")]);
    let web = std::sync::Arc::new(
        WebConfig::new(Some(dist.clone()), None).with_spot_check(msec_spot(20, 1)),
    );
    let (buf, _guard) = capture_warnings();
    assert!(web.spawn_spot_check().is_some());
    wait_for_spot_rounds(&web, 3).await; // the same key is sampled again
    web.stop_spot_check();
    let out = String::from_utf8(lock_or_recover(&buf.0).clone()).unwrap_or_default();
    assert!(out.contains("web asset spot check failed"), "{out}");
    assert_eq!(
        out.matches("/assets/app.js").count(),
        1,
        "persistent offense must warn exactly once: {out}"
    );
    std::fs::remove_dir_all(&dist).ok();
}
