//! Kernel-hosted web UI helpers (design D2/D3/D5): hybrid auth config,
//! opaque session cookies, origin/next validation and the login page.

use axum::response::IntoResponse as _;
use base64::Engine as _;
use sha2::{Digest as _, Sha384};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

use crate::client_ip::IpSource;
use crate::login_limiter::{Counters, IpSnapshot, LoginLimiter, Throttle};

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

/// `dist/asset-manifest.json` written by the UI build (design D5/G5): the
/// sha384 digest of every bundle artefact, keyed by its service path.
#[derive(serde::Deserialize)]
struct AssetManifest {
    algorithm: String,
    assets: std::collections::HashMap<String, String>,
}

/// Read and validate the dist manifest. `(None, None)` means the file is absent
/// (legacy build); unreadable or invalid content yields an error string.
fn read_asset_manifest(dist: Option<&std::path::Path>) -> (Option<AssetManifest>, Option<String>) {
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
fn validate_manifest(manifest: &AssetManifest) -> Result<(), String> {
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
enum AssetCheck {
    Match,
    Mismatch,
    Missing,
}

/// Resolve the manifest key for a mount prefix plus a stripped request path
/// (the `/assets` service sees `path` without its mount prefix; `mount_prefix`
/// is a static so the allocation happens only on a miss).
fn manifest_key_for(mount_prefix: &'static str, path: &str) -> String {
    format!("{mount_prefix}{path}")
}

/// Pick the manifest key for a request: the exact `mount_prefix + path` key when
/// the manifest has it, otherwise `fallback_key` (the SPA shell for history
/// routes) when that is present too. `None` means "not a manifest asset", so the
/// request is never hashed against an unrelated digest.
fn verify_key(
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
fn check_asset(dist: &std::path::Path, key: &str, expected: &str) -> AssetCheck {
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
fn verify_assets(dist: &std::path::Path, manifest: &AssetManifest) -> IntegrityReport {
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
fn read_integrity(
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
fn integrity_warning(report: &IntegrityReport) -> Option<String> {
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
    offenses: Vec<String>,
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
struct VerifyEntry {
    #[allow(dead_code)] // inspected by the test-only `verify_cache_verdict`
    verdict: AssetCheck,
    checked_at: Instant,
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

/// Static-web-UI runtime config: resolved dist directory plus the optional
/// access token and the in-memory set of issued sessions (D3, no persistence).
pub struct WebConfig {
    pub dist: Option<std::path::PathBuf>,
    pub token: Option<String>,
    dist_version: Option<DistVersion>,
    dist_version_error: Option<String>,
    integrity: IntegrityReport,
    integrity_error: Option<String>,
    manifest: Option<AssetManifest>,
    spot: SpotCheckConfig,
    verify_on_request: bool,
    verify_cache_ttl: Duration,
    verify_cache: std::sync::Mutex<std::collections::HashMap<String, VerifyEntry>>,
    on_demand: std::sync::RwLock<IntegrityOnDemandReport>,
    request_checked: std::sync::atomic::AtomicU64,
    request_mismatched: std::sync::atomic::AtomicU64,
    request_missing: std::sync::atomic::AtomicU64,
    spot_report: std::sync::RwLock<SpotCheckReport>,
    warned: std::sync::Mutex<std::collections::HashSet<String>>,
    spot_cursor: std::sync::atomic::AtomicUsize,
    spot_task: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    sessions: std::sync::RwLock<std::collections::HashSet<String>>,
    login_limiter: LoginLimiter,
    trust_proxy_headers: bool,
    metrics_state: Option<std::path::PathBuf>,
    ip_sources: std::sync::RwLock<IpSourceTotals>,
    state_report: std::sync::RwLock<StateReport>,
    flush_task: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    flush_interval: Duration,
}

impl WebConfig {
    pub fn disabled() -> Self {
        Self::new(None, None)
    }
    /// Empty-string tokens normalize to `None` (no auth configured).
    pub fn new(dist: Option<std::path::PathBuf>, token: Option<String>) -> Self {
        let (dist_version, dist_version_error) = read_dist_version(dist.as_deref());
        let (integrity, integrity_error, manifest) = read_integrity(dist.as_deref());
        Self {
            dist,
            token: token.filter(|t| !t.is_empty()),
            dist_version,
            dist_version_error,
            integrity,
            integrity_error,
            manifest,
            spot: SpotCheckConfig::default(),
            verify_on_request: true,
            verify_cache_ttl: Duration::from_secs(60),
            verify_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            on_demand: std::sync::RwLock::new(IntegrityOnDemandReport::default()),
            request_checked: std::sync::atomic::AtomicU64::new(0),
            request_mismatched: std::sync::atomic::AtomicU64::new(0),
            request_missing: std::sync::atomic::AtomicU64::new(0),
            spot_report: std::sync::RwLock::new(SpotCheckReport::default()),
            warned: std::sync::Mutex::new(std::collections::HashSet::new()),
            spot_cursor: std::sync::atomic::AtomicUsize::new(0),
            spot_task: std::sync::Mutex::new(None),
            sessions: std::sync::RwLock::new(std::collections::HashSet::new()),
            login_limiter: LoginLimiter::new(),
            trust_proxy_headers: false,
            metrics_state: None,
            ip_sources: std::sync::RwLock::new(IpSourceTotals::default()),
            state_report: std::sync::RwLock::new(StateReport::default()),
            flush_task: std::sync::Mutex::new(None),
            flush_interval: Duration::from_secs(5),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_login_limiter(mut self, limiter: LoginLimiter) -> Self {
        self.login_limiter = limiter;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_spot_check(mut self, spot: SpotCheckConfig) -> Self {
        self.spot = spot;
        self
    }
    #[cfg(test)]
    pub(crate) fn spot_interval(&self) -> Duration {
        self.spot.interval
    }
    #[cfg(test)]
    pub(crate) fn spot_sample(&self) -> usize {
        self.spot.sample
    }
    #[cfg(test)]
    pub(crate) fn verify_cache_ttl(&self) -> Duration {
        self.verify_cache_ttl
    }
    #[cfg(test)]
    pub(crate) fn verify_on_request_enabled(&self) -> bool {
        self.verify_on_request
    }
    /// Test-only escape hatch past the `MIN_VERIFY_TTL` clamp, so TTL tests can use
    /// sub-second windows instead of sleeping a full second.
    #[cfg(test)]
    pub(crate) fn with_raw_verify_cache_ttl(mut self, ttl: Duration) -> Self {
        self.verify_cache_ttl = ttl;
        self
    }
    /// Apply the `[web]` integrity knobs. Re-clamps defensively: the fields of
    /// `IntegrityOptions` are `pub`, and `tokio::time::interval(0)` panics.
    pub(crate) fn with_integrity_options(mut self, opts: IntegrityOptions) -> Self {
        self.spot = SpotCheckConfig {
            interval: opts
                .spot_interval
                .clamp(MIN_SPOT_INTERVAL, MAX_SPOT_INTERVAL),
            sample: opts.spot_sample.clamp(MIN_SPOT_SAMPLE, MAX_SPOT_SAMPLE),
        };
        self.verify_on_request = opts.verify_on_request;
        self.verify_cache_ttl = opts.verify_cache_ttl.clamp(MIN_VERIFY_TTL, MAX_VERIFY_TTL);
        self
    }
    /// Verify one manifest entry, memoised for `verify_cache_ttl` (design D9/D10).
    ///
    /// The memo is keyed by manifest key and expires on an absolute deadline: a
    /// hit never refreshes `checked_at`, so a tampered file is re-hashed once the
    /// window closes instead of being trusted forever. Keys absent from the
    /// manifest are ignored (they are never hashed).
    pub(crate) fn verify_cached_asset(&self, key: &str) {
        let now = Instant::now();
        {
            let cache = self.verify_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = cache.get(key) {
                // Absolute TTL: a hit never refreshes `checked_at`.
                if now.duration_since(entry.checked_at) < self.verify_cache_ttl {
                    return;
                }
            }
        }; // release before any IO — never hold this lock across a read + hash
        // Only a miss consults the manifest: a hit must not even clone the digest.
        let Some(dist) = self.dist.as_deref() else {
            return;
        };
        let Some(expected) = self.manifest.as_ref().and_then(|m| m.assets.get(key)) else {
            return;
        };
        let verdict = check_asset(dist, key, expected);
        match verdict {
            AssetCheck::Match => {
                self.request_checked
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            AssetCheck::Mismatch => {
                self.request_checked
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.request_mismatched
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            AssetCheck::Missing => {
                self.request_missing
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if verdict != AssetCheck::Match {
            self.warn_offense_once(key);
        }
        self.store_verify_entry(key.to_string(), verdict, now);
    }
    /// Insert one verdict, keeping the memo bounded: expired entries are dropped
    /// first, and a still-full cache is cleared rather than grown without limit.
    fn store_verify_entry(&self, key: String, verdict: AssetCheck, at: Instant) {
        let mut cache = self.verify_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= VERIFY_CACHE_MAX && !cache.contains_key(&key) {
            let ttl = self.verify_cache_ttl;
            cache.retain(|_, entry| at.duration_since(entry.checked_at) < ttl);
            if cache.len() >= VERIFY_CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert(
            key,
            VerifyEntry {
                verdict,
                checked_at: at,
            },
        );
    }
    /// One `warn!` per path for the whole process; shared by every integrity path.
    pub(crate) fn warn_offense_once(&self, path: &str) {
        let mut warned = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        if warned.insert(path.to_string()) {
            tracing::warn!(path = %path, "web asset integrity check failed");
        }
    }
    /// The memoised verdict for `key`, for tests that need to see the cache.
    #[cfg(test)]
    fn verify_cache_verdict(&self, key: &str) -> Option<AssetCheck> {
        self.verify_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .map(|entry| entry.verdict)
    }
    /// Opt in to forwarded-header trust (design D1). Takes effect only when the
    /// direct peer is loopback.
    pub(crate) fn with_trust_proxy_headers(mut self, trust: bool) -> Self {
        self.trust_proxy_headers = trust;
        self
    }
    /// Opt in to state persistence (design D3). `None` keeps the limiter purely
    /// in memory — which is what every directly constructed `WebConfig` does.
    pub(crate) fn with_metrics_state(mut self, path: Option<std::path::PathBuf>) -> Self {
        self.metrics_state = path;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }
    pub(crate) fn trust_proxy_headers(&self) -> bool {
        self.trust_proxy_headers
    }
    #[allow(dead_code)] // only used by the tcp bind tests
    pub(crate) fn metrics_state_path(&self) -> Option<&std::path::Path> {
        self.metrics_state.as_deref()
    }
    /// Tally one resolved client IP (design D6). `invalid` is set by the caller
    /// only when trust applied, a header was present and nothing usable came out.
    pub(crate) fn record_ip_source(&self, source: crate::client_ip::IpSource, invalid: bool) {
        let mut totals = self.ip_sources.write().unwrap_or_else(|e| e.into_inner());
        match source {
            crate::client_ip::IpSource::Peer => totals.peer += 1,
            crate::client_ip::IpSource::XForwardedFor => totals.forwarded += 1,
            crate::client_ip::IpSource::XRealIp => totals.real_ip += 1,
        }
        if invalid {
            totals.invalid += 1;
        }
    }
    fn warn_once(&self, key: &str, message: &str) {
        let first = self
            .warned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string());
        if first {
            tracing::warn!("{message}");
        }
    }
    /// Load the persisted limiter state (design D3): a missing file is a normal
    /// first run; an unusable one warns once and starts from zero, never blocking
    /// startup. Returns whether state was restored.
    pub(crate) fn restore_metrics_state(&self) -> bool {
        let Some(path) = self.metrics_state.as_deref() else {
            return false;
        };
        match crate::metrics_state::read(path) {
            Ok(Some(state)) => {
                let entries = self.login_limiter.restore_from(
                    &state,
                    Instant::now(),
                    crate::metrics_state::unix_now(),
                );
                let mut report = self.state_report.write().unwrap_or_else(|e| e.into_inner());
                report.restored = true;
                report.restored_entries = entries as u64;
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.warn_once(
                    "metrics-state-read",
                    &format!(
                        "warning: login metrics state at {} is unusable ({error}); counters start at zero",
                        path.display()
                    ),
                );
                false
            }
        }
    }
    /// Write the limiter state if a path is configured (design D3). Failures are
    /// counted and warned once; the dirty flag stays set so the next tick retries.
    ///
    /// Graceful-shutdown API: the CLI calls this (before `stop_metrics_flush`)
    /// so a recorded login-failure window survives the process exiting.
    pub fn flush_metrics_state(&self) {
        let Some(path) = self.metrics_state.as_deref() else {
            return;
        };
        let now = Instant::now();
        let now_unix = crate::metrics_state::unix_now();
        let state = self.login_limiter.to_state(now, now_unix);
        match crate::metrics_state::write(path, &state) {
            Ok(()) => {
                self.login_limiter.mark_clean();
                let mut report = self.state_report.write().unwrap_or_else(|e| e.into_inner());
                report.last_flush_unix = now_unix;
            }
            Err(error) => {
                {
                    let mut report = self.state_report.write().unwrap_or_else(|e| e.into_inner());
                    report.write_failures += 1;
                }
                self.warn_once(
                    "metrics-state-write",
                    &format!(
                        "warning: cannot persist login metrics state to {} ({error})",
                        path.display()
                    ),
                );
            }
        }
    }
    /// Periodic debounced flush (design D3): one tick writes only when the
    /// limiter is dirty, mirroring the spot-check task's lifecycle.
    pub(crate) fn spawn_metrics_flush(self: &Arc<Self>) -> Option<JoinHandle<()>> {
        self.metrics_state.as_ref()?;
        let web = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(web.flush_interval);
            loop {
                ticks.tick().await;
                if web.login_limiter.dirty() {
                    web.flush_metrics_state();
                }
            }
        });
        *self.flush_task.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle.abort_handle());
        Some(handle)
    }
    /// Graceful-shutdown API: abort the debounced flush task. Callers flush
    /// first (`flush_metrics_state`), so the last snapshot is never lost.
    pub fn stop_metrics_flush(&self) {
        let task = self
            .flush_task
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(task) = task {
            task.abort();
        }
    }
    /// Run one spot-check round (design D6): hash up to `spot.sample` manifest
    /// entries starting at the rotating cursor over the sorted key set, fold the
    /// outcome into the shared report and return it. Offending paths recorded in
    /// `warned` by an earlier round are omitted, and the task records new ones
    /// (design D7) so persistent tampering warns once per path.
    pub(crate) fn run_spot_round(&self) -> SpotRound {
        let (Some(dist), Some(manifest)) = (&self.dist, &self.manifest) else {
            return SpotRound::default();
        };
        let mut keys: Vec<&str> = manifest.assets.keys().map(String::as_str).collect();
        if keys.is_empty() {
            return SpotRound::default();
        }
        keys.sort_unstable();
        let sample = self.spot.sample.max(1).min(keys.len());
        let cursor = self
            .spot_cursor
            .fetch_add(sample, std::sync::atomic::Ordering::Relaxed);
        let start = cursor % keys.len();
        let mut round = SpotRound::default();
        for index in 0..sample {
            let key = keys[(start + index) % keys.len()];
            match check_asset(dist, key, &manifest.assets[key]) {
                AssetCheck::Match => round.checked += 1,
                AssetCheck::Mismatch => {
                    round.checked += 1;
                    round.mismatched += 1;
                    round.offenses.push(key.to_string());
                }
                AssetCheck::Missing => {
                    round.missing += 1;
                    round.offenses.push(key.to_string());
                }
            }
        }
        {
            let mut report = self.spot_report.write().unwrap_or_else(|e| e.into_inner());
            report.rounds += 1;
            report.checked += round.checked;
            report.mismatched += round.mismatched;
            report.missing += round.missing;
            report.last_run = Some(Instant::now());
        }
        let warned = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        round.offenses.retain(|path| !warned.contains(path));
        round
    }
    /// The runtime spot-check report plus the age of its last round in whole
    /// seconds; the age is `None` until the first round has run (design G7).
    pub(crate) fn runtime_snapshot(&self) -> (SpotCheckReport, Option<u64>) {
        let report = self
            .spot_report
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let age = report
            .last_run
            .map(|t| Instant::now().duration_since(t).as_secs());
        (report, age)
    }
    /// Request-path verification counters plus the current memo size (design G2).
    pub(crate) fn request_integrity_snapshot(&self) -> RequestIntegritySnapshot {
        let cache_entries = self
            .verify_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        RequestIntegritySnapshot {
            checked: self
                .request_checked
                .load(std::sync::atomic::Ordering::Relaxed),
            mismatched: self
                .request_mismatched
                .load(std::sync::atomic::Ordering::Relaxed),
            missing: self
                .request_missing
                .load(std::sync::atomic::Ordering::Relaxed),
            cache_entries,
        }
    }
    /// Report the manual-sweep totals plus the age of the last sweep in whole
    /// seconds; `None` until the first sweep has run (design G7).
    pub(crate) fn on_demand_snapshot(&self) -> (IntegrityOnDemandReport, Option<u64>) {
        let report = self
            .on_demand
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let age = report
            .last_run
            .map(|t| Instant::now().duration_since(t).as_secs());
        (report, age)
    }
    /// Sweep every manifest entry once (design G3/D5): the totals are folded into
    /// the manual-sweep report first (the write lock only wraps that arithmetic),
    /// then each offending path warns once per process and the returned list is
    /// sorted and truncated to [`INTEGRITY_OFFENSES_LIMIT`]. An absent dist or
    /// manifest does no work and is not counted.
    pub(crate) fn run_full_integrity_sweep(&self) -> FullSweepOutcome {
        let (Some(dist), Some(manifest)) = (&self.dist, &self.manifest) else {
            return FullSweepOutcome::default();
        };
        let started = Instant::now();
        let report = verify_assets(dist, manifest);
        {
            let mut on_demand = self.on_demand.write().unwrap_or_else(|e| e.into_inner());
            on_demand.sweeps += 1;
            on_demand.checked += report.checked;
            on_demand.mismatched += report.mismatched;
            on_demand.missing += report.missing;
            on_demand.last_run = Some(started);
        }
        let mut offenses = report.offenses.clone();
        offenses.sort();
        for path in &offenses {
            self.warn_offense_once(path);
        }
        let truncated = offenses.len() > INTEGRITY_OFFENSES_LIMIT;
        offenses.truncate(INTEGRITY_OFFENSES_LIMIT);
        FullSweepOutcome {
            manifest_present: true,
            checked: report.checked,
            mismatched: report.mismatched,
            missing: report.missing,
            offenses,
            truncated,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }
    /// Spawn the periodic spot-check task (design D6/D7): every tick runs one
    /// round and warns once per newly offending path. `None` when the manifest
    /// is absent/unusable or has no keys (nothing to sample); the abort handle
    /// is retained for [`Self::stop_spot_check`].
    pub(crate) fn spawn_spot_check(self: &Arc<Self>) -> Option<JoinHandle<()>> {
        let manifest = self.manifest.as_ref()?;
        if manifest.assets.is_empty() {
            return None;
        }
        let web = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(web.spot.interval);
            loop {
                ticks.tick().await;
                let round = web.run_spot_round();
                for path in round.offenses {
                    let first = web
                        .warned
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(path.clone());
                    if first {
                        tracing::warn!(path = %path, "web asset spot check failed");
                    }
                }
            }
        });
        let abort = handle.abort_handle();
        *self.spot_task.lock().unwrap_or_else(|e| e.into_inner()) = Some(abort);
        Some(handle)
    }
    /// Abort the periodic spot-check task, if one is running (server shutdown,
    /// tests).
    pub(crate) fn stop_spot_check(&self) {
        let task = self
            .spot_task
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(task) = task {
            task.abort();
        }
    }
    /// The static web UI is served only when a dist directory was resolved.
    pub fn enabled(&self) -> bool {
        self.dist.is_some()
    }
    pub fn dist_version(&self) -> Option<&DistVersion> {
        self.dist_version.as_ref()
    }
    pub(crate) fn integrity(&self) -> &IntegrityReport {
        &self.integrity
    }
    /// Startup diagnostics: empty when aligned or no dist is configured.
    /// Version mismatches and failed integrity checks warn without blocking.
    pub fn startup_warnings(&self) -> Vec<String> {
        if self.dist.is_none() {
            return Vec::new();
        }
        let mut out = self.version_warnings();
        if let Some(error) = &self.integrity_error {
            out.push(error.clone());
        } else if let Some(warning) = integrity_warning(self.integrity()) {
            out.push(warning);
        }
        out
    }
    fn version_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        match (&self.dist_version, &self.dist_version_error) {
            (Some(v), _) => {
                let product = env!("CARGO_PKG_VERSION");
                if v.product_version != product {
                    out.push(format!(
                        "warning: web dist was built for agentj {} but this server is {}; rebuild the UI",
                        v.product_version, product
                    ));
                }
                let protocol = agentj_protocol::PROTOCOL_VERSION;
                if v.protocol_version != protocol {
                    out.push(format!(
                        "warning: web dist targets protocol {} but this server speaks {}; rebuild the UI",
                        v.protocol_version, protocol
                    ));
                }
            }
            (None, Some(err)) => out.push(format!(
                "warning: web dist version.json is unreadable ({err}); rebuild the UI"
            )),
            (None, None) => out.push(
                "warning: web dist has no version.json (stale build?); rebuild the UI".to_string(),
            ),
        }
        out
    }
    pub fn issue_session(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.write().expect("sessions").insert(id.clone());
        id
    }
    pub fn is_valid_session(&self, id: &str) -> bool {
        self.sessions.read().expect("sessions").contains(id)
    }
    pub fn revoke_session(&self, id: &str) {
        self.sessions.write().expect("sessions").remove(id);
    }
}

/// Whether the request peer is a loopback address (hybrid auth exemption, D2).
pub fn peer_is_loopback(peer: std::net::SocketAddr) -> bool {
    crate::client_ip::is_loopback_ip(peer.ip())
}

/// Derive the client IP used for identification (design D2/D5). Tally the source
/// as a side effect. `invalid` is true only when trust applied, the request
/// carried a proxy header and nothing usable came out of it.
fn client_ip_for(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> crate::client_ip::ClientIp {
    let trust = web.trust_proxy_headers() && peer_is_loopback(peer);
    let resolved = crate::client_ip::resolve_client_ip(peer, headers, trust);
    let invalid = trust
        && resolved.source == crate::client_ip::IpSource::Peer
        && crate::client_ip::has_proxy_header(headers);
    web.record_ip_source(resolved.source, invalid);
    resolved
}

/// Extract the `agentj_session` cookie value from a `Cookie` header.
pub fn session_cookie(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("agentj_session="))
}

/// Same-origin check for the cookie channel (D4): strip the scheme from
/// `Origin` and compare the remaining authority to the `Host` header.
pub fn origin_matches_host(headers: &axum::http::HeaderMap) -> bool {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    match (host, origin) {
        (Some(h), Some(o)) => {
            o.strip_prefix("http://")
                .or_else(|| o.strip_prefix("https://"))
                == Some(h)
        }
        _ => false,
    }
}

/// Whitelist `next` redirect targets to same-site absolute paths (防开放重定向).
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.contains('\\')
                && !n.chars().any(char::is_control) =>
        {
            n.to_string()
        }
        _ => "/".to_string(),
    }
}

/// Percent-encode everything but RFC 3986 unreserved characters (and `/`).
pub fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Minimal login page rendered on the remote-access path (D2).
pub fn login_page_html(error: Option<&str>, next: &str) -> String {
    let error = error
        .map(|e| format!("<p class=\"err\">{}</p>", escape_html(e)))
        .unwrap_or_default();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentJ</title></head>\
         <body><main><h1>AgentJ</h1><p>输入访问令牌以连接到内核。</p>{error}\
         <form method=\"post\" action=\"/login\"><input type=\"hidden\" name=\"next\" value=\"{}\">\
         <input name=\"token\" type=\"password\" required autofocus><button>登录</button></form>\
         </main></body></html>",
        escape_html(next)
    )
}

#[derive(serde::Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(serde::Deserialize)]
struct LoginForm {
    token: String,
    next: Option<String>,
}

fn redirect_found(location: &str) -> axum::response::Response {
    (
        axum::http::StatusCode::FOUND,
        [(axum::http::header::LOCATION, location.to_string())],
    )
        .into_response()
}

/// Validate the submitted token (only when one is configured) and, on success,
/// issue an opaque session id in a session cookie (no Max-Age) before
/// redirecting to the whitelisted `next` path. A locked-out peer is answered
/// from the limiter before the token is even read (the failure escalates).
async fn submit_login(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
    form: LoginForm,
) -> axum::response::Response {
    let now = Instant::now();
    // Identification only: the throttle key follows the derived client IP, while
    // the auth exemption below keeps the raw peer (design D5).
    let ip = client_ip_for(web, peer, headers).ip;
    if matches!(web.login_limiter.check(ip, now), Throttle::Locked { .. }) {
        let throttle = web.login_limiter.record_failure(ip, now);
        return throttle_response(throttle, form.next.as_deref());
    }
    let expected = web.token.as_deref().unwrap_or_default();
    if !expected.is_empty() && form.token == expected {
        web.login_limiter.record_success(ip);
        let sid = web.issue_session();
        let mut res = redirect_found(&safe_next(form.next.as_deref()));
        res.headers_mut().insert(
            axum::http::header::SET_COOKIE,
            format!("{SESSION_COOKIE}={sid}; HttpOnly; SameSite=Lax; Path=/")
                .parse()
                .expect("valid cookie header"),
        );
        res
    } else {
        let throttle = web.login_limiter.record_failure(ip, now);
        throttle_response(throttle, form.next.as_deref())
    }
}

fn unauthorized_response(next: Option<&str>) -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::response::Html(login_page_html(Some("访问令牌无效"), &safe_next(next))),
    )
        .into_response()
}

/// `Locked` answers 429 with the remaining wait (whole seconds, min 1, ceil)
/// and no cookie; an `Allow` throttle is just the plain 401 page.
fn throttle_response(throttle: Throttle, next: Option<&str>) -> axum::response::Response {
    let Throttle::Locked { retry_after } = throttle else {
        return unauthorized_response(next);
    };
    let secs = retry_after.as_millis().div_ceil(1000).max(1) as u64;
    let message = format!("尝试次数过多，请在 {secs} 秒后重试");
    let mut res = (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        axum::response::Html(login_page_html(Some(&message), &safe_next(next))),
    )
        .into_response();
    res.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        axum::http::HeaderValue::from_str(&secs.to_string()).expect("retry-after"),
    );
    res
}

/// Revoke the caller's session if present and clear the cookie (`Max-Age=0`).
async fn logout(web: &WebConfig, headers: axum::http::HeaderMap) -> axum::response::Response {
    if let Some(sid) = session_cookie(&headers) {
        web.revoke_session(sid);
    }
    let mut res = redirect_found("/login");
    res.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
            .parse()
            .expect("valid cookie header"),
    );
    res
}

/// Register the credential endpoints. They are deliberately **not** gated (T5
/// wraps only the static surface), so a signed-out browser cannot loop.
fn auth_routes(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
) -> axum::Router {
    let login_get = |axum::extract::Query(q): axum::extract::Query<LoginQuery>| async move {
        axum::response::Html(login_page_html(None, &safe_next(q.next.as_deref())))
    };
    let login_post = {
        let web = web.clone();
        move |headers: axum::http::HeaderMap, form: axum::extract::Form<LoginForm>| {
            let web = web.clone();
            async move { submit_login(&web, peer, &headers, form.0).await }
        }
    };
    let logout_post = move |headers: axum::http::HeaderMap| {
        let web = web.clone();
        async move { logout(&web, headers).await }
    };
    router
        .route("/login", axum::routing::get(login_get).post(login_post))
        .route("/logout", axum::routing::post(logout_post))
}

/// Whether the hybrid login gate applies to this request (design D2): a dist is
/// served, a token is configured, the peer is remote and no valid session
/// cookie was presented. Loopback peers and token-less servers stay open.
pub fn gate_required(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> bool {
    web.enabled() && auth_required(web, peer, headers)
}

/// Auth check without the dist requirement (design G3): the machine surface
/// (`/metrics`) must answer even when no static UI is mounted. A token must be
/// configured, the peer must be remote and no valid session cookie presented.
pub fn auth_required(
    web: &WebConfig,
    peer: std::net::SocketAddr,
    headers: &axum::http::HeaderMap,
) -> bool {
    // D5: the loopback exemption is decided by the RAW TCP peer only. The
    // derived client IP (`client_ip::resolve_client_ip`) must never reach here —
    // otherwise a request header could grant kernel access.
    web.token.is_some()
        && !peer_is_loopback(peer)
        && !session_cookie(headers).is_some_and(|sid| web.is_valid_session(sid))
}

/// Login-gate middleware for one static mount. `nest_service` strips the mount
/// prefix before the inner service (and this layer) runs, so `mount` re-attaches
/// it — the redirect must carry the full same-origin path.
async fn gate(
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    mount: &'static str,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if gate_required(&web, peer, req.headers()) {
        let full_path = format!("{mount}{}", req.uri().path());
        return redirect_found(&format!("/login?next={}", pct_encode(&full_path)));
    }
    next.run(req).await
}

/// Per-request integrity gate for one static mount (D9: fail-open — the
/// verdict never changes the response, the served bytes are checked and
/// counted). `mount_prefix` re-attaches what `nest_service` stripped so the
/// manifest key can be resolved, and `fallback_key` is the SPA fallback's
/// document for non-asset paths.
async fn verify_asset(
    web: std::sync::Arc<WebConfig>,
    mount_prefix: &'static str,
    fallback_key: Option<&'static str>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if web.verify_on_request {
        let key = web.manifest.as_ref().and_then(|manifest| {
            verify_key(manifest, mount_prefix, fallback_key, req.uri().path())
        });
        if let Some(key) = key {
            web.verify_cached_asset(&key);
        }
    }
    next.run(req).await
}

/// Mount the static SPA surface (D6: cache headers + login gate wrapped around
/// each service individually — `Router::layer` would leak onto `/ws`). The root
/// mount is `fallback_service` (unknown paths -> index.html, 200) because
/// `nest_service("/")` panics.
fn static_surface(
    router: axum::Router,
    web: std::sync::Arc<WebConfig>,
    peer: std::net::SocketAddr,
    dist: &std::path::Path,
) -> axum::Router {
    use tower::Layer as _;
    let spa = tower_http::services::ServeDir::new(dist).fallback(
        tower_http::services::ServeFile::new(dist.join("index.html")),
    );
    let spa = tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    )
    .layer(spa);
    let assets = tower_http::set_header::SetResponseHeaderLayer::overriding(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    )
    .layer(tower_http::services::ServeDir::new(dist.join("assets")));
    let gate_assets = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { gate(web, peer, "/assets", req, next).await }
        })
    };
    let gate_spa = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { gate(web, peer, "", req, next).await }
        })
    };
    let verify_assets = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { verify_asset(web, "/assets", None, req, next).await }
        })
    };
    let verify_spa = {
        let web = web.clone();
        axum::middleware::from_fn(move |req, next| {
            let web = web.clone();
            async move { verify_asset(web, "", Some("/index.html"), req, next).await }
        })
    };
    // Gate outermost, verify innermost: an unauthorised request must not hash.
    router
        .nest_service("/assets", gate_assets.layer(verify_assets.layer(assets)))
        .fallback_service(gate_spa.layer(verify_spa.layer(spa)))
}

