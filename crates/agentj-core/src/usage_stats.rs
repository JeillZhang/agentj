//! Usage accounting for AgentJ (provisional minimum per T9; design-pending).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::SessionId;

/// Per-session token usage totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub turns: u64,
}

/// Collects per-session and global token usage totals.
///
/// Locks are poison-tolerant: a poisoned mutex yields empty/last-known data
/// instead of panicking.
pub struct UsageCollector {
    sessions: Mutex<HashMap<SessionId, SessionUsage>>,
}

impl UsageCollector {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Records one turn of token usage. A zero-token record is a no-op.
    pub fn record_token_usage(
        &self,
        session: SessionId,
        prompt_tokens: u64,
        completion_tokens: u64,
    ) {
        if prompt_tokens == 0 && completion_tokens == 0 {
            return;
        }
        if let Ok(mut map) = self.sessions.lock() {
            let usage = map.entry(session).or_default();
            usage.prompt_tokens += prompt_tokens;
            usage.completion_tokens += completion_tokens;
            usage.turns += 1;
        }
    }

    /// Returns accumulated totals for a session, or `None` if unknown.
    pub fn get_session_totals(&self, session: &str) -> Option<SessionUsage> {
        self.sessions
            .lock()
            .ok()
            .and_then(|map| map.get(session).copied())
    }

    /// Returns totals summed across all recorded sessions.
    pub fn get_global_totals(&self) -> SessionUsage {
        let mut total = SessionUsage::default();
        if let Ok(map) = self.sessions.lock() {
            for usage in map.values() {
                total.prompt_tokens += usage.prompt_tokens;
                total.completion_tokens += usage.completion_tokens;
                total.turns += usage.turns;
            }
        }
        total
    }

    /// Records one completed turn without token counts (providers do not
    /// report usage yet). Unlike record_token_usage this always creates the
    /// entry so turn counts stay observable.
    pub fn record_turn(&self, session: SessionId) {
        if let Ok(mut map) = self.sessions.lock() {
            let usage = map.entry(session).or_default();
            usage.turns += 1;
        }
    }
}

impl Default for UsageCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_accumulates_totals() {
        let collector = UsageCollector::new();
        collector.record_token_usage("s1".into(), 100, 20);
        collector.record_token_usage("s1".into(), 50, 30);
        collector.record_token_usage("s2".into(), 10, 5);

        let s1 = collector.get_session_totals("s1").unwrap();
        assert_eq!(s1.prompt_tokens, 150);
        assert_eq!(s1.completion_tokens, 50);
        assert_eq!(s1.turns, 2);

        let global = collector.get_global_totals();
        assert_eq!(global.prompt_tokens, 160);
        assert_eq!(global.completion_tokens, 55);

        assert!(collector.get_session_totals("nope").is_none());
    }

    #[test]
    fn zero_record_is_noop() {
        let collector = UsageCollector::new();
        collector.record_token_usage("s1".into(), 0, 0);
        assert!(
            collector.get_session_totals("s1").is_none(),
            "zero record must not create an entry"
        );
    }

    #[test]
    fn record_turn_counts_turns_without_tokens() {
        let collector = UsageCollector::new();
        collector.record_turn("s1".into());
        collector.record_turn("s1".into());
        let s1 = collector.get_session_totals("s1").unwrap();
        assert_eq!(s1.turns, 2);
        assert_eq!(s1.prompt_tokens, 0);
        assert_eq!(s1.completion_tokens, 0);

        let global = collector.get_global_totals();
        assert_eq!(global.turns, 2);
    }
}
