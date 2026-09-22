//! Login failure throttling (design D2/D3): per-IP counters + exponential
//! lockout, in-memory like the session set; `now` is injected for testability.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
        }
    }

    pub(crate) fn check(&self, ip: IpAddr, now: Instant) -> Throttle {
        self.counters.attempts.fetch_add(1, Ordering::Relaxed);
        let mut entries = self.entries.lock().expect("login limiter");
        Self::prune_idle(&mut entries, ip, self.policy.idle_ttl, now);
        entries
            .get(&ip)
            .map_or(Throttle::Allow, |e| Self::status(e, now))
    }

    pub(crate) fn record_failure(&self, ip: IpAddr, now: Instant) -> Throttle {
        let mut entries = self.entries.lock().expect("login limiter");
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
            return Throttle::Allow;
        }
        let lockout = self.lockout_for(entry.failures);
        entry.locked_until = Some(now + lockout);
        if !was_locked {
            self.counters.lockouts.fetch_add(1, Ordering::Relaxed);
        }
        Throttle::Locked {
            retry_after: lockout,
        }
    }

    pub(crate) fn record_success(&self, ip: IpAddr) {
        self.entries.lock().expect("login limiter").remove(&ip);
    }

    pub(crate) fn counters(&self, now: Instant) -> Counters {
        let entries = self.entries.lock().expect("login limiter");
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
}
