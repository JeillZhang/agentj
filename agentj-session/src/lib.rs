#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ModuleError;
    use agentj_core::module::SessionStore;
    use agentj_core::types::{ChatMessage, Role, SessionId};

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
}
