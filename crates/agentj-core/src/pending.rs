//! Single storage primitive for "requests awaiting an out-of-band answer".
//!
//! Permission approvals and questions both keep a table of entries keyed by
//! request id, owned by a session, cloned out for readers, and pruned either by
//! a predicate (the waiter consumed the entry) or by age. Duplicating that table
//! twice invited the two copies to drift; this registry is the one
//! implementation both modules back onto.
//!
//! Policy notes:
//! - `insert` never overwrites: the first writer wins, so a duplicate request id
//!   cannot steal another waiter's decision.
//! - A TTL is optional; `None` means entries live until an explicit
//!   `remove`/`prune_where`/`drain`. Expiry is only swept on `insert` and
//!   `prune_expired`, so a quiet table is not rewritten in the background.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::sync::lock_or_recover;

struct Entry<P> {
    payload: P,
    session_id: String,
    created_at: Instant,
}

/// A keyed table of pending entries, each tagged with its owning session.
pub struct PendingRegistry<K: Eq + Hash + Clone, P> {
    entries: Mutex<HashMap<K, Entry<P>>>,
    ttl: Option<Duration>,
}

impl<K: Eq + Hash + Clone, P: Clone> PendingRegistry<K, P> {
    /// Create an empty registry; `Some(ttl)` enables age-based sweeping.
    pub fn new(ttl: Option<Duration>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Register `payload` under `key`. Returns `false` when the key already
    /// exists (the first payload stays authoritative). Expired entries are
    /// swept first so a stale key cannot block a live one.
    pub fn insert(&self, key: K, session_id: &str, payload: P) -> bool {
        self.prune_expired();
        let mut entries = lock_or_recover(&self.entries);
        if entries.contains_key(&key) {
            return false;
        }
        let entry = Entry {
            payload,
            session_id: session_id.to_string(),
            created_at: Instant::now(),
        };
        entries.insert(key, entry);
        true
    }

    /// Clone the payload for `key`, if still pending.
    pub fn get(&self, key: &K) -> Option<P> {
        let entries = lock_or_recover(&self.entries);
        entries.get(key).map(|entry| entry.payload.clone())
    }

    /// Take the payload out of the table; the second call returns `None`.
    pub fn remove(&self, key: &K) -> Option<P> {
        let mut entries = lock_or_recover(&self.entries);
        entries.remove(key).map(|entry| entry.payload)
    }

    /// Whether `key` is currently pending.
    pub fn contains(&self, key: &K) -> bool {
        let entries = lock_or_recover(&self.entries);
        entries.contains_key(key)
    }

    /// Number of pending entries (expired entries are not swept here).
    pub fn len(&self) -> usize {
        let entries = lock_or_recover(&self.entries);
        entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The session that owns `key`, if still pending.
    pub fn session_of(&self, key: &K) -> Option<String> {
        let entries = lock_or_recover(&self.entries);
        entries.get(key).map(|entry| entry.session_id.clone())
    }

    /// Visit every entry owned by `session_id` without consuming it; returns
    /// how many were visited.
    pub fn for_session(&self, session_id: &str, mut f: impl FnMut(&K, &P)) -> usize {
        let entries = lock_or_recover(&self.entries);
        let mut visited = 0;
        for (key, entry) in entries.iter() {
            if entry.session_id == session_id {
                f(key, &entry.payload);
                visited += 1;
            }
        }
        visited
    }

    /// Drop every entry for which `pred` returns `true`; returns how many went.
    pub fn prune_where(&self, mut pred: impl FnMut(&K, &P) -> bool) -> usize {
        let mut entries = lock_or_recover(&self.entries);
        let before = entries.len();
        entries.retain(|key, entry| !pred(key, &entry.payload));
        before - entries.len()
    }

    /// Drop entries older than the configured TTL; returns how many went.
    pub fn prune_expired(&self) -> usize {
        let Some(ttl) = self.ttl else {
            return 0;
        };
        let mut entries = lock_or_recover(&self.entries);
        let before = entries.len();
        entries.retain(|_, entry| entry.created_at.elapsed() < ttl);
        before - entries.len()
    }

    /// Remove and return every pending entry (used on shutdown).
    pub fn drain(&self) -> Vec<(K, P)> {
        let mut entries = lock_or_recover(&self.entries);
        entries
            .drain()
            .map(|(key, entry)| (key, entry.payload))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn insert_remove_is_single_winner() {
        let reg: PendingRegistry<String, u8> = PendingRegistry::new(None);
        assert!(reg.insert("a".into(), "s1", 1));
        assert!(!reg.insert("a".into(), "s1", 2));
        assert_eq!(reg.get(&"a".into()), Some(1));
        assert_eq!(reg.remove(&"a".into()), Some(1));
        assert_eq!(reg.remove(&"a".into()), None);
    }

    #[test]
    fn for_session_and_prune_where_do_not_consume() {
        let reg: PendingRegistry<String, bool> = PendingRegistry::new(None);
        reg.insert("a".into(), "s1", true);
        reg.insert("b".into(), "s2", false);
        assert_eq!(reg.for_session("s1", |_, _| {}), 1);
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.prune_where(|_, v| *v), 1);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.session_of(&"b".into()), Some("s2".to_string()));
    }

    #[test]
    fn expired_entries_drop_on_insert() {
        let reg: PendingRegistry<String, u8> = PendingRegistry::new(Some(Duration::from_millis(1)));
        reg.insert("a".into(), "s1", 1);
        std::thread::sleep(Duration::from_millis(5));
        reg.insert("b".into(), "s1", 2);
        assert_eq!(reg.len(), 1);
        assert!(reg.contains(&"b".into()));
    }
}
