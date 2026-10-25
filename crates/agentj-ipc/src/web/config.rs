//! `WebConfig`: the resolved static-web-UI runtime config plus every mutable
//! counter, cache and task handle the HTTP surface reads (D3, no persistence).
//!
//! Persisting the login-limiter state lives in the sibling `persist` module;
//! this module only owns construction, clamping and the read-only accessors.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

use crate::login_limiter::LoginLimiter;
use agentj_core::sync::{lock_or_recover, read_or_recover, write_or_recover};

use super::*;

/// Static-web-UI runtime config: resolved dist directory plus the optional
/// access token and the in-memory set of issued sessions (D3, no persistence).
pub struct WebConfig {
    pub dist: Option<std::path::PathBuf>,
    pub token: Option<String>,
    pub(crate) dist_version: Option<DistVersion>,
    pub(crate) dist_version_error: Option<String>,
    pub(crate) integrity: IntegrityReport,
    pub(crate) integrity_error: Option<String>,
    pub(crate) manifest: Option<AssetManifest>,
    pub(crate) spot: SpotCheckConfig,
    pub(crate) verify_on_request: bool,
    pub(crate) verify_cache_ttl: Duration,
    pub(crate) verify_cache: std::sync::Mutex<std::collections::HashMap<String, VerifyEntry>>,
    pub(crate) on_demand: std::sync::RwLock<IntegrityOnDemandReport>,
    pub(crate) request_checked: std::sync::atomic::AtomicU64,
    pub(crate) request_mismatched: std::sync::atomic::AtomicU64,
    pub(crate) request_missing: std::sync::atomic::AtomicU64,
    pub(crate) spot_report: std::sync::RwLock<SpotCheckReport>,
    pub(crate) warned: std::sync::Mutex<std::collections::HashSet<String>>,
    pub(crate) spot_cursor: std::sync::atomic::AtomicUsize,
    pub(crate) spot_task: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    pub(crate) sessions: std::sync::RwLock<std::collections::HashSet<String>>,
    pub(crate) login_limiter: LoginLimiter,
    pub(crate) trust_proxy_headers: bool,
    pub(crate) secure_cookies: bool,
    pub(crate) metrics_state: Option<std::path::PathBuf>,
    pub(crate) ip_sources: std::sync::RwLock<IpSourceTotals>,
    pub(crate) state_report: std::sync::RwLock<StateReport>,
    pub(crate) flush_task: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    pub(crate) flush_interval: Duration,
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
            secure_cookies: false,
            metrics_state: None,
            ip_sources: std::sync::RwLock::new(IpSourceTotals::default()),
            state_report: std::sync::RwLock::new(StateReport::default()),
            flush_task: std::sync::Mutex::new(None),
            flush_interval: Duration::from_secs(5),
        }
    }
    /// Mark session cookies `Secure` (enable when TLS terminates in front of the
    /// kernel). Off by default so plain `http://127.0.0.1` deployments keep working.
    pub fn with_secure_cookies(mut self, secure: bool) -> Self {
        self.secure_cookies = secure;
        self
    }
    #[cfg(test)]
    pub(crate) fn secure_cookies_enabled(&self) -> bool {
        self.secure_cookies
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
            let cache = lock_or_recover(&self.verify_cache);
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
        let mut cache = lock_or_recover(&self.verify_cache);
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
        let mut warned = lock_or_recover(&self.warned);
        if warned.insert(path.to_string()) {
            tracing::warn!(path = %path, "web asset integrity check failed");
        }
    }
    /// The memoised verdict for `key`, for tests that need to see the cache.
    #[cfg(test)]
    pub(super) fn verify_cache_verdict(&self, key: &str) -> Option<AssetCheck> {
        agentj_core::sync::lock_or_recover(&self.verify_cache)
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
        let mut totals = write_or_recover(&self.ip_sources);
        match source {
            crate::client_ip::IpSource::Peer => totals.peer += 1,
            crate::client_ip::IpSource::XForwardedFor => totals.forwarded += 1,
            crate::client_ip::IpSource::XRealIp => totals.real_ip += 1,
        }
        if invalid {
            totals.invalid += 1;
        }
    }
    pub(super) fn warn_once(&self, key: &str, message: &str) {
        let first = agentj_core::sync::lock_or_recover(&self.warned).insert(key.to_string());
        if first {
            tracing::warn!("{message}");
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
            let mut report = write_or_recover(&self.spot_report);
            report.rounds += 1;
            report.checked += round.checked;
            report.mismatched += round.mismatched;
            report.missing += round.missing;
            report.last_run = Some(Instant::now());
        }
        let warned = lock_or_recover(&self.warned);
        round.offenses.retain(|path| !warned.contains(path));
        round
    }
    /// The runtime spot-check report plus the age of its last round in whole
    /// seconds; the age is `None` until the first round has run (design G7).
    pub(crate) fn runtime_snapshot(&self) -> (SpotCheckReport, Option<u64>) {
        let report = agentj_core::sync::read_or_recover(&self.spot_report).clone();
        let age = report
            .last_run
            .map(|t| Instant::now().duration_since(t).as_secs());
        (report, age)
    }
    /// Request-path verification counters plus the current memo size (design G2).
    pub(crate) fn request_integrity_snapshot(&self) -> RequestIntegritySnapshot {
        let cache_entries = agentj_core::sync::lock_or_recover(&self.verify_cache).len();
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
        let report = agentj_core::sync::read_or_recover(&self.on_demand).clone();
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
            let mut on_demand = write_or_recover(&self.on_demand);
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
                    let first =
                        agentj_core::sync::lock_or_recover(&web.warned).insert(path.clone());
                    if first {
                        tracing::warn!(path = %path, "web asset spot check failed");
                    }
                }
            }
        });
        let abort = handle.abort_handle();
        *lock_or_recover(&self.spot_task) = Some(abort);
        Some(handle)
    }
    /// Abort the periodic spot-check task, if one is running (server shutdown,
    /// tests).
    pub(crate) fn stop_spot_check(&self) {
        let task = agentj_core::sync::lock_or_recover(&self.spot_task).take();
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
        write_or_recover(&self.sessions).insert(id.clone());
        id
    }
    pub fn is_valid_session(&self, id: &str) -> bool {
        read_or_recover(&self.sessions).contains(id)
    }
    pub fn revoke_session(&self, id: &str) {
        write_or_recover(&self.sessions).remove(id);
    }
}
