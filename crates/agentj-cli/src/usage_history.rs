//! Persisted `provider/usage` samples: one JSON file per provider, written only when the
//! upstream was really contacted (design 4.1.1). Best effort by contract — every failure
//! warns and returns, never failing the request that triggered the sampling. @since 0.20.0

use std::path::{Path, PathBuf};

use agentj_protocol::provider::{UsageSample, UsageWindow, UsageWindowKind};

/// Default retention window in days (`usage.history_days` mirrors it). @since 0.20.0
pub const DEFAULT_RETENTION_DAYS: i64 = 30;
/// Default sample cap (`usage.history_max_samples` mirrors it). @since 0.20.0
pub const DEFAULT_MAX_SAMPLES: usize = 8640;
/// Upper bound of a `provider/usageHistory` `limit` (design 4.1.5). @since 0.20.0
pub const MAX_QUERY_LIMIT: u32 = 5000;
/// Two samples closer than this are collapsed when nothing changed (design 4.1.4).
pub const DEDUPE_WINDOW_SECS: i64 = 60;

const SECS_PER_DAY: i64 = 86_400;
const MAX_PROVIDER_NAME_LEN: usize = 64;
/// Schema version of `<provider>.json`; a bump means "ignore the old file", never migrate.
const SAMPLE_FILE_VERSION: u32 = 1;
const TMP_EXTENSION: &str = "json.tmp";

/// `<provider>.json`-safe name, or `None` when the provider id cannot be a filename.
/// Only `[a-z0-9._-]`, non-empty, ≤ 64 chars (design 4.1.1).
pub fn file_name(provider: &str) -> Option<String> {
    let usable = !provider.is_empty()
        && provider.len() <= MAX_PROVIDER_NAME_LEN
        && provider.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    usable.then(|| provider.to_string())
}

/// Drops samples older than `now - retention_days` (the boundary itself survives).
pub fn prune(samples: Vec<UsageSample>, now: i64, retention_days: i64) -> Vec<UsageSample> {
    let cutoff = now.saturating_sub(retention_days.max(1).saturating_mul(SECS_PER_DAY));
    samples.into_iter().filter(|s| s.at >= cutoff).collect()
}

/// Keeps the newest `max` samples, dropping the oldest (input order is irrelevant).
pub fn cap_len(samples: Vec<UsageSample>, max: usize) -> Vec<UsageSample> {
    if samples.len() <= max {
        return samples;
    }
    let mut sorted = samples;
    sorted.sort_by_key(|s| s.at);
    sorted.split_off(sorted.len() - max)
}

/// Merges two sample sets ordered by `at`; for an equal `at` the incoming sample wins, so a
/// later writer never loses the earlier one's samples and never keeps a stale duplicate
/// (design 4.1.4).
pub fn merge(existing: Vec<UsageSample>, incoming: Vec<UsageSample>) -> Vec<UsageSample> {
    // Incoming first: the stable sort then makes the incoming sample the kept one for an
    // equal `at`, and `dedup_by` drops the following duplicate.
    let mut out = incoming;
    out.extend(existing);
    out.sort_by_key(|s| s.at);
    out.dedup_by(|next, kept| next.at == kept.at);
    out
}

/// `true` when `next` repeats `last` within [`DEDUPE_WINDOW_SECS`] with identical windows —
/// the signal to skip the write (design 4.1.4). The first sample is never a repeat.
pub fn is_repeat(last: Option<&UsageSample>, next: &UsageSample) -> bool {
    last.is_some_and(|last| {
        next.at.saturating_sub(last.at) < DEDUPE_WINDOW_SECS && last.windows == next.windows
    })
}

/// Storage for one provider's samples; retention and query rules live in the free functions
/// above so they stay unit-testable without IO. @since 0.20.0
pub struct UsageHistory {
    dir: PathBuf,
    retention_days: i64,
    max_samples: usize,
}