/// Unauthenticated version probe: reports this server's product/protocol
/// versions, the optional facts read from the mounted dist, the aggregate
/// login-limiter counters, the startup integrity summary and the runtime
/// spot-check block, so a remote client or operator can detect a stale UI and
/// observe throttling without holding a session cookie. Registered as an
/// explicit route (like `/login`), so the login gate and the SPA fallback never
/// answer it.
async fn version_handler(web: std::sync::Arc<WebConfig>) -> axum::response::Response {
    let dist = web.dist_version();
    let counters = web.login_limiter.counters(Instant::now());
    let integrity = web.integrity();
    let (runtime, last_run_age) = web.runtime_snapshot();
    let (on_demand, on_demand_age) = web.on_demand_snapshot();
    let mut runtime_json = serde_json::json!({
        "rounds": runtime.rounds,
        "checked": runtime.checked,
        "mismatched": runtime.mismatched,
        "missing": runtime.missing,
    });
    if let Some(age) = last_run_age {
        runtime_json["last_run_age_secs"] = serde_json::json!(age);
    }
    let mut on_demand_json = serde_json::json!({
        "sweeps": on_demand.sweeps,
        "checked": on_demand.checked,
        "mismatched": on_demand.mismatched,
        "missing": on_demand.missing,
    });
    if let Some(age) = on_demand_age {
        on_demand_json["last_run_age_secs"] = serde_json::json!(age);
    }
    let body = serde_json::json!({
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
            "runtime": runtime_json,
            "on_demand": on_demand_json,
        },
    });
    let mut res = axum::Json(body).into_response();
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    res
}

