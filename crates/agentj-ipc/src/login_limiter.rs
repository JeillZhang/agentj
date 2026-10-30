//! Login failure throttling (design D2/D3): per-IP counters + exponential
//! lockout, in-memory like the session set; `now` is injected for testability.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::metrics_state::{self, CountersSnapshot, IpState, MetricsState, STATE_VERSION};
use agentj_core::sync::lock_or_recover;

#[derive(Clone, Debug)]
pub(crate) struct Policy {
    pub(crate) free_attempts: u32,
    pub(crate) base_lockout: Duration,
    pub(crate) max_lockout: Duration,
    pub(crate) idle_ttl: Duration,
    pub(crate) max_tracked: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            free_attempts: 5,
            base_lockout: Duration::from_secs(30),
            max_lockout: Duration::from_secs(900),
            idle_ttl: Duration::from_secs(1800),
            max_tracked: 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Throttle {
    Allow,
    Locked { retry_after: Duration },
}

#[derive(Debug)]
struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    last: Instant,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct IpSnapshot {
    pub(crate) ip: IpAddr,
    pub(crate) failures: u32,
    pub(crate) locked: bool,
    pub(crate) retry_after: Duration,
    pub(crate) last_seen: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Counters {
    pub(crate) attempts: u64,
    pub(crate) failures: u64,
    pub(crate) lockouts: u64,
    pub(crate) blocked: u64,
    pub(crate) locked_now: u64,
    pub(crate) tracked_ips: u64,
}

#[derive(Default)]
struct CounterAtoms {
    attempts: AtomicU64,
    failures: AtomicU64,
    lockouts: AtomicU64,
    blocked: AtomicU64,
}

pub(crate) struct LoginLimiter {
    policy: Policy,
    entries: Mutex<HashMap<IpAddr, Entry>>,
    counters: CounterAtoms,
    /// Set by every state mutation, cleared after a successful flush (design D3).
    dirty: AtomicBool,
    /// Bumped by every mutation so a flush can tell whether its snapshot is stale.
    generation: AtomicU64,
}

impl LoginLimiter {
    pub(crate) fn new() -> Self {
        Self::with_policy(Policy::default())
    }

    pub(crate) fn with_policy(policy: Policy) -> Self {
        Self {
            policy,
            entries: Mutex::new(HashMap::new()),
            counters: CounterAtoms::default(),
            dirty: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    pub(crate) fn check(&self, ip: IpAddr, now: Instant) -> Throttle {
        self.counters.attempts.fetch_add(1, Ordering::Relaxed);
        let throttle = {
            let mut entries = lock_or_recover(&self.entries);
            Self::prune_idle(&mut entries, ip, self.policy.idle_ttl, now);
            entries
                .get(&ip)
                .map_or(Throttle::Allow, |e| Self::status(e, now))
        };
        if let Throttle::Locked { retry_after } = &throttle {
            debug!(
                ip = %ip,
                retry_after_secs = retry_after.as_secs(),
                "login blocked while locked"
            );
        }
        self.mark_dirty();
        throttle
    }

    pub(crate) fn record_failure(&self, ip: IpAddr, now: Instant) -> Throttle {
        let mut entries = lock_or_recover(&self.entries);
        Self::prune_idle(&mut entries, ip, self.policy.idle_ttl, now);
        if !entries.contains_key(&ip) {
            Self::evict_oldest(&mut entries, self.policy.max_tracked);
        }
        let entry = entries.entry(ip).or_insert(Entry {
            failures: 0,
            locked_until: None,
            last: now,
        });
        let was_locked = matches!(Self::status(entry, now), Throttle::Locked { .. });
        if was_locked {
            self.counters.blocked.fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters.failures.fetch_add(1, Ordering::Relaxed);
        }
        entry.failures = entry.failures.saturating_add(1);
        entry.last = now;
        if entry.failures < self.policy.free_attempts {
            debug!(ip = %ip, failures = entry.failures, "login failure");
            self.mark_dirty();
            return Throttle::Allow;
        }
        let lockout = self.lockout_for(entry.failures);
        entry.locked_until = Some(now + lockout);
        let retry_after_secs = lockout.as_secs();
        if was_locked {
            debug!(ip = %ip, retry_after_secs, "login lockout extended");
        } else {
            self.counters.lockouts.fetch_add(1, Ordering::Relaxed);
            warn!(
                ip = %ip,
                failures = entry.failures,
                retry_after_secs,
                "login lockout"
            );
        }
        self.mark_dirty();
        Throttle::Locked {
            retry_after: lockout,
        }
    }

    pub(crate) fn record_success(&self, ip: IpAddr) {
        let prior_failures = lock_or_recover(&self.entries)
            .remove(&ip)
            .map_or(0, |entry| entry.failures);
        info!(ip = %ip, prior_failures, "login ok");
        self.mark_dirty();
    }

    pub(crate) fn counters(&self, now: Instant) -> Counters {
        let entries = lock_or_recover(&self.entries);
        let locked_now = entries
            .values()
            .filter(|e| matches!(Self::status(e, now), Throttle::Locked { .. }))
            .count() as u64;
        Counters {
            attempts: self.counters.attempts.load(Ordering::Relaxed),
            failures: self.counters.failures.load(Ordering::Relaxed),
            lockouts: self.counters.lockouts.load(Ordering::Relaxed),
            blocked: self.counters.blocked.load(Ordering::Relaxed),
            locked_now,
            tracked_ips: entries.len() as u64,
        }
    }

    /// Read-only projection under the existing entries lock (≤ max_tracked, no await).
    pub(crate) fn snapshot(&self, now: Instant) -> Vec<IpSnapshot> {
        let entries = lock_or_recover(&self.entries);
        let mut out: Vec<IpSnapshot> = entries
            .iter()
            .map(|(ip, entry)| {
                let locked = matches!(Self::status(entry, now), Throttle::Locked { .. });
                let retry_after = match Self::status(entry, now) {
                    Throttle::Locked { retry_after } => retry_after,
                    Throttle::Allow => Duration::ZERO,
                };
                IpSnapshot {
                    ip: *ip,
                    failures: entry.failures,
                    locked,
                    retry_after,
                    last_seen: entry.last,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.locked
                .cmp(&a.locked)
                .then(b.failures.cmp(&a.failures))
                .then(a.ip.cmp(&b.ip))
        });
        out
    }

    /// Whether anything changed since the last successful flush.
    pub(crate) fn dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_clean(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }

    /// Snapshot token for a flush: mutations that land after this reading make the
    /// in-flight snapshot stale, so `mark_clean_at` refuses to clear the flag.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Clear the dirty flag only when nothing changed since `generation` was read.
    pub(crate) fn mark_clean_at(&self, generation: u64) {
        if self.generation.load(Ordering::Relaxed) == generation {
            self.dirty.store(false, Ordering::Relaxed);
        }
    }

    fn mark_dirty(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Serialize the aggregate counters plus every per-IP entry (design D3):
    /// `Instant`s are converted against the caller's `now_unix` reading.
    pub(crate) fn to_state(&self, now: Instant, now_unix: u64) -> MetricsState {
        let ips = {
            let entries = lock_or_recover(&self.entries);
            entries
                .iter()
                .map(|(ip, entry)| IpState {
                    ip: ip.to_string(),
                    failures: entry.failures,
                    locked_until_unix: entry.locked_until.map(|until| {
                        now_unix.saturating_add(until.saturating_duration_since(now).as_secs())
                    }),
                    last_unix: now_unix
                        .saturating_sub(now.saturating_duration_since(entry.last).as_secs()),
                })
                .collect()
        };
        MetricsState {
            version: STATE_VERSION,
            saved_at_unix: now_unix,
            counters: CountersSnapshot {
                attempts: self.counters.attempts.load(Ordering::Relaxed),
                failures: self.counters.failures.load(Ordering::Relaxed),
                lockouts: self.counters.lockouts.load(Ordering::Relaxed),
                blocked: self.counters.blocked.load(Ordering::Relaxed),
            },
            ips,
        }
    }

    /// Replace the in-memory state with a persisted snapshot and return how many
    /// per-IP entries survived the conversion and the policy bounds (design D3).
    pub(crate) fn restore_from(&self, state: &MetricsState, now: Instant, now_unix: u64) -> usize {
        self.counters
            .attempts
            .store(state.counters.attempts, Ordering::Relaxed);
        self.counters
            .failures
            .store(state.counters.failures, Ordering::Relaxed);
        self.counters
            .lockouts
            .store(state.counters.lockouts, Ordering::Relaxed);
        self.counters
            .blocked
            .store(state.counters.blocked, Ordering::Relaxed);
        let seeds = metrics_state::restore_seeds(
            &state.ips,
            now_unix,
            self.policy.max_lockout,
            self.policy.idle_ttl,
            self.policy.max_tracked,
        );
        let restored = seeds.len();
        let mut entries = lock_or_recover(&self.entries);
        entries.clear();
        for (ip, seed) in seeds {
            entries.insert(
                ip,
                Entry {
                    failures: seed.failures,
                    locked_until: seed.locked_for.map(|remaining| now + remaining),
                    last: now.checked_sub(seed.last_age).unwrap_or(now),
                },
            );
        }
        self.mark_clean();
        restored
    }

    fn lockout_for(&self, failures: u32) -> Duration {
        let exp = failures.saturating_sub(self.policy.free_attempts).min(16);
        self.policy
            .base_lockout
            .saturating_mul(2u32.saturating_pow(exp))
            .min(self.policy.max_lockout)
    }

    fn status(entry: &Entry, now: Instant) -> Throttle {
        match entry.locked_until {
            Some(until) if until > now => Throttle::Locked {
                retry_after: until - now,
            },
            _ => Throttle::Allow,
        }
    }

    fn prune_idle(entries: &mut HashMap<IpAddr, Entry>, ip: IpAddr, ttl: Duration, now: Instant) {
        if entries
            .get(&ip)
            .is_some_and(|e| now.duration_since(e.last) > ttl)
        {
            entries.remove(&ip);
        }
    }

    fn evict_oldest(entries: &mut HashMap<IpAddr, Entry>, max: usize) {
        if entries.len() < max {
            return;
        }
        if let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, e)| e.last)
            .map(|(ip, _)| *ip)
        {
            entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> std::net::IpAddr {
        s.parse().unwrap()
    }
    fn at(t0: std::time::Instant, secs: u64) -> std::time::Instant {
        t0 + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn locks_on_fifth_failure_then_escalates_to_the_cap() {
        let limiter = LoginLimiter::new();
        let t0 = std::time::Instant::now();
        let who = ip("203.0.113.7");
        for i in 0..4 {
            assert_eq!(limiter.record_failure(who, at(t0, i)), Throttle::Allow);
        }
        assert_eq!(
            limiter.record_failure(who, at(t0, 4)),
            Throttle::Locked {
                retry_after: std::time::Duration::from_secs(30)
            }
        );
        assert!(matches!(
            limiter.check(who, at(t0, 5)),
            Throttle::Locked { .. }
        ));
        assert_eq!(
            limiter.record_failure(who, at(t0, 5)),
            Throttle::Locked {
                retry_after: std::time::Duration::from_secs(60)
            }
        );
        let mut last = std::time::Duration::ZERO;
        for i in 6..12 {
            if let Throttle::Locked { retry_after } = limiter.record_failure(who, at(t0, i)) {
                last = retry_after;
            }
        }
        assert_eq!(
            last,
            std::time::Duration::from_secs(900),
            "lockout caps at 15min"
        );
    }

    #[test]
    fn success_clears_counters_and_idle_entries_reset() {
        let limiter = LoginLimiter::new();
        let t0 = std::time::Instant::now();
        let who = ip("203.0.113.7");
        for i in 0..3 {
            let _ = limiter.record_failure(who, at(t0, i));
        }
        limiter.record_success(who);
        for i in 0..4 {
            assert_eq!(limiter.record_failure(who, at(t0, 10 + i)), Throttle::Allow);
        }
        // idle prune: advance past idle_ttl (30min default) -> counter restarts
        assert_eq!(limiter.record_failure(who, at(t0, 4000)), Throttle::Allow);
    }

    #[test]
    fn ips_are_isolated_and_loopback_counts() {
        let limiter = LoginLimiter::new();
        let t0 = std::time::Instant::now();
        for i in 0..5 {
            let _ = limiter.record_failure(ip("203.0.113.7"), at(t0, i));
        }
        assert!(matches!(
            limiter.check(ip("198.51.100.9"), at(t0, 5)),
            Throttle::Allow
        ));
        for i in 0..5 {
            let _ = limiter.record_failure(ip("127.0.0.1"), at(t0, i));
        }
        assert!(matches!(
            limiter.check(ip("127.0.0.1"), at(t0, 5)),
            Throttle::Locked { .. }
        ));
    }

    #[test]
    fn tracked_entries_are_bounded() {
        let policy = Policy {
            max_tracked: 2,
            ..Policy::default()
        };
        let limiter = LoginLimiter::with_policy(policy);
        let t0 = std::time::Instant::now();
        for i in 0..4 {
            let _ = limiter.record_failure(ip("10.0.0.1"), at(t0, i));
        }
        for i in 0..2 {
            let _ = limiter.record_failure(ip("10.0.0.2"), at(t0, 10 + i));
        }
        let _ = limiter.record_failure(ip("10.0.0.3"), at(t0, 20)); // evicts 10.0.0.1
        assert_eq!(
            limiter.record_failure(ip("10.0.0.1"), at(t0, 21)),
            Throttle::Allow,
            "evicted entry restarts from zero"
        );
    }

    #[test]
    fn counters_track_attempts_failures_lockouts_and_blocks() {
        let limiter = LoginLimiter::with_policy(Policy {
            free_attempts: 3,
            ..Policy::default()
        });
        let t0 = Instant::now();
        let ip = ip("198.51.100.9");
        for _ in 0..2 {
            let _ = limiter.check(ip, t0); // attempts += 1
            let _ = limiter.record_failure(ip, t0); // failures += 1
        }
        let snap = limiter.counters(t0);
        assert_eq!(
            (snap.attempts, snap.failures, snap.lockouts, snap.blocked),
            (2, 2, 0, 0)
        );
        let _ = limiter.check(ip, t0);
        assert!(matches!(
            limiter.record_failure(ip, t0),
            Throttle::Locked { .. }
        ));
        assert_eq!(
            limiter.counters(t0).lockouts,
            1,
            "3rd failure starts the lockout"
        );
        let _ = limiter.check(ip, t0); // blocked attempt
        let _ = limiter.record_failure(ip, t0); // escalation
        let snap = limiter.counters(t0);
        assert_eq!(
            (
                snap.blocked,
                snap.lockouts,
                snap.locked_now,
                snap.tracked_ips
            ),
            (1, 1, 1, 1)
        );
        limiter.record_success(ip);
        let snap = limiter.counters(t0);
        assert_eq!(
            (snap.failures, snap.tracked_ips),
            (3, 0),
            "cumulative counters never reset"
        );
    }

    #[test]
    fn snapshot_sorts_locked_first_then_failures_then_ip() {
        // free_attempts = 2: the 2nd failure locks, so locked_ip ends locked (failures=2)
        // while busy_ip stays allowed (failures=1) — a deterministic rank difference.
        let limiter = LoginLimiter::with_policy(Policy {
            free_attempts: 2,
            ..Policy::default()
        });
        let t0 = Instant::now();
        let locked_ip = ip("203.0.113.9");
        let busy_ip = ip("198.51.100.4");
        let _ = limiter.record_failure(locked_ip, t0);
        let _ = limiter.record_failure(locked_ip, t0); // locks (free_attempts = 2)
        let _ = limiter.record_failure(busy_ip, t0); // still allowed
        let snap = limiter.snapshot(t0);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].ip, locked_ip);
        assert!(snap[0].locked);
        assert!(snap[0].retry_after > Duration::ZERO);
        assert_eq!(snap[1].ip, busy_ip);
        assert!(!snap[1].locked);
        assert_eq!(snap[1].retry_after, Duration::ZERO);
        assert!(limiter.snapshot(t0).iter().all(|s| s.last_seen == t0));
    }

    /// Production-scale policy: whole-second lockout/ttl values, so a file
    /// round-trip (which stores whole seconds) keeps the lock window intact.
    fn restore_policy() -> Policy {
        Policy {
            free_attempts: 5,
            base_lockout: Duration::from_secs(30),
            max_lockout: Duration::from_secs(900),
            idle_ttl: Duration::from_secs(1800),
            max_tracked: 1024,
        }
    }

    #[test]
    fn to_state_round_trips_through_restore_from() {
        let limiter = LoginLimiter::with_policy(restore_policy());
        let now = Instant::now();
        let client = ip("203.0.113.9");
        let other = ip("198.51.100.4");
        for _ in 0..3 {
            limiter.check(client, now);
            limiter.record_failure(client, now);
        }
        // `check` alone creates no per-IP entry, so the second IP needs a failure.
        limiter.check(other, now);
        limiter.record_failure(other, now);
        let state = limiter.to_state(now, 1_700_000_000);
        assert_eq!(state.version, crate::metrics_state::STATE_VERSION);
        assert_eq!(state.saved_at_unix, 1_700_000_000);
        assert_eq!(state.counters.failures, 4);
        assert_eq!(state.ips.len(), 2);

        let restored = LoginLimiter::with_policy(restore_policy());
        let entries = restored.restore_from(&state, now, 1_700_000_000);
        assert_eq!(entries, 2);
        assert_eq!(restored.counters(now).failures, 4);
        assert_eq!(restored.counters(now).attempts, state.counters.attempts);
        assert_eq!(restored.snapshot(now).len(), 2);
        assert!(!restored.dirty(), "restoring must not mark the state dirty");
    }

    #[test]
    fn restore_from_drops_idle_entries_and_keeps_live_locks() {
        let now = Instant::now();
        let state = crate::metrics_state::MetricsState {
            version: crate::metrics_state::STATE_VERSION,
            saved_at_unix: 1_700_000_000,
            counters: crate::metrics_state::CountersSnapshot::default(),
            ips: vec![
                crate::metrics_state::IpState {
                    ip: "203.0.113.9".into(),
                    failures: 5,
                    locked_until_unix: Some(1_700_000_400),
                    last_unix: 1_700_000_000,
                },
                crate::metrics_state::IpState {
                    ip: "198.51.100.4".into(),
                    failures: 9,
                    locked_until_unix: None,
                    last_unix: 1_699_998_000, // age 2000s > idle_ttl 1800s
                },
            ],
        };
        let limiter = LoginLimiter::with_policy(restore_policy());
        assert_eq!(limiter.restore_from(&state, now, 1_700_000_000), 1);
        let snapshot = limiter.snapshot(now);
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].locked);
        assert_eq!(snapshot[0].failures, 5);
        assert!(snapshot[0].retry_after <= Duration::from_secs(400));
        assert!(
            snapshot[0].retry_after >= Duration::from_secs(399),
            "the live lock keeps its remaining window: {:?}",
            snapshot[0].retry_after
        );
    }

    #[test]
    fn restore_from_returns_the_restored_entry_count() {
        let now = Instant::now();
        let state = crate::metrics_state::MetricsState {
            version: crate::metrics_state::STATE_VERSION,
            saved_at_unix: 1_700_000_000,
            counters: crate::metrics_state::CountersSnapshot::default(),
            ips: (0..2000)
                .map(|n| crate::metrics_state::IpState {
                    ip: format!("10.{}.{}.1", n / 256, n % 256),
                    failures: 1,
                    locked_until_unix: None,
                    last_unix: 1_700_000_000,
                })
                .collect(),
        };
        let limiter = LoginLimiter::with_policy(restore_policy());
        let restored = limiter.restore_from(&state, now, 1_700_000_000);
        assert_eq!(restored, 1024, "truncated to the policy cap");
        assert_eq!(limiter.counters(now).tracked_ips, 1024);
    }

    #[test]
    fn dirty_marks_on_every_mutation_and_clears_on_mark_clean() {
        let limiter = LoginLimiter::with_policy(restore_policy());
        let now = Instant::now();
        assert!(!limiter.dirty(), "a fresh limiter has nothing to persist");
        limiter.check(ip("203.0.113.9"), now);
        assert!(limiter.dirty(), "check prunes and may bump attempts");
        limiter.mark_clean();
        assert!(!limiter.dirty());
        limiter.record_failure(ip("203.0.113.9"), now);
        assert!(limiter.dirty());
        limiter.mark_clean();
        limiter.record_success(ip("203.0.113.9"));
        assert!(limiter.dirty());
    }

    /// A flush that lost the race against a mutation must not clear the dirty flag:
    /// the counters it just wrote are already stale, so the next tick has to retry.
    #[test]
    fn mark_clean_at_keeps_dirty_when_the_generation_moved() {
        let limiter = LoginLimiter::with_policy(restore_policy());
        let now = Instant::now();
        limiter.record_failure(ip("203.0.113.9"), now);
        let captured = limiter.generation();
        limiter.record_failure(ip("203.0.113.9"), now);
        limiter.mark_clean_at(captured);
        assert!(
            limiter.dirty(),
            "a mutation during the save must keep the state dirty"
        );
        let latest = limiter.generation();
        limiter.mark_clean_at(latest);
        assert!(!limiter.dirty(), "an unchanged generation clears the flag");
    }

    #[test]
    fn restore_from_replaces_previous_entries() {
        let limiter = LoginLimiter::with_policy(restore_policy());
        let now = Instant::now();
        for _ in 0..3 {
            limiter.record_failure(ip("192.0.2.5"), now);
        }
        let state = crate::metrics_state::MetricsState {
            version: crate::metrics_state::STATE_VERSION,
            saved_at_unix: 1_700_000_000,
            counters: crate::metrics_state::CountersSnapshot {
                attempts: 4,
                failures: 4,
                lockouts: 0,
                blocked: 0,
            },
            ips: vec![crate::metrics_state::IpState {
                ip: "203.0.113.9".into(),
                failures: 4,
                locked_until_unix: None,
                last_unix: 1_700_000_000,
            }],
        };
        assert_eq!(limiter.restore_from(&state, now, 1_700_000_000), 1);
        let snapshot = limiter.snapshot(now);
        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[0].ip,
            ip("203.0.113.9"),
            "pre-restore entries are gone"
        );
        assert_eq!(limiter.counters(now).attempts, 4);
    }

    #[derive(Clone, Default)]
    struct CaptureWriter(std::sync::Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            lock_or_recover(&self.0).extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = CaptureWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Serializes the log-capture tests (see [`capture`] for why the subscriber is
    /// process-global rather than thread-local).
    static LOG_CAPTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Process-wide log sink, installed once by the first [`capture`].
    static CAPTURE_SINK: std::sync::OnceLock<CaptureWriter> = std::sync::OnceLock::new();

    /// One capture session.
    ///
    /// The subscriber is installed as the **process-global** default (once) instead of a
    /// thread-local one: `tracing::subscriber::set_default` rebuilds the global
    /// callsite-interest cache on every set/drop, which races with the other tests logging
    /// in parallel and can drop an event (seen as `info expected:` without the `login ok`
    /// line, ~1/30 stress runs). The shared sink is cleared per session under
    /// `LOG_CAPTURE`, so the two capture tests stay serialized; log lines from other tests
    /// may interleave into the buffer, which `contains`-style assertions tolerate.
    struct Capture {
        buf: CaptureWriter,
        _serial: std::sync::MutexGuard<'static, ()>,
    }

    fn capture() -> Capture {
        let serial = LOG_CAPTURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let buf = CAPTURE_SINK
            .get_or_init(|| {
                let buf = CaptureWriter::default();
                let sub = tracing_subscriber::fmt()
                    .with_writer(buf.clone())
                    .with_ansi(false)
                    .with_max_level(tracing::Level::DEBUG)
                    .finish();
                let _ = tracing::subscriber::set_global_default(sub);
                buf
            })
            .clone();
        lock_or_recover(&buf.0).clear();
        Capture {
            buf,
            _serial: serial,
        }
    }

    fn captured(buf: &CaptureWriter) -> String {
        String::from_utf8(lock_or_recover(&buf.0).clone()).unwrap()
    }

    #[test]
    fn lockout_emits_warn_and_success_emits_info() {
        let capture = capture();
        let limiter = LoginLimiter::with_policy(Policy {
            free_attempts: 1,
            ..Policy::default()
        });
        let t0 = Instant::now();
        let ip = ip("203.0.113.5");
        let _ = limiter.record_failure(ip, t0);
        limiter.record_success(ip);
        let out = captured(&capture.buf);
        assert!(out.contains("login lockout"), "warn expected: {out}");
        assert!(out.contains("ip=203.0.113.5"), "ip field expected: {out}");
        assert!(
            out.contains("retry_after_secs=30"),
            "retry_after_secs field expected: {out}"
        );
        assert!(out.contains("login ok"), "info expected: {out}");
        assert!(
            out.contains("prior_failures=1"),
            "prior_failures field expected: {out}"
        );
    }

    #[test]
    fn ordinary_failure_and_blocked_attempt_emit_debug_lines() {
        let capture = capture();
        let limiter = LoginLimiter::with_policy(Policy {
            free_attempts: 3,
            ..Policy::default()
        });
        let t0 = Instant::now();
        let who = ip("198.51.100.44");
        assert_eq!(limiter.record_failure(who, t0), Throttle::Allow);
        let _ = limiter.record_failure(who, at(t0, 1));
        let _ = limiter.record_failure(who, at(t0, 2)); // 3rd failure locks
        assert!(matches!(
            limiter.check(who, at(t0, 3)),
            Throttle::Locked { .. }
        ));
        let out = captured(&capture.buf);
        let failure_line = out
            .lines()
            .find(|line| line.contains("login failure"))
            .unwrap_or_else(|| panic!("DEBUG failure line expected: {out}"));
        assert!(
            failure_line.contains("DEBUG"),
            "ordinary failure must be DEBUG: {failure_line}"
        );
        let blocked_line = out
            .lines()
            .find(|line| line.contains("login blocked while locked"))
            .unwrap_or_else(|| panic!("DEBUG blocked line expected: {out}"));
        assert!(
            blocked_line.contains("DEBUG"),
            "blocked attempt must be DEBUG: {blocked_line}"
        );
    }
}
