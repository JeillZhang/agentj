#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ModuleError, ToolError};

    struct TestModule;
    impl Module for TestModule {
        fn id(&self) -> ModuleId { "test".into() }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata { id: "test".into(), name: "Test".into(), version: "0.1".into(), description: "".into(), author: "".into() }
        }
        fn init(&mut self, _ctx: &KernelContext, _register: &mut ModuleRegister) -> Result<(), ModuleError> { Ok(()) }
        fn shutdown(&mut self) -> Result<(), ModuleError> { Ok(()) }
    }

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &str { "dummy" }
        fn description(&self) -> &str { "" }
        fn input_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::Value::String("ok".into()))
        }
    }

    #[test]
    fn module_id() {
        let m = TestModule;
        assert_eq!(m.id(), "test");
    }

    #[test]
    fn tool_registry_registers_and_gets() {
        let registry = ToolRegistry::new();
        assert!(registry.is_empty());
        registry.register(Arc::new(DummyTool));
        assert_eq!(registry.get("dummy").unwrap().name(), "dummy");
        assert_eq!(registry.definitions().len(), 1);
    }

    #[test]
    fn provider_registry_has_no_default_before_register() {
        let registry = ProviderRegistry::new();
        assert!(registry.get_default().is_none());
    }

    #[test]
    fn event_type_lifecycle_variants_exist() {
        let loaded = EventType::ModuleLoaded("cfg".into());
        let failed = EventType::ModuleFailed("cfg".into());
        let stopped = EventType::ModuleStopped("cfg".into());
        assert_ne!(loaded, failed);
        assert_ne!(failed, stopped);
        assert_eq!(loaded, EventType::ModuleLoaded("cfg".into()));
    }

    #[test]
    fn event_type_session_variants_exist() {
        let archived = EventType::SessionArchived("s1".into());
        let deleted = EventType::SessionDeleted("s1".into());
        let expired = EventType::SessionExpired("s1".into());
        assert_ne!(archived, deleted);
        assert_ne!(deleted, expired);
        assert_eq!(archived, EventType::SessionArchived("s1".into()));
    }

    // Contract-only store: relies on SessionStore's default impls for
    // status/archive/delete — proves existing stores keep compiling.
    struct ContractOnlyStore;
    impl SessionStore for ContractOnlyStore {
        fn create(&self) -> Result<SessionId, ModuleError> { Ok("s".into()) }
        fn append(&self, _s: &SessionId, _m: ChatMessage) -> Result<(), ModuleError> { Ok(()) }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> { Ok(vec![]) }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> { Ok(0) }
    }

    #[test]
    fn session_store_contract_defaults_apply() {
        let store = ContractOnlyStore;
        assert_eq!(store.status(&"s1".into()).unwrap(), SessionStatus::Active);
        assert!(store.archive(&"s1".into()).is_err());
        assert!(store.delete(&"s1".into()).is_err());
    }
}
