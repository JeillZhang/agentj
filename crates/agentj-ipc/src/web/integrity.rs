//! Static-asset integrity: the build-time `asset-manifest.json` reader, the
//! startup sweep, the request-path memo and the runtime spot-check schedule
//! (design D5-D8 / G2 / G6 / G7). Owns every integrity state type `WebConfig`
//! exposes, so the config module only wires them together.

use base64::Engine as _;
use sha2::{Digest as _, Sha384};
use std::time::{Duration, Instant};

/// `dist/asset-manifest.json` written by the UI build (design D5/G5): the
/// sha384 digest of every bundle artefact, keyed by its service path.
#[derive(serde::Deserialize)]
pub(crate) struct AssetManifest {
    algorithm: String,
    pub(super) assets: std::collections::HashMap<String, String>,
}

/// Read and validate the dist manifest. `(None, None)` means the file is absent
/// (legacy build); unreadable or invalid content yields an error string.
pub(super) fn read_asset_manifest(
    dist: Option<&std::path::Path>,
) -> (Option<AssetManifest>, Option<String>) {
    let Some(dir) = dist else { return (None, None) };
    let path = dir.join("asset-manifest.json");
    if !path.is_file() {
        return (None, None);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) => return (None, Some(e.to_string())),
    };
    match serde_json::from_str::<AssetManifest>(&raw) {
        Ok(manifest) => match validate_manifest(&manifest) {
            Ok(()) => (Some(manifest), None),
            Err(reason) => (None, Some(reason)),
        },
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Every key must be a `/`-prefixed relative service path without a traversal
/// component (design D5); any violation makes the whole manifest unusable.
pub(super) fn validate_manifest(manifest: &AssetManifest) -> Result<(), String> {
    if manifest.algorithm != "sha384" {
        return Err(format!("unsupported algorithm {:?}", manifest.algorithm));
    }
    for key in manifest.assets.keys() {
        let Some(rel) = key.strip_prefix('/') else {
            return Err(format!("asset key {key:?} must start with '/'"));
        };
        if rel.is_empty() || rel.contains('\\') || rel.split('/').any(|s| s == "..") {
            return Err(format!(
                "asset key {key:?} is not a canonical relative path"
            ));
        }
    }
    Ok(())
}

/// Outcome of hashing one manifest entry against the bytes on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetCheck {
    Match,
    Mismatch,
    Missing,
}

/// Resolve the manifest key for a mount prefix plus a stripped request path
/// (the `/assets` service sees `path` without its mount prefix; `mount_prefix`
/// is a static so the allocation happens only on a miss).
pub(super) fn manifest_key_for(mount_prefix: &'static str, path: &str) -> String {
    format!("{mount_prefix}{path}")
}

/// Pick the manifest key for a request: the exact `mount_prefix + path` key when
/// the manifest has it, otherwise `fallback_key` (the SPA shell for history
/// routes) when that is present too. `None` means "not a manifest asset", so the
/// request is never hashed against an unrelated digest.
pub(super) fn verify_key(
    manifest: &AssetManifest,
    mount_prefix: &'static str,
    fallback_key: Option<&'static str>,
    path: &str,
) -> Option<String> {
    let key = manifest_key_for(mount_prefix, path);
    if manifest.assets.contains_key(&key) {
        return Some(key);
    }
    fallback_key
        .filter(|fallback| manifest.assets.contains_key(*fallback))
        .map(str::to_string)
}

/// Hash one manifest entry against the bytes on disk; shared by the startup
/// sweep and the runtime spot-check rounds.
pub(super) fn check_asset(dist: &std::path::Path, key: &str, expected: &str) -> AssetCheck {
    let rel = key.strip_prefix('/').unwrap_or(key);
    match std::fs::read(dist.join(rel)) {
        Ok(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(Sha384::digest(&bytes));
            if expected.strip_prefix("sha384-") == Some(encoded.as_str()) {
                AssetCheck::Match
            } else {
                AssetCheck::Mismatch
            }
        }
        Err(_) => AssetCheck::Missing,
    }
}

/// Hash every manifest entry against the bytes on disk (design D6/D7):
/// `checked` counts entries whose file exists, and offending keys are collected
/// so the startup warning can name them.
pub(super) fn verify_assets(dist: &std::path::Path, manifest: &AssetManifest) -> IntegrityReport {
    let mut report = IntegrityReport {
        manifest_present: true,
        ..Default::default()
    };
    for (key, expected) in &manifest.assets {
        match check_asset(dist, key, expected) {
            AssetCheck::Match => report.checked += 1,
            AssetCheck::Mismatch => {
                report.checked += 1;
                report.mismatched += 1;
                report.offenses.push(key.clone());
            }
            AssetCheck::Missing => {
                report.missing += 1;
                report.offenses.push(key.clone());
            }
        }
    }
    report
}

/// Read + verify the manifest once at startup (design D7). A missing dist stays
/// silent; every other failure yields a non-blocking warning (design D8). The
/// parsed manifest is handed back so runtime spot checks can reuse it (D6/D7).
pub(super) fn read_integrity(
    dist: Option<&std::path::Path>,
) -> (IntegrityReport, Option<String>, Option<AssetManifest>) {
    let Some(dir) = dist else {
        return (IntegrityReport::default(), None, None);
    };
    match read_asset_manifest(Some(dir)) {
        (Some(manifest), _) => (verify_assets(dir, &manifest), None, Some(manifest)),
        (None, Some(err)) => (
            IntegrityReport::default(),
            Some(format!(
                "warning: web dist asset manifest is unusable ({err}); skipping integrity verification"
            )),
            None,
        ),
        (None, None) => (
            IntegrityReport::default(),
            Some(
                "warning: web dist has no asset-manifest.json (stale build?); rebuild the UI"
                    .to_string(),
            ),
            None,
        ),
    }
}

