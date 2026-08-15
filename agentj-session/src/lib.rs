use std::collections::HashMap;
use std::sync::Mutex;
use agentj_core::error::{ModuleError, SessionError};
use agentj_core::module::{SessionStatus, SessionStore};
use agentj_core::types::{ChatMessage, SessionId};

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
            _ => Err(ModuleError::Session(SessionError::InactiveSession(session.clone()))),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self { Self::new() }
}

impl SessionStore for InMemoryStore {
    fn create(&self) -> Result<SessionId, ModuleError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.lock().unwrap().insert(id.clone(), vec![]);
        self.statuses.lock().unwrap().insert(id.clone(), SessionStatus::Active);
        Ok(id)
    }

    fn append(&self, session: &SessionId, message: ChatMessage) -> Result<(), ModuleError> {
        let mut map = self.sessions.lock().unwrap();
        self.require_active(session)?; // inactive → SessionError::InactiveSession
        map.entry(session.clone()).or_default().push(message);
        Ok(())
    }

    fn get(&self, session: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
        self.require_active(session)?;
        Ok(self.sessions.lock().unwrap().get(session).cloned().unwrap_or_default())
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
        statuses.get(session).copied().ok_or(ModuleError::Session(SessionError::NotFound(session.clone())))
    }

    fn archive(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = self.statuses.lock().unwrap();
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(session.clone())));
        }
        statuses.insert(session.clone(), SessionStatus::Archived);
        Ok(())
    }

    fn delete(&self, session: &SessionId) -> Result<(), ModuleError> {
        let mut statuses = self.statuses.lock().unwrap();
        if !statuses.contains_key(session) {
            return Err(ModuleError::Session(SessionError::NotFound(session.clone())));
        }
        statuses.insert(session.clone(), SessionStatus::Deleted);
        self.sessions.lock().unwrap().remove(session); // hard delete: data gone
        Ok(())
    }
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
        store.append(&sid, ChatMessage::user("hello world, this is a test message with enough chars to count tokens roughly"))?;
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
        assert!(matches!(store.get(&sid), Err(ModuleError::Session(SessionError::InactiveSession(_)))));

        store.delete(&sid)?;
        assert_eq!(store.status(&sid)?, SessionStatus::Deleted);
        Ok(())
    }

    #[test]
    fn delete_missing_session_returns_not_found() -> Result<(), ModuleError> {
        let store = InMemoryStore::new();
        assert!(matches!(store.delete(&"nonexistent".into()), Err(ModuleError::Session(SessionError::NotFound(_)))));
        Ok(())
    }
}
