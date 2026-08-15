use agentj_core::error::{ModuleError, SessionError};
use agentj_core::module::{SessionStatus, SessionStore};
use agentj_core::types::{ChatMessage, SessionId, SessionSnapshot, now_epoch_secs};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct InMemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<ChatMessage>>>,
    statuses: Mutex<HashMap<SessionId, SessionStatus>>, // v0.1 lifecycle contract
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            statuses: Mutex::new(HashMap::new()),
        }
    }

    fn require_active(&self, session: &SessionId) -> Result<(), ModuleError> {
        let statuses = self.statuses.lock().unwrap();
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
        self.sessions.lock().unwrap().insert(id.clone(), vec![]);
        self.statuses
            .lock()
            .unwrap()
            .insert(id.clone(), SessionStatus::Active);
        Ok(id)
    }

    fn append(&self, session: &SessionId, message: ChatMessage) -> Result<(), ModuleError> {
        self.require_active(session)?; // inactive → SessionError::InactiveSession
        let mut map = self.sessions.lock().unwrap();
        map.entry(session.clone()).or_default().push(message);
        Ok(())
    }

    fn get(&self, session: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
        self.require_active(session)?;
        Ok(self
            .sessions
            .lock()
            .unwrap()
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

    // ---- v0.1 lifecycle contract (design §Session 生命周期契约) ----

    fn status(&self, session: &SessionId) -> Result<SessionStatus, ModuleError> {
        let statuses = self.statuses.lock().unwrap();
        statuses
            .get(session)
            .copied()
            .ok_or(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )))
    }

    fn archive(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = self.statuses.lock().unwrap();
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        statuses.insert(session.clone(), SessionStatus::Archived);
        Ok(())
    }

    fn delete(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = self.statuses.lock().unwrap();
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        statuses.insert(session.clone(), SessionStatus::Deleted);
        self.sessions.lock().unwrap().remove(session); // hard delete: data gone
        Ok(())
    }

    // ---- v0.1 recovery contract (design §会话持久化与恢复, decision #16) ----

    fn snapshot(&self, session: &SessionId) -> Result<SessionSnapshot, ModuleError> {
        let statuses = self.statuses.lock().unwrap();
        let status = statuses
            .get(session)
            .copied()
            .ok_or_else(|| ModuleError::Session(SessionError::NotFound(session.clone())))?;
        if status == SessionStatus::Deleted {
            return Err(ModuleError::Session(SessionError::NotFound(
                session.clone(),
            )));
        }
        let messages = self
            .sessions
            .lock()
            .unwrap()
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
        })
    }

    fn restore(&self, snapshot: &SessionSnapshot) -> Result<(), ModuleError> {
        let mut statuses = self.statuses.lock().unwrap();
        let mut sessions = self.sessions.lock().unwrap();
        statuses.insert(snapshot.session_id.clone(), snapshot.status);
        sessions.insert(snapshot.session_id.clone(), snapshot.messages.clone());
        Ok(())
    }
    // load_all: keep the trait default (empty) — InMemory carries no persistence
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::{ModuleError, SessionError};
    use agentj_core::module::{SessionStatus, SessionStore};
    use agentj_core::types::{ChatMessage, Role};

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
}