/// Warning for a verified manifest with failed entries (design D8): at most
/// three offending paths, sorted for deterministic output.
pub(super) fn integrity_warning(report: &IntegrityReport) -> Option<String> {
    if !report.manifest_present || (report.mismatched == 0 && report.missing == 0) {
        return None;
    }
    let mut names: Vec<&str> = report.offenses.iter().map(String::as_str).collect();
    names.sort_unstable();
    let listed = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    let suffix = if names.len() > 3 { ", …" } else { "" };
    Some(format!(
        "warning: web dist asset integrity check failed: {} mismatched, {} missing after {} \
         checked ({listed}{suffix})",
        report.mismatched, report.missing, report.checked
    ))
}

/// Startup integrity summary for the mounted dist (design G6): counts only,
/// exposed through `WebConfig::integrity()`.
#[derive(Default, Clone)]
pub(crate) struct IntegrityReport {
    pub(crate) manifest_present: bool,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    /// Keys whose digest mismatched or whose file was missing.
    pub(super) offenses: Vec<String>,
}

/// Runtime spot-check schedule (design D6): each round hashes `sample` manifest
/// entries, advancing a cursor over the sorted key set so the full manifest is
/// covered every `ceil(keys / sample)` rounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpotCheckConfig {
    pub(crate) interval: Duration,
    pub(crate) sample: usize,
}

impl Default for SpotCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            sample: 8,
        }
    }
}

/// Request-path integrity knobs derived from the `[web]` config section.
///
/// The bounds are re-clamped inside [`WebConfig::with_integrity_options`]
/// instead of depending on `agentj-config`: these fields are `pub`, and
/// `tokio::time::interval(0)` panics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrityOptions {
    pub spot_interval: Duration,
    pub spot_sample: usize,
    pub verify_on_request: bool,
    pub verify_cache_ttl: Duration,
}

impl Default for IntegrityOptions {
    fn default() -> Self {
        Self {
            spot_interval: Duration::from_secs(60),
            spot_sample: 8,
            verify_on_request: true,
            verify_cache_ttl: Duration::from_secs(60),
        }
    }
}

/// Clamp bounds for [`IntegrityOptions`], mirroring `agentj-config`'s `[web]` bounds.
pub(crate) const MIN_SPOT_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const MAX_SPOT_INTERVAL: Duration = Duration::from_secs(3600);
pub(crate) const MIN_SPOT_SAMPLE: usize = 1;
pub(crate) const MAX_SPOT_SAMPLE: usize = 1024;
pub(crate) const MIN_VERIFY_TTL: Duration = Duration::from_secs(1);
pub(crate) const MAX_VERIFY_TTL: Duration = Duration::from_secs(3600);
/// Upper bound on the request-path verification memo (design: bounded cache).
pub(crate) const VERIFY_CACHE_MAX: usize = 1024;

/// How many offending paths a single manual sweep reports (counts stay exact;
/// the list is truncated to this many, sorted, with `truncated` set).
pub(crate) const INTEGRITY_OFFENSES_LIMIT: usize = 50;

/// Cumulative outcome of the manual `POST /integrity/verify` sweeps (design G7):
/// written by the sweep handler, read by `/version`.
#[derive(Debug, Clone, Default)]
pub(crate) struct IntegrityOnDemandReport {
    pub(crate) sweeps: u64,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) last_run: Option<Instant>,
}

/// The one-off result of a manual sweep (design G3), serialised by the endpoint.
#[derive(Debug, Clone, Default)]
pub(crate) struct FullSweepOutcome {
    pub(crate) manifest_present: bool,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) offenses: Vec<String>,
    pub(crate) truncated: bool,
    pub(crate) duration_ms: u64,
}

/// Request-path verification counters plus the current memo size (design G2).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestIntegritySnapshot {
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) cache_entries: usize,
}

/// One memoised request-path verdict plus the instant it was taken, so the
/// expiry check is absolute (a cache hit never extends the TTL).
pub(crate) struct VerifyEntry {
    #[allow(dead_code)] // inspected by the test-only `verify_cache_verdict`
    pub(super) verdict: AssetCheck,
    pub(super) checked_at: Instant,
}

/// Cumulative runtime spot-check state (design D7): written by the periodic
/// task, read by `/version` and `/metrics`.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpotCheckReport {
    pub(crate) rounds: u64,
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) last_run: Option<Instant>,
}

/// Outcome of one spot-check round. `offenses` names mismatched and missing
/// paths that were not reported by an earlier round, so the periodic task can
/// warn once per path (design D7).
#[derive(Debug, Clone, Default)]
pub(crate) struct SpotRound {
    pub(crate) checked: u64,
    pub(crate) mismatched: u64,
    pub(crate) missing: u64,
    pub(crate) offenses: Vec<String>,
}

/// Client-IP source tallies for `agentj_web_client_ip_source_total` plus the
/// derivation failures (trust on, header present, nothing usable).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct IpSourceTotals {
    pub(crate) peer: u64,
    pub(crate) forwarded: u64,
    pub(crate) real_ip: u64,
    pub(crate) invalid: u64,
}

/// Outcome of the persisted-state lifecycle, exposed as gauges/counters.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StateReport {
    pub(crate) restored: bool,
    pub(crate) restored_entries: u64,
    pub(crate) write_failures: u64,
    pub(crate) last_flush_unix: u64,
}
