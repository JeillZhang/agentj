//! Web tests for this domain module.

use super::super::prometheus::prometheus_text;
use super::support::*;
use agentj_core::sync::{lock_or_recover, read_or_recover};

#[test]
fn restore_reads_a_persisted_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::metrics_state::state_path_in(dir.path(), 4096);
    let limiter = crate::login_limiter::LoginLimiter::with_policy(small_policy());
    let now = std::time::Instant::now();
    for _ in 0..3 {
        limiter.record_failure("203.0.113.9".parse().unwrap(), now);
    }
    crate::metrics_state::write(
        &path,
        &limiter.to_state(now, crate::metrics_state::unix_now()),
    )
    .unwrap();

    let web =
        crate::web::WebConfig::new(None, Some("secret".into())).with_metrics_state(Some(path));
    assert!(web.restore_metrics_state());
    assert_eq!(
        web.login_limiter
            .counters(std::time::Instant::now())
            .failures,
        3
    );

    let out = prometheus_text(&web);
    assert!(
        has_metric_line(&out, "agentj_web_metrics_state_restored 1"),
        "{out}"
    );
    assert!(
        has_metric_line(&out, "agentj_web_metrics_state_entries_restored 1"),
        "{out}"
    );
}

#[test]
fn restore_warns_once_and_starts_at_zero_on_a_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::metrics_state::state_path_in(dir.path(), 4096);
    std::fs::write(&path, b"{ garbage").unwrap();
    let web =
        crate::web::WebConfig::new(None, Some("secret".into())).with_metrics_state(Some(path));
    assert!(!web.restore_metrics_state());
    assert!(
        !web.restore_metrics_state(),
        "the second attempt is still not restored"
    );
    assert_eq!(
        lock_or_recover(&web.warned)
            .iter()
            .filter(|k| k.starts_with("metrics-state"))
            .count(),
        1,
        "the warning is emitted once (warned set)"
    );
    assert_eq!(
        web.login_limiter
            .counters(std::time::Instant::now())
            .failures,
        0
    );
}

#[test]
fn flush_writes_only_for_a_configured_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::metrics_state::state_path_in(dir.path(), 4096);
    // `spawn_metrics_flush` takes `self: &std::sync::Arc<Self>`, so this test
    // must hold an Arc; with no configured path it returns `None`.
    let web = std::sync::Arc::new(crate::web::WebConfig::new(None, None));
    web.flush_metrics_state();
    assert!(!path.exists(), "no path configured means no file");
    assert!(web.spawn_metrics_flush().is_none());
}

#[test]
fn flush_reports_write_failures_and_keeps_the_state_dirty() {
    // A path whose parent is a regular file: create_dir_all must fail.
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let web = crate::web::WebConfig::new(None, None)
        .with_metrics_state(Some(crate::metrics_state::state_path_in(&blocker, 4096)));
    web.login_limiter
        .check("203.0.113.9".parse().unwrap(), std::time::Instant::now());
    web.flush_metrics_state();
    let report = *read_or_recover(&web.state_report);
    assert_eq!(report.write_failures, 1);
    assert!(web.login_limiter.dirty(), "a failed write must stay dirty");
    let out = prometheus_text(&web);
    assert!(
        has_metric_line(&out, "agentj_web_metrics_state_write_failures_total 1"),
        "{out}"
    );
    assert!(
        has_metric_line(&out, "agentj_web_metrics_state_last_flush_unix 0"),
        "{out}"
    );
}

#[test]
fn flush_clears_dirty_and_stamps_the_last_flush() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::metrics_state::state_path_in(dir.path(), 4096);
    let web = crate::web::WebConfig::new(None, None).with_metrics_state(Some(path.clone()));
    web.login_limiter
        .check("203.0.113.9".parse().unwrap(), std::time::Instant::now());
    web.flush_metrics_state();
    assert!(!web.login_limiter.dirty());
    assert!(path.is_file());
    let report = *read_or_recover(&web.state_report);
    assert!(report.last_flush_unix > 0);
    assert_eq!(report.write_failures, 0);
}

#[tokio::test]
async fn the_flush_task_writes_only_when_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::metrics_state::state_path_in(dir.path(), 4096);
    let web = std::sync::Arc::new(
        crate::web::WebConfig::new(None, None)
            .with_metrics_state(Some(path.clone()))
            .with_flush_interval(std::time::Duration::from_millis(10)),
    );
    let handle = web.spawn_metrics_flush().expect("flush task");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !path.exists(),
        "a clean limiter must not be written to disk"
    );

    web.login_limiter
        .check("203.0.113.9".parse().unwrap(), std::time::Instant::now());
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        path.is_file(),
        "a dirty limiter must be flushed by the next tick"
    );
    assert!(
        !web.login_limiter.dirty(),
        "one successful flush clears dirty"
    );

    web.stop_metrics_flush();
    handle.abort();
}
