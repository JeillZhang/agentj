//! allow_always session state (design #17 — in-memory, session-scoped,
//! never written back to config).

use agentj_core::sync::lock_or_recover;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Identifies one approved (tool, rule) combination.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleFingerprint {
    pub tool: String,
    /// Command prefix for bash; normalized glob pattern for file tools.
    pub rule_key: String,
}

/// session_id → granted fingerprints. Session end clears the entry.
#[derive(Debug, Default)]
pub struct SessionAllowState {
    grants: Mutex<HashMap<String, HashSet<RuleFingerprint>>>,
}

impl SessionAllowState {
    pub fn new() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
        }
    }

    /// Record an allow_always grant for a session.
    pub fn grant(&self, session_id: &str, fingerprint: RuleFingerprint) {
        lock_or_recover(&self.grants)
            .entry(session_id.into())
            .or_default()
            .insert(fingerprint);
    }

    /// Whether this session previously granted the fingerprint.
    pub fn allows(&self, session_id: &str, fingerprint: &RuleFingerprint) -> bool {
        lock_or_recover(&self.grants)
            .get(session_id)
            .map(|set| set.contains(fingerprint))
            .unwrap_or(false)
    }

    /// Clear all grants for a session (session termination).
    pub fn clear_session(&self, session_id: &str) {
        lock_or_recover(&self.grants).remove(session_id);
    }

    /// Clear every session's grants (kernel shutdown).
    pub fn clear_all(&self) {
        lock_or_recover(&self.grants).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(tool: &str, key: &str) -> RuleFingerprint {
        RuleFingerprint {
            tool: tool.into(),
            rule_key: key.into(),
        }
    }

    #[test]
    fn grant_visible_within_session() {
        let state = SessionAllowState::new();
        let f = fp("bash", "npm test");
        state.grant("s1", f.clone());
        assert!(state.allows("s1", &f));
    }

    #[test]
    fn grant_not_visible_in_other_session() {
        let state = SessionAllowState::new();
        let f = fp("bash", "npm test");
        state.grant("s1", f.clone());
        assert!(!state.allows("s2", &f));
    }

    #[test]
    fn clear_session_removes_grants() {
        let state = SessionAllowState::new();
        let f = fp("bash", "npm test");
        state.grant("s1", f.clone());
        assert!(state.allows("s1", &f));
        state.clear_session("s1");
        assert!(!state.allows("s1", &f));
    }

    #[test]
    fn grants_are_distinct_per_fingerprint() {
        let state = SessionAllowState::new();
        state.grant("s1", fp("bash", "npm test"));
        assert!(!state.allows("s1", &fp("bash", "rm -rf")));
    }

    #[test]
    fn clear_all_removes_every_session() {
        let state = SessionAllowState::new();
        state.grant("s1", fp("bash", "a"));
        state.grant("s2", fp("bash", "b"));
        state.clear_all();
        assert!(!state.allows("s1", &fp("bash", "a")));
        assert!(!state.allows("s2", &fp("bash", "b")));
    }
}
