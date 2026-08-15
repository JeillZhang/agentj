use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use async_trait::async_trait;
use futures::stream::BoxStream;
use parking_lot::RwLock;
use crate::error::{ModuleError, ModelError, ToolError};
use crate::types::*;

// ---- Traits ----

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError>;

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError>;
}

/// Assembles the request context and folds tool results back into history.
/// Defined in core so `AgentLoop` stays trait-only; implemented by domain crates.
pub trait ContextPipeline: Send + Sync {
    fn assemble(
        &self,
        system: &str,
        history: &[ChatMessage],
        tool_results: &[(ToolCall, String)],
    ) -> Vec<ChatMessage>;
    fn reflux(&self, history: &mut Vec<ChatMessage>, tool_call: &ToolCall, result: String);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventType {
    PreRequest,
    PostResponse,
    Shutdown,
    ToolStart(String),
    ToolEnd(String),
    // 生命周期事件（v0.1 契约先行，v0.2 Hook System 消费）
    ModuleLoaded(ModuleId),
    ModuleFailed(ModuleId),
    ModuleStopped(ModuleId),
    // Session 生命周期事件（v0.1 契约先行，v0.2 消费；见设计 §Session 生命周期契约）
    SessionArchived(SessionId),
    SessionDeleted(SessionId),
    SessionExpired(SessionId),
}

pub trait EventHandler: Send + Sync {
    fn on_event(&self, event: &EventType, ctx: &KernelContext);
}

#[derive(Debug, Clone)]
pub struct ModuleMetadata {
    pub id: ModuleId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
}

/// Lifecycle state of a registered module. Tracked by the Kernel registry (NOT by
/// the module itself) — state belongs to the orchestrator (see design §模块生命周期状态机).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Unloaded,
    Initializing,
    Ready,
    Stopping,
    Stopped,
    Failed,
}

// ---- Module & KernelModule ----

pub trait Module: Send + Sync {
    fn id(&self) -> ModuleId;
    fn metadata(&self) -> ModuleMetadata;
    fn init(&mut self, ctx: &KernelContext, register: &mut ModuleRegister) -> Result<(), ModuleError>;
    fn shutdown(&mut self) -> Result<(), ModuleError>;
}

pub trait KernelModule: Module {
    fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError>;
}

// ---- Registries ----

pub struct ModuleRegister {
    pub tools: Arc<ToolRegistry>,
    pub events: Vec<(EventType, Box<dyn EventHandler>)>,
    pub providers: Arc<ProviderRegistry>,
}

impl ModuleRegister {
    pub fn new(tools: Arc<ToolRegistry>, providers: Arc<ProviderRegistry>) -> Self {
        Self { tools, events: vec![], providers }
    }

    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) -> Result<(), ModuleError> {
        self.tools.register(tool);
        Ok(())
    }

    pub fn on_event(&mut self, event: EventType, handler: Box<dyn EventHandler>) -> Result<(), ModuleError> {
        self.events.push((event, handler));
        Ok(())
    }

    pub fn register_provider(&mut self, provider: Arc<dyn ModelProvider>) -> Result<(), ModuleError> {
        self.providers.register("default".into(), provider);
        Ok(())
    }
}

/// Ownership of a registered tool. v0.1 contract-first (design §10): Builtin for
/// built-ins, Mcp for MCP-sourced tools, Plugin for plugin-sourced tools.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolSource {
    Builtin,
    Mcp { server_id: String },
    Plugin { plugin_id: String },
}

/// A tool entry with its ownership source.
#[derive(Clone)]
pub struct ToolEntry {
    pub tool: Arc<dyn Tool>,
    pub source: ToolSource,
}

impl fmt::Debug for ToolEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolEntry")
            .field("tool", &self.tool.name())
            .field("source", &self.source)
            .finish()
    }
}

/// Conflict policy when registering a tool whose name is already taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacePolicy { Reject, Replace }

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    sources: RwLock<HashMap<String, ToolSource>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            sources: RwLock::new(HashMap::new()),
        }
    }

    /// Legacy register — kept for compatibility, delegates to Builtin + Reject.
    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.register_tool_with_source(tool, ToolSource::Builtin, ReplacePolicy::Reject).ok();
    }

    /// Register a tool with its ownership source and a conflict policy.
    pub fn register_tool_with_source(
        &self,
        tool: Arc<dyn Tool>,
        source: ToolSource,
        policy: ReplacePolicy,
    ) -> Result<(), ModuleError> {
        let name = tool.name().to_string();
        let mut tools = self.tools.write();
        if tools.contains_key(&name) && policy == ReplacePolicy::Reject {
            return Err(ModuleError::Handler(format!("tool '{name}' already registered")));
        }
        tools.insert(name.clone(), tool);
        self.sources.write().insert(name, source);
        Ok(())
    }

    /// Remove a tool by name, returning the removed entry (None if absent).
    pub fn unregister_tool(&self, name: &str) -> Option<ToolEntry> {
        let tool = self.tools.write().remove(name)?;
        let source = self.sources.write().remove(name).unwrap_or(ToolSource::Builtin);
        Some(ToolEntry { tool, source })
    }

    /// Remove ALL tools owned by a source; returns the number removed.
    /// This is the ghost-tool cleanup primitive (design §MCP 连接生命周期).
    pub fn unregister_by_source(&self, source: &ToolSource) -> usize {
        let names: Vec<String> = {
            let sources = self.sources.read();
            sources
                .iter()
                .filter(|(_, s)| **s == *source)
                .map(|(name, _)| (*name).clone())
                .collect()
        };
        let mut tools = self.tools.write();
        let mut sources = self.sources.write();
        for name in &names {
            tools.remove(name);
            sources.remove(name);
        }
        names.len()
    }

    /// Return all entries with their ownership source.
    pub fn tools(&self) -> Vec<ToolEntry> {
        let tools = self.tools.read();
        let sources = self.sources.read();
        tools
            .iter()
            .map(|(name, tool)| ToolEntry {
                tool: tool.clone(),
                source: sources.get(name).cloned().unwrap_or(ToolSource::Builtin),
            })
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.read().values().map(|t| ToolDefinition {
            name: t.name().into(),
            description: t.description().into(),
            input_schema: t.input_schema(),
        }).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self { Self::new() }
}