/// Auth-gated Prometheus scrape endpoint (design G2/D1): the machine surface
/// answers `401` without a redirect, so scrapers never chase the login page.
async fn metrics_handler(
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
async fn integrity_verify_handler(
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

/// Prometheus text exposition (design G2): limiter aggregates, the ranked
/// per-IP series capped at [`PER_IP_SERIES_LIMIT`], the startup integrity
/// summary and the runtime spot-check counters.
fn prometheus_text(web: &WebConfig) -> String {
    let now = Instant::now();
    let counters = web.login_limiter.counters(now);
    let series = web.login_limiter.snapshot(now);
    let (runtime, _) = web.runtime_snapshot();
    let mut out = String::new();
    push_login_aggregates(&mut out, &counters);
    push_login_ip_series(&mut out, &series, now);
    push_integrity_summary(&mut out, web.integrity());
    push_spot_summary(&mut out, &runtime);
    push_integrity_request_summary(&mut out, &web.request_integrity_snapshot());
    let (on_demand, _) = web.on_demand_snapshot();
    push_integrity_sweep_summary(&mut out, &on_demand);
    push_client_ip_summary(web, &mut out);
    push_state_summary(web, &mut out);
    out
}

fn push_login_aggregates(out: &mut String, counters: &Counters) {
    push_metric(
        out,
        "agentj_login_attempts_total",
        "counter",
        "Login attempts that reached the throttler.",
        counters.attempts,
    );
    push_metric(
        out,
        "agentj_login_failures_total",
        "counter",
        "Token check failures (locked-out attempts excluded).",
        counters.failures,
    );
    push_metric(
        out,
        "agentj_login_lockouts_total",
        "counter",
        "Lockouts newly triggered (extensions excluded).",
        counters.lockouts,
    );
    push_metric(
        out,
        "agentj_login_blocked_total",
        "counter",
        "Requests rejected while their source IP was locked out.",
        counters.blocked,
    );
    push_metric(
        out,
        "agentj_login_locked_ips",
        "gauge",
        "Source IPs currently locked out.",
        counters.locked_now,
    );
    push_metric(
        out,
        "agentj_login_tracked_ips",
        "gauge",
        "Source IPs currently tracked by the throttler.",
        counters.tracked_ips,
    );
}

fn push_login_ip_series(out: &mut String, snapshot: &[IpSnapshot], now: Instant) {
    let truncated = snapshot.len() > PER_IP_SERIES_LIMIT;
    let shown = &snapshot[..snapshot.len().min(PER_IP_SERIES_LIMIT)];
    push_ip_family(
        out,
        "agentj_login_ip_failures_total",
        "counter",
        "Token check failures per source IP (top series by lock state and failures).",
        shown,
        |s| u64::from(s.failures),
    );
    push_ip_family(
        out,
        "agentj_login_ip_locked",
        "gauge",
        "Whether the source IP is currently locked out (1) or not (0).",
        shown,
        |s| u64::from(s.locked),
    );
    push_ip_family(
        out,
        "agentj_login_ip_retry_after_seconds",
        "gauge",
        "Remaining lockout for the source IP in whole seconds (0 when unlocked).",
        shown,
        |s| s.retry_after.as_secs(),
    );
    push_ip_family(
        out,
        "agentj_login_ip_last_seen_seconds",
        "gauge",
        "Seconds since the source IP last reached the throttler.",
        shown,
        |s| now.duration_since(s.last_seen).as_secs(),
    );
    push_metric(
        out,
        "agentj_login_ip_series_truncated",
        "gauge",
        "Whether the per-IP series were capped at the Top-50 limit (1) or not (0).",
        u64::from(truncated),
    );
}

fn push_integrity_summary(out: &mut String, report: &IntegrityReport) {
    push_metric(
        out,
        "agentj_web_manifest_present",
        "gauge",
        "Whether the dist asset manifest was present and usable (1) or not (0).",
        u64::from(report.manifest_present),
    );
    push_metric(
        out,
        "agentj_web_assets_checked",
        "gauge",
        "Startup integrity check: assets hashed.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_assets_mismatched",
        "gauge",
        "Startup integrity check: digest mismatches.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_assets_missing",
        "gauge",
        "Startup integrity check: manifest entries missing on disk.",
        report.missing,
    );
}

/// Runtime spot-check counters (design G2), cumulative across rounds.
fn push_spot_summary(out: &mut String, report: &SpotCheckReport) {
    push_metric(
        out,
        "agentj_web_spot_rounds_total",
        "counter",
        "Web asset spot-check rounds completed.",
        report.rounds,
    );
    push_metric(
        out,
        "agentj_web_spot_checked_total",
        "counter",
        "Web asset spot-check: files hashed across rounds.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_spot_mismatched_total",
        "counter",
        "Web asset spot-check: digest mismatches across rounds.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_spot_missing_total",
        "counter",
        "Web asset spot-check: manifest entries missing on disk across rounds.",
        report.missing,
    );
}

/// Request-path verification series (design G2): how many served assets were
/// hashed, how many of those were tampered with or already gone, and how many
/// verdicts the memo currently holds.
fn push_integrity_request_summary(out: &mut String, snapshot: &RequestIntegritySnapshot) {
    push_metric(
        out,
        "agentj_web_integrity_request_checked_total",
        "counter",
        "Per-request web asset checks that read and hashed a file.",
        snapshot.checked,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_mismatched_total",
        "counter",
        "Per-request web asset checks that found a digest mismatch.",
        snapshot.mismatched,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_missing_total",
        "counter",
        "Per-request web asset checks whose manifest entry was missing on disk.",
        snapshot.missing,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_cache_entries",
        "gauge",
        "Memoised per-request verification verdicts currently held.",
        snapshot.cache_entries as u64,
    );
}

/// Manual-sweep series (design G3/G7): swept runs and their cumulative outcome.
fn push_integrity_sweep_summary(out: &mut String, report: &IntegrityOnDemandReport) {
    push_metric(
        out,
        "agentj_web_integrity_sweeps_total",
        "counter",
        "On-demand full asset sweeps actually executed.",
        report.sweeps,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_checked_total",
        "counter",
        "On-demand sweeps: files read and hashed.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_mismatched_total",
        "counter",
        "On-demand sweeps: digest mismatches.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_missing_total",
        "counter",
        "On-demand sweeps: manifest entries missing on disk.",
        report.missing,
    );
}

fn push_metric(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    out.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"
    ));
}

