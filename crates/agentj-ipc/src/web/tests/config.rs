//! Web tests for this domain module.

use super::super::*;
use super::support::*;
use std::time::Duration;

#[test]
fn new_web_config_starts_with_zero_integrity_counters() {
    let web = WebConfig::new(None, None);
    let snapshot = web.request_integrity_snapshot();
    assert_eq!(
        (snapshot.checked, snapshot.mismatched, snapshot.missing),
        (0, 0, 0)
    );
    assert_eq!(snapshot.cache_entries, 0);
    let (report, age) = web.on_demand_snapshot();
    assert_eq!(
        (
            report.sweeps,
            report.checked,
            report.mismatched,
            report.missing
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(age, None);
}

#[test]
fn with_integrity_options_clamps_zero_durations_and_oversized_samples() {
    let web = WebConfig::new(None, None).with_integrity_options(IntegrityOptions {
        spot_interval: Duration::ZERO,
        spot_sample: 10_000,
        verify_on_request: false,
        verify_cache_ttl: Duration::ZERO,
    });
    assert_eq!(web.spot_interval(), MIN_SPOT_INTERVAL);
    assert_eq!(web.spot_sample(), MAX_SPOT_SAMPLE);
    assert_eq!(web.verify_cache_ttl(), MIN_VERIFY_TTL);
    assert!(!web.verify_on_request_enabled());
}

#[test]
fn sessions_roundtrip_and_empty_token_normalizes_to_none() {
    let web = WebConfig::new(None, Some(String::new()));
    assert!(web.token.is_none());
    let sid = web.issue_session();
    assert!(web.is_valid_session(&sid));
    web.revoke_session(&sid);
    assert!(!web.is_valid_session(&sid));
}

#[test]
fn startup_warnings_cover_aligned_missing_and_mismatched_dists() {
    let aligned = temp_dist("ver-aligned");
    write_version_json(
        &aligned,
        env!("CARGO_PKG_VERSION"),
        agentj_protocol::PROTOCOL_VERSION,
    );
    write_temp_dist_manifest(&aligned);
    let web = WebConfig::new(Some(aligned.clone()), None);
    assert!(web.dist_version().is_some());
    assert!(
        web.startup_warnings().is_empty(),
        "{:?}",
        web.startup_warnings()
    );

    let mismatched = temp_dist("ver-mismatch");
    write_version_json(&mismatched, "0.0.1-test", "0.0.1");
    write_temp_dist_manifest(&mismatched);
    let warnings = WebConfig::new(Some(mismatched.clone()), None).startup_warnings();
    assert_eq!(warnings.len(), 2);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("0.0.1-test") && w.contains(env!("CARGO_PKG_VERSION")))
    );
    assert!(warnings.iter().any(|w| w.contains("protocol")));

    let legacy = temp_dist("ver-legacy"); // no version.json
    write_temp_dist_manifest(&legacy);
    let warnings = WebConfig::new(Some(legacy.clone()), None).startup_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("version.json"));

    let broken = temp_dist("ver-broken");
    std::fs::write(broken.join("version.json"), "{not json").unwrap();
    let warnings = WebConfig::new(Some(broken.clone()), None).startup_warnings();
    assert!(warnings[0].contains("unreadable"), "{warnings:?}");

    assert!(WebConfig::disabled().startup_warnings().is_empty());
    for d in [aligned, mismatched, legacy, broken] {
        std::fs::remove_dir_all(&d).ok();
    }
}