pub struct ProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn ModelProvider>>>,
    default: RwLock<Option<String>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self { providers: RwLock::new(HashMap::new()), default: RwLock::new(None) }
    }

    pub fn register(&self, name: String, provider: Arc<dyn ModelProvider>) {
        self.providers.write().insert(name.clone(), provider);
        if self.default.read().is_none() {
            *self.default.write() = Some(name);
        }
    }

    pub fn get_default(&self) -> Option<Arc<dyn ModelProvider>> {
        self.default.read().as_ref()
            .and_then(|name| self.providers.read().get(name).cloned())
    }

    pub fn list(&self) -> Vec<String> {
        self.providers.read().keys().cloned().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self { Self::new() }
}

// ---- Session & Config ----

/// Session lifecycle status. Contract-first (v0.1 signature, v0.2 semantics) —
/// see design §Session 生命周期契约 (decision #14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus { Active, Archived, Deleted }

pub trait SessionStore: Send + Sync {
    fn create(&self) -> Result<SessionId, ModuleError>;
    fn append(&self, session: &SessionId, message: ChatMessage) -> Result<(), ModuleError>;
    fn get(&self, session: &SessionId) -> Result<Vec<ChatMessage>, ModuleError>;
    fn estimate_tokens(&self, session: &SessionId) -> Result<usize, ModuleError>;

    // v0.1 contract-first: default impls keep existing stores compiling;
    // real archive/delete semantics land in T17 (agentj-session).
    fn status(&self, _session: &SessionId) -> Result<SessionStatus, ModuleError> {
        Ok(SessionStatus::Active)
    }
    fn archive(&self, _session: &SessionId) -> Result<(), ModuleError> {
        Err(ModuleError::Handler("archive not supported".into()))
    }
    fn delete(&self, _session: &SessionId) -> Result<(), ModuleError> {
        Err(ModuleError::Handler("delete not supported".into()))
    }
}

pub trait ConfigProvider: Send + Sync {
    fn get_str(&self, key: &str) -> Option<String>;
    fn get_int(&self, key: &str) -> Option<i64>;
    fn providers(&self) -> Vec<(String, String)>; // Vec<(name, base_url)>
}

#[derive(Clone)]
pub struct KernelContext {
    pub session: Arc<dyn SessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub providers: Arc<ProviderRegistry>,
    pub config: Arc<dyn ConfigProvider>,
}

// 预留访问器方法（v0.2+ Kernel 服务定位通道的契约，纯加性，零破坏）：
// 未来方法内部可从"硬编码字段"换成 ModuleResolver::resolve()，模块代码零改动。
impl KernelContext {
    pub fn session(&self) -> Arc<dyn SessionStore> { self.session.clone() }
    pub fn tools(&self) -> Arc<ToolRegistry> { self.tools.clone() }
    pub fn providers(&self) -> Arc<ProviderRegistry> { self.providers.clone() }
    pub fn config(&self) -> Arc<dyn ConfigProvider> { self.config.clone() }
}

// ---- Kernel Message ----

#[derive(Debug, Clone)]
pub enum KernelMessage {
    UserRequest { session_id: SessionId, content: String, attachments: Vec<Attachment> },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum KernelResponse {
    Handled,
    Streaming(Vec<String>),
}

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

    struct OtherTool;
    impl Tool for OtherTool {
        fn name(&self) -> &str { "other" }
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
    fn register_with_source_and_conflict_policy() {
        let registry = ToolRegistry::new();
        registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Builtin, ReplacePolicy::Reject).unwrap();
        // same name, same source, Reject → error
        assert!(registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Builtin, ReplacePolicy::Reject).is_err());
        // Replace → overwrites, still 1 entry
        registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Builtin, ReplacePolicy::Replace).unwrap();
        assert_eq!(registry.definitions().len(), 1);
    }

    #[test]
    fn unregister_tool_removes_by_name() {
        let registry = ToolRegistry::new();
        registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Builtin, ReplacePolicy::Reject).unwrap();
        let removed = registry.unregister_tool("dummy");
        assert!(removed.is_some());
        assert!(registry.get("dummy").is_none());
        assert!(registry.unregister_tool("dummy").is_none()); // idempotent
    }

    #[test]
    fn unregister_by_source_removes_all_and_counts() {
        let registry = ToolRegistry::new();
        registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Mcp { server_id: "srv".into() }, ReplacePolicy::Reject).unwrap();
        registry.register_tool_with_source(Arc::new(OtherTool), ToolSource::Plugin { plugin_id: "p1".into() }, ReplacePolicy::Reject).unwrap();
        let removed = registry.unregister_by_source(&ToolSource::Mcp { server_id: "srv".into() });
        assert_eq!(removed, 1);
        assert_eq!(registry.definitions().len(), 1);
        assert_eq!(registry.get("other").unwrap().name(), "other");
        // Legacy register() still lands as Builtin once "dummy" is free again.
        registry.unregister_tool("other");
        registry.register(Arc::new(DummyTool));
        assert_eq!(registry.tools()[0].source, ToolSource::Builtin);
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