/// Resolved client-IP source tallies plus the unusable-forwarded-header count
/// (design D6/G2). Labelled family: one header, then one sample per source.
fn push_client_ip_summary(web: &WebConfig, out: &mut String) {
    let totals = *web.ip_sources.read().unwrap_or_else(|e| e.into_inner());
    out.push_str(
        "# HELP agentj_web_client_ip_source_total Resolved client-IP sources.\n\
         # TYPE agentj_web_client_ip_source_total counter\n",
    );
    for (source, value) in [
        (IpSource::Peer, totals.peer),
        (IpSource::XForwardedFor, totals.forwarded),
        (IpSource::XRealIp, totals.real_ip),
    ] {
        out.push_str(&format!(
            "agentj_web_client_ip_source_total{{source=\"{}\"}} {value}\n",
            source.label()
        ));
    }
    push_metric(
        out,
        "agentj_web_proxy_header_invalid_total",
        "counter",
        "Trusted requests whose forwarded headers were present but unusable.",
        totals.invalid,
    );
}

/// Persisted login-limiter state lifecycle (design D3/G2).
fn push_state_summary(web: &WebConfig, out: &mut String) {
    let report = *web.state_report.read().unwrap_or_else(|e| e.into_inner());
    push_metric(
        out,
        "agentj_web_metrics_state_restored",
        "gauge",
        "Whether persisted login-limiter state was restored at startup (1) or not (0).",
        u64::from(report.restored),
    );
    push_metric(
        out,
        "agentj_web_metrics_state_entries_restored",
        "gauge",
        "Per-IP login-limiter entries restored at startup.",
        report.restored_entries,
    );
    push_metric(
        out,
        "agentj_web_metrics_state_write_failures_total",
        "counter",
        "Failed login-limiter state flushes since startup.",
        report.write_failures,
    );
    push_metric(
        out,
        "agentj_web_metrics_state_last_flush_unix",
        "gauge",
        "Wall-clock seconds of the last successful state flush (0 = never).",
        report.last_flush_unix,
    );
}

