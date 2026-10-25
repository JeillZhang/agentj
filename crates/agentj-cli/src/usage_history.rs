//! Persisted `provider/usage` samples: one JSON file per provider, written only when the
//! upstream was really contacted (design 4.1.1). Best effort by contract — every failure
//! warns and returns, never failing the request that triggered the sampling. @since 0.20.0

use std::path::PathBuf;

use agentj_protocol::provider::{UsageSample, UsageWindowKind};

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
    fn limits_are_clamped() {
        let h = UsageHistory::new(PathBuf::from("/tmp/unused")).with_limits(0, 1);
        assert_eq!(h.retention_days(), 1, "retention days clamp to 1..=365");
        assert_eq!(h.max_samples, 100, "max samples clamp to 100..=100_000");
    }
}
