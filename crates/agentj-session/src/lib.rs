pub mod file_store;
pub use file_store::FileBackedStore;

use agentj_core::error::{ModuleError, SessionError};
use agentj_core::hooks::EventBus;
use agentj_core::module::{EventType, SessionStatus, SessionStore};
use agentj_core::sync::lock_or_recover;
use agentj_core::types::{ChatMessage, SessionId, SessionSnapshot, now_epoch_secs};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct InMemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<ChatMessage>>>,
    statuses: Mutex<HashMap<SessionId, SessionStatus>>, // v0.1 lifecycle contract
    // v0.3 T9: unix-seconds ledger of last activity per session (create/append stamp it)
    last_activity: Mutex<HashMap<SessionId, u64>>,
    // v0.3 T9: injectable clock for deterministic expiry tests (SystemTime by default)
    clock: fn() -> u64,
    // T6/design decision #3: session-level path base
    roots: Mutex<HashMap<SessionId, PathBuf>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::with_clock(now_epoch_secs)
    }

    /// Test-only clock injection: drives the last_activity timestamps recorded by
    /// create()/append() so scan_expired can be asserted against an explicit now_epoch.
    pub fn with_clock(clock: fn() -> u64) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            statuses: Mutex::new(HashMap::new()),
            last_activity: Mutex::new(HashMap::new()),
            clock,
            roots: Mutex::new(HashMap::new()),
        }
    }

    fn now(&self) -> u64 {
        (self.clock)()
    }

    fn require_active(&self, session: &SessionId) -> Result<(), ModuleError> {
        let statuses = lock_or_recover(&self.statuses);
        match statuses.get(session) {
            Some(SessionStatus::Active) | None => Ok(()),
            _ => Err(ModuleError::Session(SessionError::InactiveSession(
                session.clone(),
            ))),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for InMemoryStore {
    fn create(&self) -> Result<SessionId, ModuleError> {
        let id = uuid::Uuid::new_v4().to_string();
        lock_or_recover(&self.sessions).insert(id.clone(), vec![]);
        lock_or_recover(&self.statuses).insert(id.clone(), SessionStatus::Active);
        agentj_core::sync::lock_or_recover(&self.last_activity).insert(id.clone(), self.now());
        Ok(id)
    }

    fn append(&self, session: &SessionId, message: ChatMessage) -> Result<(), ModuleError> {
        self.require_active(session)?; // inactive → SessionError::InactiveSession
        let mut map = lock_or_recover(&self.sessions);
        map.entry(session.clone()).or_default().push(message);
        agentj_core::sync::lock_or_recover(&self.last_activity).insert(session.clone(), self.now());
        Ok(())
    }

    fn get(&self, session: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
        self.require_active(session)?;
        Ok(lock_or_recover(&self.sessions)
            .get(session)
            .cloned()
            .unwrap_or_default())
    }

    fn estimate_tokens(&self, session: &SessionId) -> Result<usize, ModuleError> {
        let messages = self.get(session)?;
        // v0.1: approximate token counting — ~4 chars per token for English text
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        Ok(total_chars / 4)
    }

    fn project_root(&self, session: &SessionId) -> Option<PathBuf> {
        agentj_core::sync::lock_or_recover(&self.roots)
            .get(session)
            .cloned()
    }

    fn set_project_root(&self, session: &SessionId, root: PathBuf) -> Result<(), ModuleError> {
        agentj_core::sync::lock_or_recover(&self.roots).insert(session.clone(), root);
        Ok(())
    }

    // ---- v0.1 lifecycle contract (design §Session 生命周期契约) ----

    fn status(&self, session: &SessionId) -> Result<SessionStatus, ModuleError> {
        let statuses = lock_or_recover(&self.statuses);
        statuses
            .get(session)
            .copied()
            .ok_or(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )))
    }

    fn archive(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = lock_or_recover(&self.statuses);
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        statuses.insert(session.clone(), SessionStatus::Archived);
        Ok(())
    }

    fn delete(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = lock_or_recover(&self.statuses);
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        statuses.insert(session.clone(), SessionStatus::Deleted);
        lock_or_recover(&self.sessions).remove(session); // hard delete: data gone
        Ok(())
    }

    // ---- v0.1 recovery contract (design §会话持久化与恢复, decision #16) ----

    fn snapshot(&self, session: &SessionId) -> Result<SessionSnapshot, ModuleError> {
        let statuses = lock_or_recover(&self.statuses);
        let status = statuses
            .get(session)
            .copied()
            .ok_or_else(|| ModuleError::Session(SessionError::NotFound(session.clone())))?;
        if status == SessionStatus::Deleted {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        let messages = lock_or_recover(&self.sessions)
            .get(session)
            .cloned()
            .unwrap_or_default();
        let now = now_epoch_secs();
        Ok(SessionSnapshot {
            session_id: session.clone(),
            status,
            messages,
            created_at: now,
            updated_at: now,
            project_root: None,
        })
    }

    fn restore(&self, snapshot: &SessionSnapshot) -> Result<(), ModuleError> {
        let mut statuses = lock_or_recover(&self.statuses);
        let mut sessions = lock_or_recover(&self.sessions);
        statuses.insert(snapshot.session_id.clone(), snapshot.status);
        sessions.insert(snapshot.session_id.clone(), snapshot.messages.clone());
        Ok(())
    }
    // load_all: keep the trait default (empty) — InMemory carries no persistence

    fn scan_expired(
        &self,
        ttl: Duration,
        now_epoch: u64,
        bus: &EventBus,
    ) -> Result<usize, ModuleError> {
        let ledger: Vec<(SessionId, u64)> = agentj_core::sync::lock_or_recover(&self.last_activity)
            .iter()
            .map(|(sid, last)| (sid.clone(), *last))
            .collect();
        let mut expired = Vec::new();
        for (sid, last) in ledger {
            // saturating: a now_epoch predating last_activity is never treated as expired
            if now_epoch.saturating_sub(last) > ttl.as_secs() && self.is_active(&sid) {
                expired.push(sid);
            }
        }
        for sid in &expired {
            self.archive(sid)?;
            bus.publish(&EventType::SessionExpired(sid.clone()));
        }
        Ok(expired.len())
    }

    fn spawn_expiry_loop(
        self: Arc<Self>,
        bus: Arc<EventBus>,
        ttl: Duration,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let _ = self.scan_expired(ttl, now_epoch_secs(), &bus);
            }
        })
    }
}