impl UsageHistory {
    /// Default limits; [`Self::with_limits`] narrows them to the configured values.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_samples: DEFAULT_MAX_SAMPLES,
        }
    }

    /// Applies `usage.history_days` / `usage.history_max_samples`, clamping out-of-range values
    /// instead of failing startup.
    pub fn with_limits(mut self, retention_days: i64, max_samples: usize) -> Self {
        self.retention_days = retention_days.clamp(1, 365);
        self.max_samples = max_samples.clamp(100, 100_000);
        self
    }

    /// Retention actually applied, for `ProviderUsageHistoryResult::retention_days`.
    pub fn retention_days(&self) -> u32 {
        u32::try_from(self.retention_days).unwrap_or(u32::MAX)
    }

    /// `kind` keeps every sample that reported that window (windows stay intact), `since` drops
    /// older samples, and `limit` keeps the newest `limit.clamp(1, MAX_QUERY_LIMIT)` (design 4.1.5).
    pub fn filter(
        &self,
        samples: Vec<UsageSample>,
        kind: Option<UsageWindowKind>,
        since: Option<i64>,
        limit: Option<u32>,
    ) -> Vec<UsageSample> {
        // `Unknown` is what a forward client's unrecognized kind deserializes to; treating it as
        // a window subset would silently return no samples, so it filters nothing.
        let kind = kind.filter(|k| *k != UsageWindowKind::Unknown);
        let mut out: Vec<UsageSample> = samples
            .into_iter()
            .filter(|s| kind.is_none_or(|k| s.windows.iter().any(|w| w.kind == k)))
            .filter(|s| since.is_none_or(|since| s.at >= since))
            .collect();
        out.sort_by_key(|s| s.at);
        let keep = limit.map_or(out.len(), |l| l.clamp(1, MAX_QUERY_LIMIT) as usize);
        if out.len() > keep {
            out.drain(..out.len() - keep);
        }
        out
    }

    /// Appends one sampling of `windows` for `provider`: load, skip an unchanged repeat, merge,
    /// apply retention + cap, then rewrite the file atomically (design 4.1.4).
    pub fn record(&self, provider: &str, at: i64, windows: &[UsageWindow]) {
        let Some(path) = self.path(provider) else {
            tracing::warn!("usage history skipped: {provider:?} is not a safe file name");
            return;
        };
        let sample = UsageSample {
            at,
            windows: windows.to_vec(),
        };
        let existing = self.load(&path);
        if is_repeat(existing.iter().max_by_key(|s| s.at), &sample) {
            return;
        }
        let merged = merge(existing, vec![sample]);
        let keep = self.max_samples;
        let kept = cap_len(prune(merged, at, self.retention_days), keep);
        self.store(&path, provider, &kept);
    }

    /// Persisted samples of `provider` with `kind`, at or after `since`, newest `limit` kept.
    pub fn query(
        &self,
        provider: &str,
        kind: Option<UsageWindowKind>,
        since: i64,
        limit: u32,
    ) -> Vec<UsageSample> {
        let Some(path) = self.path(provider) else {
            return Vec::new();
        };
        self.filter(self.load(&path), kind, Some(since), Some(limit))
    }

    /// `<dir>/<provider>.json`, or `None` when the provider id cannot be a file name.
    fn path(&self, provider: &str) -> Option<PathBuf> {
        file_name(provider).map(|name| self.dir.join(format!("{name}.json")))
    }

    /// Stored samples, or an empty list when the file is missing or unusable — a broken history
    /// never fails the caller (design 4.1.1).
    fn load(&self, path: &Path) -> Vec<UsageSample> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("usage history unreadable ({path:?}): {error}");
                }
                return Vec::new();
            }
        };
        decode(&text).unwrap_or_else(|| {
            tracing::warn!("usage history unusable ({path:?}): starting from empty");
            Vec::new()
        })
    }

    /// Atomic replace of one provider's file; every failure warns instead of propagating.
    // Same discipline as agentj-ipc/src/metrics_state.rs::write (0700/0600/tmp+rename); kept
    // local on purpose — different schema, and that writer is bound to per-port state.
    fn store(&self, path: &Path, provider: &str, samples: &[UsageSample]) {
        if let Err(error) = write_history_file(&self.dir, path, provider, samples) {
            tracing::warn!("usage history write failed ({path:?}): {error}");
        }
    }
}

/// Creates `dir` as `0700`, writes the sample file through `*.json.tmp` + rename as `0600`,
/// and removes the tmp file whenever anything fails.
fn write_history_file(
    dir: &Path,
    path: &Path,
    provider: &str,
    samples: &[UsageSample],
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    set_private_dir(dir)?;
    let body = serde_json::json!({
        "version": SAMPLE_FILE_VERSION,
        "provider": provider,
        "samples": samples,
    });
    let tmp = path.with_extension(TMP_EXTENSION);
    let mut file = open_private(&tmp)?;
    let written = std::io::Write::write_all(&mut file, body.to_string().as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|error| discard_tmp(&tmp, error))?;
    std::fs::rename(&tmp, path).map_err(|error| discard_tmp(&tmp, error))
}