/// Family header followed by one labelled sample per shown IP; the text format
/// wants each family's samples contiguous, so callers loop one family at a time.
fn push_ip_family(
    out: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    series: &[IpSnapshot],
    value: impl Fn(&IpSnapshot) -> u64,
) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
    for snap in series {
        out.push_str(&format!(
            "{name}{{ip=\"{}\"}} {}\n",
            escape_label(&snap.ip.to_string()),
            value(snap)
        ));
    }
}

/// Escape a Prometheus label value: `\`, `"` and newlines (text format 0.0.4).
fn escape_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// Build the HTTP router for one accepted connection: the `/ws` upgrade route
/// plus — when a dist was resolved — the statically gated SPA surface. `/ws`,
/// `/login`, `/logout`, `/version` and `/metrics` are never behind the static
/// gate (no redirect loop; the probes stay reachable for remote peers and the
/// metrics endpoint gates itself with a plain `401`).
pub fn build_router(
    state: std::sync::Arc<crate::ws::WsServerState>,
    peer: std::net::SocketAddr,
) -> axum::Router {
    let ws_route = axum::routing::get({
        let state = state.clone();
        move |axum::extract::Query(q): axum::extract::Query<crate::ws::WsQuery>,
              ws: axum::extract::WebSocketUpgrade,
              headers: axum::http::HeaderMap| {
            let state = state.clone();
            async move { crate::ws::ws_upgrade(state, q, peer, ws, headers).await }
        }
    });
    let version_route = {
        let web = state.web.clone();
        move || {
            let web = web.clone();
            async move { version_handler(web).await }
        }
    };
    let metrics_route = {
        let web = state.web.clone();
        move |headers: axum::http::HeaderMap| {
            let web = web.clone();
            async move { metrics_handler(web, peer, headers).await }
        }
    };
    let integrity_verify_route = {
        let web = state.web.clone();
        move |headers: axum::http::HeaderMap| {
            let web = web.clone();
            async move { integrity_verify_handler(web, peer, headers).await }
        }
    };
    let mut router = axum::Router::new()
        .route("/ws", ws_route)
        .route("/version", axum::routing::get(version_route))
        .route("/metrics", axum::routing::get(metrics_route))
        .route(
            "/integrity/verify",
            axum::routing::post(integrity_verify_route),
        );
    router = auth_routes(router, state.web.clone(), peer);
    if let Some(dist) = state.web.dist.clone() {
        router = static_surface(router, state.web.clone(), peer, &dist);
    }
    router
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    /// True when `body` has a whole line equal to `expected`, so an exact-value
    /// check cannot be satisfied by a longer number (`" 5"` vs `" 50"`).
    fn has_metric_line(body: &str, expected: &str) -> bool {
        body.lines()
            .any(|line| line.trim_end_matches('\r') == expected)
    }

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
    fn integrity_options_defaults_match_the_documented_values() {
        let opts = IntegrityOptions::default();
        assert_eq!(opts.spot_interval, Duration::from_secs(60));
        assert_eq!(opts.spot_sample, 8);
        assert!(opts.verify_on_request);
        assert_eq!(opts.verify_cache_ttl, Duration::from_secs(60));
    }

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
    fn loopback_detection_covers_v4_mapped_peers() {
        assert!(peer_is_loopback("127.0.0.1:5555".parse().unwrap()));
        assert!(peer_is_loopback("[::1]:5555".parse().unwrap()));
        assert!(peer_is_loopback("[::ffff:127.0.0.1]:5555".parse().unwrap()));
        assert!(!peer_is_loopback("[::ffff:8.8.8.8]:1".parse().unwrap()));
        assert!(!peer_is_loopback("203.0.113.7:41000".parse().unwrap()));
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
    fn pct_encode_escapes_query_delimiters() {
        assert_eq!(pct_encode("/a b?c&d"), "/a%20b%3Fc%26d");
    }

    fn temp_dist(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentj-web-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<html>agentj-index</html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
        dir
    }

    fn write_version_json(dir: &std::path::Path, product: &str, protocol: &str) {
        std::fs::write(
            dir.join("version.json"),
            format!(
                "{{\"product_version\":\"{product}\",\"protocol_version\":\"{protocol}\",\
                 \"build_time\":\"t\"}}"
            ),
        )
        .unwrap();
    }

    /// `sha384-<base64>` digest in the SRI format the build manifest uses.
    fn sha384_b64(bytes: &[u8]) -> String {
        use base64::Engine as _;
        use sha2::Digest as _;
        format!(
            "sha384-{}",
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha384::digest(bytes))
        )
    }

    /// Write an `asset-manifest.json` with the given `/`-prefixed entries.
    fn write_manifest(dir: &std::path::Path, entries: &[(&str, &str)]) {
        let assets: std::collections::BTreeMap<&str, &str> = entries.iter().copied().collect();
        let json = serde_json::json!({ "algorithm": "sha384", "assets": assets });
        std::fs::write(dir.join("asset-manifest.json"), json.to_string()).unwrap();
    }

    /// A manifest matching the bytes `temp_dist` writes by default.
    fn write_temp_dist_manifest(dir: &std::path::Path) {
        let index = sha384_b64(b"<html>agentj-index</html>");
        let app = sha384_b64(b"console.log(1)");
        write_manifest(dir, &[("/index.html", &index), ("/assets/app.js", &app)]);
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
        let web =
            WebConfig::new(Some(dist.clone()), None).with_integrity_options(IntegrityOptions {
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
    async fn version_omits_the_on_demand_age_before_any_sweep() {
        let dist = temp_dist("version-ondemand");
        write_temp_dist_manifest(&dist);
        let state = test_state(WebConfig::new(Some(dist.clone()), None));
        let local: std::net::SocketAddr = LOOPBACK.parse().unwrap();
        let (status, _, body) =
            get(crate::web::build_router(state.clone(), local), "/version").await;
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
        let entries: Vec<(&str, &str)> =
            keys.iter().map(|k| (k.as_str(), digest.as_str())).collect();
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
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap_or_default();
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
        let web =
            WebConfig::new(Some(dist.clone()), None).with_integrity_options(IntegrityOptions {
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
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap_or_default();
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
            let _cache = poisoned.verify_cache.lock().unwrap();
            let _warned = poisoned.warned.lock().unwrap();
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
        web.warned
            .lock()
            .unwrap()
            .insert("/assets/b.js".to_string());
        let repeated = web.run_spot_round(); // /assets/b.js again
        assert_eq!(repeated.mismatched, 1);
        assert!(repeated.offenses.is_empty());
        std::fs::remove_dir_all(&dist).ok();
    }

    /// Bounded wait on the spawned spot task: `interval` fires its first tick
    /// immediately, so a 20ms schedule reaches the target well within 2s. Real
    /// time instead of `start_paused` — tokio's `test-util` feature is not a
    /// dependency of this crate (plan T4 fallback).
    async fn wait_for_spot_rounds(web: &WebConfig, target: u64) {
        let reached = tokio::time::timeout(Duration::from_secs(2), async {
            while web.runtime_snapshot().0.rounds < target {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(
            reached.is_ok(),
            "spot task did not reach {target} rounds within 2s"
        );
    }

    fn msec_spot(ms: u64, sample: usize) -> SpotCheckConfig {
        SpotCheckConfig {
            interval: Duration::from_millis(ms),
            sample,
        }
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

    /// WARN-level capture for the current thread. `#[tokio::test]` defaults to
    /// the current-thread runtime, so the spawned spot task is polled on this
    /// thread and its warnings land in the buffer (mirrors the `login_limiter`
    /// capture helper).
    #[derive(Clone, Default)]
    struct WarnCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for WarnCapture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for WarnCapture {
        type Writer = WarnCapture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture_warnings() -> (WarnCapture, tracing::subscriber::DefaultGuard) {
        let buf = WarnCapture::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish();
        (buf, tracing::subscriber::set_default(sub))
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
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap_or_default();
        assert!(out.contains("web asset spot check failed"), "{out}");
        assert_eq!(
            out.matches("/assets/app.js").count(),
            1,
            "persistent offense must warn exactly once: {out}"
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

    fn test_state(web: crate::web::WebConfig) -> std::sync::Arc<crate::ws::WsServerState> {
        let kernel = agentj_core::kernel::Kernel::new();
        let session: std::sync::Arc<dyn agentj_core::module::SessionStore> =
            std::sync::Arc::new(agentj_session::InMemoryStore::new());
        std::sync::Arc::new(crate::ws::WsServerState {
            shared: std::sync::Arc::new(crate::IpcServer::new(kernel, session)),
            token: None,
            max_connections: 16,
            active_ws: std::sync::atomic::AtomicUsize::new(0),
            web: std::sync::Arc::new(web),
        })
    }

    async fn send(
        router: axum::Router,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        use tower::util::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = if body.is_empty() {
            builder.body(axum::body::Body::empty()).unwrap()
        } else {
            builder
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(axum::body::Body::from(body.to_string()))
                .unwrap()
        };
        let res = router.oneshot(request).await.unwrap();
        let (status, response_headers) = (res.status(), res.headers().clone());
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            response_headers,
            String::from_utf8_lossy(&bytes).to_string(),
        )
    }

    async fn get_with_headers(
        router: axum::Router,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        send(router, "GET", path, headers, "").await
    }

    async fn form_post_with_headers(
        router: axum::Router,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        send(router, "POST", path, headers, body).await
    }

    async fn get(
        router: axum::Router,
        path: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        get_with_headers(router, path, &[]).await
    }

    async fn form_post(
        router: axum::Router,
        path: &str,
        body: &str,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        form_post_with_headers(router, path, &[], body).await
    }

    const LOOPBACK: &str = "127.0.0.1:41000";
    const FORWARDED_CLIENT: &str = "198.51.100.9";

    /// T6 asserts on a *live* lock (`agentj_login_ip_locked`, `blocked_total`).
    /// The shared `small_policy()` locks for only 50 ms, which turns those
    /// assertions into wall-clock races, so T6 uses a same-shape policy with a
    /// 10 s base lockout: every lock below is still held when `/metrics` is read.
    fn stable_policy() -> crate::login_limiter::Policy {
        crate::login_limiter::Policy {
            free_attempts: 3,
            base_lockout: std::time::Duration::from_secs(10),
            max_lockout: std::time::Duration::from_secs(900),
            idle_ttl: std::time::Duration::from_secs(60),
            max_tracked: 16,
        }
    }

    fn trusted_web() -> crate::web::WebConfig {
        crate::web::WebConfig::new(None, Some("secret".into()))
            .with_login_limiter(crate::login_limiter::LoginLimiter::with_policy(
                stable_policy(),
            ))
            .with_trust_proxy_headers(true)
    }

    /// `None` sends **no** `x-forwarded-for` header at all; `Some("")` would send an
    /// empty one, which the resolver counts as "header present but unusable".
    async fn login_failure(
        state: &std::sync::Arc<crate::ws::WsServerState>,
        peer: &str,
        forwarded: Option<&str>,
    ) {
        let headers: Vec<(&str, &str)> = forwarded
            .map(|value| vec![("x-forwarded-for", value)])
            .unwrap_or_default();
        let _ = form_post_with_headers(
            crate::web::build_router(state.clone(), peer.parse().unwrap()),
            "/login",
            &headers,
            "token=wrong&next=%2F",
        )
        .await;
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

    /// Two failed attempts stay 401, the third locks (50ms base for fast tests).
    fn small_policy() -> crate::login_limiter::Policy {
        crate::login_limiter::Policy {
            free_attempts: 3,
            base_lockout: Duration::from_millis(50),
            max_lockout: Duration::from_millis(400),
            idle_ttl: Duration::from_secs(60),
            max_tracked: 16,
        }
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
            web.warned
                .lock()
                .unwrap()
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
        let report = *web.state_report.read().unwrap();
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
        let report = *web.state_report.read().unwrap();
        assert!(report.last_flush_unix > 0);
        assert_eq!(report.write_failures, 0);
    }

    #[tokio::test]
    async fn integrity_metrics_reflect_a_request_check_and_a_sweep() {
        let dist = temp_dist("metrics-integrity");
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
}