impl InMemoryStore {
    fn is_active(&self, session: &SessionId) -> bool {
        matches!(
            agentj_core::sync::lock_or_recover(&self.statuses).get(session),
            Some(SessionStatus::Active)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::{ModuleError, SessionError};
    use agentj_core::hooks::EventBus;
    use agentj_core::module::{
        ConfigProvider, EventHandler, EventType, KernelContext, ProviderRegistry, SessionStatus,
        SessionStore, ToolRegistry,
    };
    use agentj_core::types::{ChatMessage, Role};
    use std::sync::Arc;

    #[test]
    fn create_and_append_session() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        let msg = ChatMessage::user("hello");
        store.append(&sid, msg.clone())?;
        let history = store.get(&sid)?;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, Role::User);
        Ok(())
    }

    #[test]
    fn unknown_session_returns_empty() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let history = store.get(&"nonexistent".to_string())?;
        assert!(history.is_empty());
        Ok(())
    }

    #[test]
    fn estimate_tokens_counts_approximate() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(
            &sid,
            ChatMessage::user(
                "hello world, this is a test message with enough chars to count tokens roughly",
            ),
        )?;
        let tokens = store.estimate_tokens(&sid)?;
        assert!(tokens > 5, "should have more than 5 tokens");
        assert!(tokens < 50, "should have fewer than 50 tokens");
        Ok(())
    }

    #[test]
    fn status_archive_delete_lifecycle() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        assert_eq!(store.status(&sid)?, SessionStatus::Active);

        store.archive(&sid)?;
        assert_eq!(store.status(&sid)?, SessionStatus::Archived);
        // archived → inactive: append and get are rejected
        assert!(matches!(
            store.append(&sid, ChatMessage::user("x")),
            Err(ModuleError::Session(SessionError::InactiveSession(_)))
        ));
        assert!(matches!(
            store.get(&sid),
            Err(ModuleError::Session(SessionError::InactiveSession(_)))
        ));

        store.delete(&sid)?;
        assert_eq!(store.status(&sid)?, SessionStatus::Deleted);
        Ok(())
    }

    #[test]
    fn delete_missing_session_returns_not_found() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        assert!(matches!(
            store.delete(&"nonexistent".into()),
            Err(ModuleError::Session(SessionError::NotFound(_)))
        ));
        Ok(())
    }

    #[test]
    fn inmemory_set_project_root_roundtrip() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        assert_eq!(store.project_root(&sid), None, "unset by default");
        store.set_project_root(&sid, std::path::PathBuf::from("/tmp/p"))?;
        assert_eq!(
            store.project_root(&sid),
            Some(std::path::PathBuf::from("/tmp/p"))
        );
        Ok(())
    }

    // ---- v0.1 recovery contract (design §会话持久化与恢复, decision #16) ----

    #[test]
    fn snapshot_roundtrip_active_session() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        store.append(&sid, ChatMessage::user("world"))?;
        let snap = store.snapshot(&sid)?;
        assert_eq!(snap.session_id, sid);
        assert_eq!(snap.status, SessionStatus::Active);
        assert_eq!(snap.messages.len(), 2);

        let fresh = InMemoryStore::new();
        fresh.restore(&snap)?;
        assert_eq!(fresh.status(&sid)?, SessionStatus::Active);
        assert_eq!(fresh.get(&sid)?.len(), 2);
        Ok(())
    }

    #[test]
    fn snapshot_archived_session_preserves_status() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        store.archive(&sid)?;
        let snap = store.snapshot(&sid)?;
        assert_eq!(snap.status, SessionStatus::Archived);
        assert_eq!(snap.messages.len(), 1); // archived keeps data
        Ok(())
    }

    #[test]
    fn snapshot_deleted_or_missing_returns_not_found() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        assert!(matches!(
            store.snapshot(&"missing".into()),
            Err(ModuleError::Session(SessionError::NotFound(_)))
        ));

        let sid = store.create()?;
        store.delete(&sid)?;
        assert!(matches!(
            store.snapshot(&sid),
            Err(ModuleError::Session(SessionError::NotFound(_)))
        ));
        Ok(())
    }

    #[test]
    fn restore_overwrites_existing_session() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("old"))?;
        let snap = store.snapshot(&sid)?;

        store.append(&sid, ChatMessage::user("new"))?; // drift after snapshot
        store.restore(&snap)?; // roll back to snapshot state
        let msgs = store.get(&sid)?;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "old");
        Ok(())
    }

    #[test]
    fn load_all_default_is_empty_for_inmemory() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        // InMemory carries no persistence — load_all reports zero persisted sessions
        assert!(store.load_all()?.is_empty());
        Ok(())
    }

    #[test]
    fn concurrent_mixed_ops_do_not_deadlock() {
        use std::sync::{Arc, Barrier, mpsc};
        use std::thread;
        use std::time::{Duration, Instant};

        let store = Arc::new(InMemoryStore::new());
        let sid = store.create().unwrap();
        const THREADS: usize = 8;
        const ITERS: usize = 20_000;

        let barrier = Arc::new(Barrier::new(THREADS));
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::with_capacity(THREADS);
        for t in 0..THREADS {
            let store = Arc::clone(&store);
            let sid = sid.clone();
            let barrier = Arc::clone(&barrier);
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                for i in 0..ITERS {
                    match t % 4 {
                        0 => drop(store.append(&sid, ChatMessage::user(format!("t{t}-{i}")))),
                        1 => drop(store.snapshot(&sid)),
                        2 => drop(store.delete(&sid)),
                        _ => drop(store.archive(&sid)),
                    }
                }
                tx.send(()).unwrap();
            }));
        }
        drop(tx);

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut done = 0;
        while done < THREADS {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(()) => done += 1,
                Err(_) => panic!("deadlock: only {done}/{THREADS} workers finished within 30s"),
            }
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    // ---- v0.3 T9 expiry scan (design 遗留问题「Session 过期回收」) ----

    struct ExpirySpy {
        events: Arc<Mutex<Vec<EventType>>>,
    }

    impl EventHandler for ExpirySpy {
        fn on_event(&self, event: &EventType, _ctx: &KernelContext) {
            lock_or_recover(&self.events).push(event.clone());
        }
    }

    struct NoopCfg;
    impl ConfigProvider for NoopCfg {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    #[test]
    fn scan_expired_archives_and_publishes() -> Result<(), ModuleError> {
        // injectable clock: everything is created at t=90s (deterministic — last_activity
        // must be comparable against the explicit now_epoch, NOT the real wall clock)
        let store = Arc::new(InMemoryStore::with_clock(|| 90));
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;

        // ctx-carrying bus: the store publishes through it; the ctx reuses the store itself
        let ctx = KernelContext {
            session: store.clone(),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(NoopCfg),
        };
        let bus = EventBus::with_ctx(Arc::new(ctx));
        let spy = Arc::new(ExpirySpy {
            events: Arc::new(Mutex::new(vec![])),
        });
        // EventBus matches by exact equality (T0) — subscribe to the ACTUAL session id
        bus.register(EventType::SessionExpired(sid.clone()), spy.clone());

        // now = 100, ttl = 5s → last_activity (90) is stale → expired
        let count = store.scan_expired(std::time::Duration::from_secs(5), 100, &bus)?;
        assert_eq!(count, 1);
        assert_eq!(store.status(&sid)?, SessionStatus::Archived);
        assert!(matches!(
            lock_or_recover(&spy.events)[0],
            EventType::SessionExpired(ref s) if *s == sid
        ));
        Ok(())
    }

    #[test]
    fn scan_keeps_fresh_sessions_active() -> Result<(), ModuleError> {
        let store = Arc::new(InMemoryStore::new());
        let sid = store.create()?;
        let count = store.scan_expired(std::time::Duration::from_secs(60), 0, &EventBus::new())?;
        assert_eq!(count, 0);
        assert_eq!(store.status(&sid)?, SessionStatus::Active);
        Ok(())
    }

    #[test]
    fn expiry_reachable_through_trait_object() -> Result<(), ModuleError> {
        let store: Arc<dyn SessionStore> = Arc::new(InMemoryStore::with_clock(|| 90));
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        let n = store.scan_expired(Duration::from_secs(5), 100, &EventBus::new())?;
        assert_eq!(n, 1);
        assert_eq!(store.status(&sid)?, SessionStatus::Archived);
        Ok(())
    }
}