/// `Some(samples)` only for this schema version with a decodable `samples` array.
fn decode(text: &str) -> Option<Vec<UsageSample>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("version")?.as_u64()? != u64::from(SAMPLE_FILE_VERSION) {
        return None;
    }
    serde_json::from_value(value.get("samples")?.clone()).ok()
}

#[cfg(unix)]
fn set_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn open_private(tmp: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(tmp)
}

#[cfg(not(unix))]
fn open_private(tmp: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(tmp)
}

fn discard_tmp(tmp: &Path, error: std::io::Error) -> std::io::Error {
    // Cleanup failures must not mask the original error, but they are not silent either.
    if let Err(remove) = std::fs::remove_file(tmp)
        && remove.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("usage history tmp cleanup failed ({tmp:?}): {remove}");
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_protocol::provider::{UsageWindow, UsageWindowStatus};

    fn window(percent: u8) -> UsageWindow {
        UsageWindow {
            kind: UsageWindowKind::Rolling,
            status: UsageWindowStatus::Ok,
            percent: Some(percent),
            resets_at: None,
        }
    }

    fn weekly(percent: u8) -> UsageWindow {
        UsageWindow {
            kind: UsageWindowKind::Weekly,
            status: UsageWindowStatus::Ok,
            percent: Some(percent),
            resets_at: None,
        }
    }

    #[test]
    fn file_name_rejects_unsafe_provider_names() {
        assert_eq!(file_name("opencode-go"), Some("opencode-go".into()));
        assert_eq!(file_name("a.b_c-1"), Some("a.b_c-1".into()));
        assert_eq!(file_name(""), None);
        assert_eq!(file_name("../escape"), None);
        assert_eq!(file_name("UPPER"), None, "only [a-z0-9._-] is allowed");
        assert_eq!(
            file_name(&"x".repeat(65)),
            None,
            "names are capped at 64 chars"
        );
    }

    #[test]
    fn prune_drops_samples_older_than_the_retention_window() {
        let s = |at: i64| UsageSample {
            at,
            windows: vec![window(10)],
        };
        // now - 1 day = 200_000 - 86_400 = 113_600, so `100` falls out and the boundary stays.
        let kept = prune(vec![s(100), s(113_600), s(200_000)], 200_000, 1);
        assert_eq!(
            kept.iter().map(|x| x.at).collect::<Vec<_>>(),
            vec![113_600, 200_000]
        );
    }

    #[test]
    fn is_repeat_ignores_sub_minute_identical_samples() {
        let a = UsageSample {
            at: 1_000,
            windows: vec![window(10)],
        };
        let same = UsageSample {
            at: 1_030,
            windows: vec![window(10)],
        };
        let moved = UsageSample {
            at: 1_030,
            windows: vec![window(11)],
        };
        assert!(is_repeat(Some(&a), &same));
        assert!(!is_repeat(Some(&a), &moved));
        assert!(!is_repeat(None, &same), "first sample is never a repeat");
    }

    #[test]
    fn merge_keeps_the_newer_sample_for_the_same_second() {
        let old = UsageSample {
            at: 5,
            windows: vec![window(1)],
        };
        let new = UsageSample {
            at: 5,
            windows: vec![window(9)],
        };
        let other = UsageSample {
            at: 7,
            windows: vec![window(3)],
        };
        let out = merge(vec![old, other.clone()], vec![new]);
        assert_eq!(out.iter().map(|s| s.at).collect::<Vec<_>>(), vec![5, 7]);
        assert_eq!(out[0].windows[0].percent, Some(9));
    }

    #[test]
    fn filter_by_kind_since_and_limit() {
        let h = UsageHistory::new(PathBuf::from("/tmp/unused"));
        let samples = vec![
            UsageSample {
                at: 10,
                windows: vec![window(1)],
            },
            UsageSample {
                at: 20,
                windows: vec![weekly(2)],
            },
            UsageSample {
                at: 30,
                windows: vec![window(3)],
            },
        ];
        let by_kind = h.filter(samples.clone(), Some(UsageWindowKind::Rolling), None, None);
        assert_eq!(
            by_kind.iter().map(|s| s.at).collect::<Vec<_>>(),
            vec![10, 30]
        );
        let since = h.filter(samples.clone(), None, Some(20), None);
        assert_eq!(since.iter().map(|s| s.at).collect::<Vec<_>>(), vec![20, 30]);
        let limited = h.filter(samples, None, None, Some(2));
        assert_eq!(
            limited.iter().map(|s| s.at).collect::<Vec<_>>(),
            vec![20, 30],
            "limit keeps the newest"
        );
    }

    #[test]
    fn unknown_kind_filters_nothing() {
        let h = UsageHistory::new(PathBuf::from("/tmp/unused"));
        let samples = vec![
            UsageSample {
                at: 10,
                windows: vec![window(1)],
            },
            UsageSample {
                at: 20,
                windows: vec![weekly(2)],
            },
        ];
        // A forward client may send a kind this build does not know; it deserializes to
        // `Unknown`, which must not silently drop every sample (未知即不过滤).
        let out = h.filter(samples, Some(UsageWindowKind::Unknown), None, None);
        assert_eq!(out.iter().map(|s| s.at).collect::<Vec<_>>(), vec![10, 20]);
    }

    #[test]
    fn limits_are_clamped() {
        let h = UsageHistory::new(PathBuf::from("/tmp/unused")).with_limits(0, 1);
        assert_eq!(h.retention_days(), 1, "retention days clamp to 1..=365");
        assert_eq!(h.max_samples, 100, "max samples clamp to 100..=100_000");
    }

    /// Fixed "now" so the retention assertions do not depend on the wall clock.
    const NOW: i64 = 1_700_000_000;

    /// A per-test directory under the system temp dir; the caller removes it at the end.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aj-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn record_then_query_roundtrips_through_disk() {
        let dir = temp_dir("roundtrip");
        let h = UsageHistory::new(dir.clone());
        h.record("opencode-go", 1_000, &[window(10)]);
        h.record("opencode-go", 1_100, &[window(20)]);
        let again = UsageHistory::new(dir.clone()); // a fresh instance = a fresh process
        let out = again.query("opencode-go", None, 0, 100);
        assert_eq!(
            out.iter().map(|s| s.at).collect::<Vec<_>>(),
            vec![1_000, 1_100]
        );
        assert_eq!(out[1].windows[0].percent, Some(20));
        assert_eq!(
            std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0),
            1,
            "no tmp file left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_writes_a_private_file_inside_a_private_directory() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let dir = temp_dir("perms");
            let h = UsageHistory::new(dir.clone());
            h.record("p", 1_000, &[window(10)]);
            let mode = |path: &std::path::Path| {
                std::fs::metadata(path)
                    .map(|m| m.permissions().mode() & 0o777)
                    .ok()
            };
            assert_eq!(mode(&dir), Some(0o700), "history dir is 0700");
            assert_eq!(
                mode(&dir.join("p.json")),
                Some(0o600),
                "samples file is 0600"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn record_skips_repeats_within_the_dedupe_window() {
        let dir = temp_dir("dedupe");
        let h = UsageHistory::new(dir.clone());
        h.record("p", 1_000, &[window(10)]);
        h.record("p", 1_020, &[window(10)]); // same windows, < 60s later
        h.record("p", 1_100, &[window(10)]); // still the same value, but the window elapsed
        let out = h.query("p", None, 0, 100);
        assert_eq!(
            out.iter().map(|s| s.at).collect::<Vec<_>>(),
            vec![1_000, 1_100]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_ignored_and_overwritten() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("p.json"), b"not json").expect("seed corrupt file");
        let h = UsageHistory::new(dir.clone());
        assert!(
            h.query("p", None, 0, 100).is_empty(),
            "unusable file reads as empty"
        );
        h.record("p", 1_000, &[window(10)]);
        assert_eq!(h.query("p", None, 0, 100).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_is_a_no_op_for_an_unsafe_provider_name() {
        let dir = temp_dir("unsafe");
        let h = UsageHistory::new(dir.clone());
        h.record("../escape", 1_000, &[window(10)]);
        let written = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        assert_eq!(written, 0, "nothing is written outside the history dir");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retention_and_cap_apply_on_write() {
        let dir = temp_dir("retention");
        let h = UsageHistory::new(dir.clone()).with_limits(1, 100);
        // Vary the reported percent so no write is collapsed as a repeat.
        for i in 0..150 {
            h.record("p", NOW - 149 + i, &[window((i % 90) as u8)]);
        }
        let out = h.query("p", None, 0, 5_000);
        assert_eq!(out.len(), 100, "cap_len keeps at most max_samples");
        assert_eq!(out.last().map(|s| s.at), Some(NOW));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
