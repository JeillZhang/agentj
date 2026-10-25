use crate::artifacts::TurnArtifact;
use crate::error::{ModelError, ModuleError, ToolError};
use crate::resolver::ServiceRegistry;
use crate::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

// ---- Traits ----

/// Per-call tool execution context (design 2026-09-13 决策 #3): the session
/// that triggered the call and the workspace root relative paths resolve
/// against.
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    pub session_id: SessionId,
    pub workspace_root: PathBuf,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError>;

    /// Execute with per-call context. Default delegates to `execute` so existing
    /// tools (including the MCP bridge) compile unchanged. @since 0.13.0
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        _ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        self.execute(input)
    }
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

    /// Model ids this provider serves. Default empty keeps legacy providers
    /// unchanged and lets `kernel/status` report real lists. @since 0.25.0
    fn models(&self) -> Vec<String> {
        Vec::new()
    }
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
    /// Trim an assembled request so it fits `budget_tokens` (design §上下文预算). The
    /// default implementation is used by every pipeline, so a request is trimmed to the
    /// budget whenever the message layout allows it (the system prompt and the newest
    /// message are always kept, so an unusually large system prompt can still exceed the
    /// budget). Override only when the pipeline knows its layout better. @since 0.29.7
    fn fit(&self, messages: Vec<ChatMessage>, budget_tokens: usize) -> Vec<ChatMessage> {
        crate::budget::fit_messages(messages, budget_tokens)
    }
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
    // v0.3 扩展（design §7 #4）：插件生命周期 + MCP 连接生命周期事件
    ModuleLoading(ModuleId),
    ModuleDisabled(ModuleId),
    ModuleEnabled(ModuleId),
    ModuleReloaded(ModuleId),
    McpConnecting(String),
    McpConnected(String),
    McpDisconnected(String),
    McpReconnecting(String),
    McpClosed(String),
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
    fn init(
        &mut self,
        ctx: &KernelContext,
        register: &mut ModuleRegister,
    ) -> Result<(), ModuleError>;
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
    /// v0.3 service location (design §7 #1): modules register capability services here.
    pub services: Arc<ServiceRegistry>,
}

impl ModuleRegister {
    pub fn new(tools: Arc<ToolRegistry>, providers: Arc<ProviderRegistry>) -> Self {
        Self::with_services(tools, providers, Arc::new(ServiceRegistry::new()))
    }

    /// Like `new`, but shares the caller's service registry (e.g. the Kernel's own).
    pub fn with_services(
        tools: Arc<ToolRegistry>,
        providers: Arc<ProviderRegistry>,
        services: Arc<ServiceRegistry>,
    ) -> Self {
        Self {
            tools,
            events: vec![],
            providers,
            services,
        }
    }

    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) -> Result<(), ModuleError> {
        self.tools.register(tool);
        Ok(())
    }

    pub fn on_event(
        &mut self,
        event: EventType,
        handler: Box<dyn EventHandler>,
    ) -> Result<(), ModuleError> {
        self.events.push((event, handler));
        Ok(())
    }

    pub fn register_provider(
        &mut self,
        provider: Arc<dyn ModelProvider>,
    ) -> Result<(), ModuleError> {
        self.providers.register("default".into(), provider);
        Ok(())
    }

    /// Register a concrete-typed capability service (resolved via `Kernel::resolve`).
    pub fn register_service<T: 'static + Send + Sync>(
        &self,
        id: ModuleId,
        service: Arc<T>,
    ) -> Result<(), ModuleError> {
        self.services.register(id, service);
        Ok(())
    }

    /// Register a routable service (invoked via `KernelMessage::ServiceCall`).
    pub fn register_kernel_service(
        &self,
        id: ModuleId,
        service: Arc<dyn crate::resolver::KernelService>,
    ) -> Result<(), ModuleError> {
        self.services.register_kernel_service(id, service);
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
pub enum ReplacePolicy {
    Reject,
    Replace,
}

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    sources: RwLock<HashMap<String, ToolSource>>,
    checker: RwLock<Option<Arc<dyn crate::permission::PermissionChecker>>>,
    // design P1 缺口闭环: sources whose tools must not reach the model tool list.
    // Empty in new() → definitions() output is byte-identical to v0.1.
    disabled: RwLock<HashSet<ToolSource>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            sources: RwLock::new(HashMap::new()),
            checker: RwLock::new(None),
            disabled: RwLock::new(HashSet::new()),
        }
    }

    /// Legacy register — kept for compatibility, delegates to Builtin + Reject.
    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.register_tool_with_source(tool, ToolSource::Builtin, ReplacePolicy::Reject)
            .ok();
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
            return Err(ModuleError::Handler(format!(
                "tool '{name}' already registered"
            )));
        }
        tools.insert(name.clone(), tool);
        self.sources.write().insert(name, source);
        Ok(())
    }

    /// Remove a tool by name, returning the removed entry (None if absent).
    pub fn unregister_tool(&self, name: &str) -> Option<ToolEntry> {
        let tool = self.tools.write().remove(name)?;
        let source = self
            .sources
            .write()
            .remove(name)
            .unwrap_or(ToolSource::Builtin);
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
        let disabled = self.disabled.read();
        self.tools
            .read()
            .iter()
            .filter(|(name, _)| {
                let source = self
                    .sources
                    .read()
                    .get(*name)
                    .cloned()
                    .unwrap_or(ToolSource::Builtin);
                !disabled.contains(&source)
            })
            .map(|(name, t)| ToolDefinition {
                name: name.clone(),
                description: t.description().into(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Enable/disable an entire source (design P1 primitive; T5's set_plugin_enabled
    /// consumes this). Disabled sources stay registered but leave `definitions()`.
    pub fn set_source_enabled(&self, source: &ToolSource, enabled: bool) {
        let mut disabled = self.disabled.write();
        if enabled {
            disabled.remove(source);
        } else {
            disabled.insert(source.clone());
        }
    }

    pub fn is_source_enabled(&self, source: &ToolSource) -> bool {
        !self.disabled.read().contains(source)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }

    /// Set the permission checker (called once at kernel startup by the CLI).
    pub fn set_permission_checker(&self, checker: Arc<dyn crate::permission::PermissionChecker>) {
        *self.checker.write() = Some(checker);
    }

    /// Read-only source lookup (v0.1 T16 maintains the sources map; expose it).
    pub fn source_of(&self, name: &str) -> Option<ToolSource> {
        self.sources.read().get(name).cloned()
    }

    /// The configured permission checker, if any (design §7 KernelContext injection
    /// is satisfied through the registry the context already owns).
    pub fn checker(&self) -> Option<Arc<dyn crate::permission::PermissionChecker>> {
        self.checker.read().clone()
    }

    /// Unified async execution entry (design #17 interception point): permission
    /// gate first, then spawn_blocking delegate to the synchronous Tool::execute.
    /// Without a checker, executes directly (v0.1 pre-permission behavior).
    pub async fn execute_async(
        &self,
        name: &str,
        args: serde_json::Value,
        perm_ctx: &crate::permission::PermissionContext,
    ) -> Result<serde_json::Value, ToolError> {
        let checker = self.checker.read().clone();
        if let Some(checker) = checker {
            match checker.check(perm_ctx) {
                crate::permission::PermissionDecision::Allow => {}
                crate::permission::PermissionDecision::Deny { reason } => {
                    return Err(ToolError::PermissionDenied {
                        name: name.into(),
                        reason,
                    });
                }
                crate::permission::PermissionDecision::Ask { request_id } => {
                    match checker.await_approval(&request_id).await {
                        crate::permission::PermissionDecision::Allow => {}
                        crate::permission::PermissionDecision::Deny { reason } => {
                            return Err(ToolError::PermissionDenied {
                                name: name.into(),
                                reason,
                            });
                        }
                        crate::permission::PermissionDecision::Ask { .. } => {
                            return Err(ToolError::PermissionDenied {
                                name: name.into(),
                                reason: "unexpected nested approval request".into(),
                            });
                        }
                    }
                }
            }
        }
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::ToolNotFound { name: name.into() })?;
        let ctx = ToolCallContext {
            session_id: perm_ctx.session_id.clone(),
            workspace_root: perm_ctx.workspace_root.clone(),
        };
        let args = crate::tool_paths::rebase_tool_args(name, args, &ctx.workspace_root);
        tokio::task::spawn_blocking(move || tool.execute_with_context(args, &ctx))
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                name: name.into(),
                reason: format!("tool panic: {e}"),
            })?
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn ModelProvider>>>,
    default: RwLock<Option<String>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            default: RwLock::new(None),
        }
    }

    pub fn register(&self, name: String, provider: Arc<dyn ModelProvider>) {
        self.providers.write().insert(name.clone(), provider);
        if self.default.read().is_none() {
            *self.default.write() = Some(name);
        }
    }

    pub fn get_default(&self) -> Option<Arc<dyn ModelProvider>> {
        self.get_default_with_name().map(|(_, provider)| provider)
    }

    /// Switch the default to an already-registered name. Returns `false` and leaves the
    /// default unchanged if the name is not registered.
    pub fn set_default(&self, name: &str) -> bool {
        if !self.providers.read().contains_key(name) {
            return false;
        }
        *self.default.write() = Some(name.to_string());
        true
    }

    /// Name of the current default provider, if any.
    pub fn default_name(&self) -> Option<String> {
        self.default.read().clone()
    }

    /// Resolves the default name and provider under a single `default` read, so a caller can
    /// never pair a name with a different provider under a concurrent `set_default`.
    pub fn get_default_with_name(&self) -> Option<(String, Arc<dyn ModelProvider>)> {
        let default = self.default.read();
        let name = default.as_ref()?;
        let provider = self.providers.read().get(name).cloned()?;
        Some((name.clone(), provider))
    }

    /// Look up a provider by its registered name (read-only). @since 0.26.0
    pub fn get(&self, name: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.read().get(name).cloned()
    }

    /// Remove a provider (additive, `@since 0.29.0`). `get_default` resolves through the map,
    /// so a dangling default name already yields None.
    pub fn remove(&self, name: &str) -> bool {
        self.providers.write().remove(name).is_some()
    }

    pub fn list(&self) -> Vec<String> {
        self.providers.read().keys().cloned().collect()
    }

    /// (name, models) pairs sorted by name, for the `kernel/status` filler.
    /// @since 0.25.0
    pub fn list_with_models(&self) -> Vec<(String, Vec<String>)> {
        let providers = self.providers.read();
        let mut out: Vec<(String, Vec<String>)> = providers
            .iter()
            .map(|(name, p)| (name.clone(), p.models()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Session & Config ----

/// Session lifecycle status. Contract-first (v0.1 signature, v0.2 semantics) —
/// see design §Session 生命周期契约 (decision #14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    Active,
    Archived,
    Deleted,
}

pub trait SessionStore: Send + Sync {
    fn create(&self) -> Result<SessionId, ModuleError>;
    fn append(&self, session: &SessionId, message: ChatMessage) -> Result<(), ModuleError>;
    fn get(&self, session: &SessionId) -> Result<Vec<ChatMessage>, ModuleError>;
    fn estimate_tokens(&self, session: &SessionId) -> Result<usize, ModuleError>;

    /// Session-level path base for tools; default returns None (unset). @since 0.13.0
    fn project_root(&self, _session: &SessionId) -> Option<PathBuf> {
        None
    }

    /// Store the session-level path base; default is a no-op. @since 0.13.0
    fn set_project_root(&self, _session: &SessionId, _root: PathBuf) -> Result<(), ModuleError> {
        Ok(())
    }

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

    /// Export the session's full state (messages + lifecycle status).
    /// Missing/Deleted session → SessionError::NotFound. Default impl assembles
    /// from get()+status() — stores with lifecycle semantics (InMemory) override
    /// so Archived sessions snapshot too (get() rejects inactive).
    fn snapshot(&self, session: &SessionId) -> Result<SessionSnapshot, ModuleError> {
        let messages = self.get(session)?;
        let status = self.status(session)?;
        let now = crate::types::now_epoch_secs();
        Ok(SessionSnapshot {
            session_id: session.clone(),
            status,
            messages,
            created_at: now,
            updated_at: now,
            project_root: None,
        })
    }

    /// Restore a session: create if missing, overwrite (messages + status) if present.
    /// Default: no-op (stores without recovery support ignore restore).
    fn restore(&self, _snapshot: &SessionSnapshot) -> Result<(), ModuleError> {
        Ok(())
    }

    /// All persisted sessions — empty for non-persistent stores.
    fn load_all(&self) -> Result<Vec<SessionSnapshot>, ModuleError> {
        Ok(vec![])
    }

    /// Enumerate known sessions, newest-first by updated_at. Default body maps
    /// `load_all()` — FileBackedStore::new preloads the dir so restarted daemons
    /// are covered; InMemoryStore keeps its documented empty contract.
    fn list(&self) -> Result<Vec<crate::types::SessionListEntry>, ModuleError> {
        let mut entries: Vec<crate::types::SessionListEntry> = self
            .load_all()?
            .into_iter()
            .map(|snap| {
                let preview = crate::types::preview_of(&snap);
                crate::types::SessionListEntry {
                    session_id: snap.session_id,
                    updated_at: snap.updated_at,
                    status: snap.status,
                    preview,
                }
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        Ok(entries)
    }

    /// Archive every Active session idle longer than `ttl`, publishing
    /// EventType::SessionExpired per archived session; returns archived count.
    /// Default: no-op for stores without expiry support.
    fn scan_expired(
        &self,
        _ttl: std::time::Duration,
        _now_epoch: u64,
        _bus: &crate::hooks::EventBus,
    ) -> Result<usize, ModuleError> {
        Ok(0)
    }

    /// Production expiry loop: every `interval`, scan with the current unix time.
    /// Default implementation drives `scan_expired` on a tokio interval.
    fn spawn_expiry_loop(
        self: std::sync::Arc<Self>,
        bus: std::sync::Arc<crate::hooks::EventBus>,
        ttl: std::time::Duration,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()>
    where
        Self: 'static,
    {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let _ = self.scan_expired(ttl, crate::types::now_epoch_secs(), &bus);
            }
        })
    }
}

pub trait ConfigProvider: Send + Sync {
    fn get_str(&self, key: &str) -> Option<String>;
    fn get_int(&self, key: &str) -> Option<i64>;
    /// Optional boolean lookup. The default returns `None` so existing providers keep
    /// compiling; an implementation must return `None` for keys it does not model.
    fn get_bool(&self, _key: &str) -> Option<bool> {
        None
    }
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
    pub fn session(&self) -> Arc<dyn SessionStore> {
        self.session.clone()
    }
    pub fn tools(&self) -> Arc<ToolRegistry> {
        self.tools.clone()
    }
    pub fn providers(&self) -> Arc<ProviderRegistry> {
        self.providers.clone()
    }
    pub fn config(&self) -> Arc<dyn ConfigProvider> {
        self.config.clone()
    }
    pub fn permission(&self) -> Option<Arc<dyn crate::permission::PermissionChecker>> {
        self.tools.checker()
    }
}

// ---- Kernel Message ----

/// Structured tool execution result: the tool's JSON output plus any artifacts
/// (file changes, todo snapshots) it produced. Artifacts are only ever present
/// on `Ok` — failures stay plain `ToolError`. @since 0.15.0
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionOutcome {
    pub result: serde_json::Value,
    pub artifacts: Vec<TurnArtifact>,
}

/// Oneshot result channel for ToolCallRequest — mirrors ServiceCallReply.
pub type ToolCallReply = Arc<
    std::sync::Mutex<
        Option<tokio::sync::oneshot::Sender<Result<ToolExecutionOutcome, crate::error::ToolError>>>,
    >,
>;

#[derive(Debug, Clone)]
pub enum KernelMessage {
    UserRequest {
        session_id: SessionId,
        content: String,
        attachments: Vec<Attachment>,
        /// Per-run overrides from `session/run`; `Default` = no override. @since 0.26.0
        overrides: RunOverrides,
        reply: tokio::sync::mpsc::Sender<crate::types::AgentEvent>,
    },
    ToolCallRequest {
        session_id: SessionId,
        tool_call: ToolCall,
        target: ModuleId,
        reply: ToolCallReply,
    },
    /// v0.3 service routing (design §7 #1): handled by the kernel BEFORE module
    /// broadcast; unknown services get an error reply so the caller never hangs.
    ServiceCall {
        service: ModuleId,
        method: String,
        params: serde_json::Value,
        reply: crate::resolver::ServiceCallReply,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum KernelResponse {
    Handled,
    Streaming(Vec<String>),
}

/// Single construction point for `KernelMessage::UserRequest`, so the variant's
/// shape can grow without touching every call site. @since 0.26.0
impl KernelMessage {
    /// Callers with no per-run overrides; delegates to the overrides form. @since 0.26.0
    pub fn user_request(
        session_id: impl Into<String>,
        content: impl Into<String>,
        reply: tokio::sync::mpsc::Sender<AgentEvent>,
    ) -> Self {
        Self::user_request_with_overrides(session_id, content, RunOverrides::default(), reply)
    }

    pub fn user_request_with_overrides(
        session_id: impl Into<String>,
        content: impl Into<String>,
        overrides: RunOverrides,
        reply: tokio::sync::mpsc::Sender<AgentEvent>,
    ) -> Self {
        KernelMessage::UserRequest {
            session_id: session_id.into(),
            content: content.into(),
            attachments: Vec::new(),
            overrides,
            reply,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ModuleError, ToolError};

    struct TestModule;
    impl Module for TestModule {
        fn id(&self) -> ModuleId {
            "test".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "test".into(),
                name: "Test".into(),
                version: "0.1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(
            &mut self,
            _ctx: &KernelContext,
            _register: &mut ModuleRegister,
        ) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::Value::String("ok".into()))
        }
    }

    struct OtherTool;
    impl Tool for OtherTool {
        fn name(&self) -> &str {
            "other"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
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
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        // same name, same source, Reject → error
        assert!(
            registry
                .register_tool_with_source(
                    Arc::new(DummyTool),
                    ToolSource::Builtin,
                    ReplacePolicy::Reject
                )
                .is_err()
        );
        // Replace → overwrites, still 1 entry
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Replace,
            )
            .unwrap();
        assert_eq!(registry.definitions().len(), 1);
    }

    #[test]
    fn unregister_tool_removes_by_name() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        let removed = registry.unregister_tool("dummy");
        assert!(removed.is_some());
        assert!(registry.get("dummy").is_none());
        assert!(registry.unregister_tool("dummy").is_none()); // idempotent
    }

    #[test]
    fn unregister_by_source_removes_all_and_counts() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Mcp {
                    server_id: "srv".into(),
                },
                ReplacePolicy::Reject,
            )
            .unwrap();
        registry
            .register_tool_with_source(
                Arc::new(OtherTool),
                ToolSource::Plugin {
                    plugin_id: "p1".into(),
                },
                ReplacePolicy::Reject,
            )
            .unwrap();
        let removed = registry.unregister_by_source(&ToolSource::Mcp {
            server_id: "srv".into(),
        });
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

    /// Local mock whose name field mirrors the registry key for readability; it is never read.
    #[allow(dead_code)]
    struct NamedProvider(&'static str);
    #[async_trait]
    impl ModelProvider for NamedProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            Ok(Box::pin(futures::stream::iter(vec![Ok(ModelEvent::Done)])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    #[test]
    fn set_default_switches_the_resolved_provider() {
        let reg = ProviderRegistry::new();
        reg.register("a".into(), Arc::new(NamedProvider("a")));
        reg.register("b".into(), Arc::new(NamedProvider("b")));
        assert_eq!(reg.default_name().as_deref(), Some("a"));
        // the first registered provider is the default and its name resolves alongside it
        assert_eq!(
            reg.get_default_with_name().map(|(n, _)| n),
            Some("a".to_string())
        );
        assert!(reg.set_default("b"));
        assert_eq!(reg.default_name().as_deref(), Some("b"));
        assert!(!reg.set_default("missing"));
        assert_eq!(reg.default_name().as_deref(), Some("b"));
        // get_default_with_name tracks the current default, so a name never pairs with
        // a different provider after set_default
        assert_eq!(
            reg.get_default_with_name().map(|(n, _)| n),
            Some("b".to_string())
        );
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
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("s".into())
        }
        fn append(&self, _s: &SessionId, _m: ChatMessage) -> Result<(), ModuleError> {
            Ok(())
        }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(vec![])
        }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> {
            Ok(0)
        }
    }

    #[test]
    fn session_store_contract_defaults_apply() {
        let store = ContractOnlyStore;
        assert_eq!(store.status(&"s1".into()).unwrap(), SessionStatus::Active);
        assert!(store.archive(&"s1".into()).is_err());
        assert!(store.delete(&"s1".into()).is_err());
    }

    #[test]
    fn default_store_project_root_is_none() {
        let store = ContractOnlyStore;
        let sid: SessionId = "s1".into();
        assert_eq!(store.project_root(&sid), None);
        assert!(
            store
                .set_project_root(&sid, std::path::PathBuf::from("/tmp/p"))
                .is_ok()
        );
        assert_eq!(store.project_root(&sid), None);
    }

    // ---- T6: permission-gated tool execution ----
    use crate::permission::{
        PermissionChecker, PermissionContext, PermissionDecision, PermissionRequestId,
    };

    #[derive(Clone)]
    struct FixedChecker {
        decision: PermissionDecision,
    }
    #[async_trait]
    impl PermissionChecker for FixedChecker {
        fn check(&self, _ctx: &PermissionContext) -> PermissionDecision {
            self.decision.clone()
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            match &self.decision {
                PermissionDecision::Ask { .. } => PermissionDecision::Allow,
                other => other.clone(),
            }
        }
    }

    // Local config mock — v0.1 T11 pattern (no ConfigProviderImpl in v0.1).
    struct MockConfig;
    impl crate::module::ConfigProvider for MockConfig {
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
    fn source_of_returns_registered_source() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Mcp {
                    server_id: "srv-1".into(),
                },
                ReplacePolicy::Reject,
            )
            .unwrap();
        assert_eq!(
            registry.source_of("dummy"),
            Some(ToolSource::Mcp {
                server_id: "srv-1".into()
            })
        );
        assert_eq!(registry.source_of("missing"), None);
    }

    #[test]
    fn kernel_context_permission_accessor_resolves() {
        let registry = ToolRegistry::new();
        registry.set_permission_checker(Arc::new(FixedChecker {
            decision: PermissionDecision::Allow,
        }));
        let ctx = KernelContext {
            session: Arc::new(ContractOnlyStore),
            tools: Arc::new(registry),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig), // local mock — v0.1 T11 pattern (no ConfigProviderImpl in v0.1)
        };
        assert!(ctx.permission().is_some());
    }

    #[tokio::test]
    async fn execute_async_allow_runs_tool() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        registry.set_permission_checker(Arc::new(FixedChecker {
            decision: PermissionDecision::Allow,
        }));
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "dummy".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let result = registry
            .execute_async("dummy", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::Value::String("ok".into()));
    }

    #[tokio::test]
    async fn execute_async_deny_blocks_tool() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        registry.set_permission_checker(Arc::new(FixedChecker {
            decision: PermissionDecision::Deny {
                reason: "denied".into(),
            },
        }));
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "dummy".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let result = registry
            .execute_async("dummy", serde_json::json!({}), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::PermissionDenied { .. })));
    }

    #[tokio::test]
    async fn execute_async_ask_then_allow_runs() {
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        registry.set_permission_checker(Arc::new(FixedChecker {
            decision: PermissionDecision::Ask {
                request_id: PermissionRequestId("r1".into()),
            },
        }));
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "dummy".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let result = registry
            .execute_async("dummy", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_async_without_checker_runs() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "dummy".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: std::path::PathBuf::from("/ws"),
        };
        let result = registry
            .execute_async("dummy", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    struct NoExpiryStore;
    impl SessionStore for NoExpiryStore {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("s".into())
        }
        fn append(&self, _s: &SessionId, _m: ChatMessage) -> Result<(), ModuleError> {
            Ok(())
        }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(vec![])
        }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> {
            Ok(0)
        }
    }

    #[test]
    fn scan_expired_default_is_noop() {
        let store = NoExpiryStore;
        let n = store
            .scan_expired(
                std::time::Duration::from_secs(60),
                0,
                &crate::hooks::EventBus::new(),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    // ---- T7: ToolCallContext + relative-path rebasing ----

    #[test]
    fn default_execute_with_context_delegates() {
        let tool = DummyTool;
        let ctx = ToolCallContext {
            session_id: "s1".into(),
            workspace_root: PathBuf::from("/ws"),
        };
        let input = serde_json::json!({"path": "src/main.rs"});
        let via_ctx = tool.execute_with_context(input.clone(), &ctx).unwrap();
        let direct = tool.execute(input).unwrap();
        assert_eq!(via_ctx, direct);
        assert_eq!(via_ctx, serde_json::Value::String("ok".into()));
    }

    // Overrides execute_with_context *and* rejects execute(), proving
    // execute_async dispatches through the context-aware path.
    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "read"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Err(ToolError::ExecutionFailed {
                name: "read".into(),
                reason: "execute() must not be called".into(),
            })
        }
        fn execute_with_context(
            &self,
            input: serde_json::Value,
            ctx: &ToolCallContext,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({
                "path": input["path"],
                "session_id": ctx.session_id,
                "workspace_root": ctx.workspace_root.to_string_lossy(),
            }))
        }
    }

    #[tokio::test]
    async fn execute_async_rebases_args_and_passes_context() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));
        let perm = PermissionContext {
            session_id: "s42".into(),
            tool: "read".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: PathBuf::from("/ws"),
        };
        let out = registry
            .execute_async("read", serde_json::json!({"path": "src/lib.rs"}), &perm)
            .await
            .unwrap();
        assert_eq!(out["path"], "/ws/src/lib.rs");
        assert_eq!(out["session_id"], "s42");
        assert_eq!(out["workspace_root"], "/ws");
    }

    struct StubProvider(Option<Vec<String>>);

    fn stub_chat_response() -> Result<ChatResponse, ModelError> {
        unimplemented!()
    }

    struct NoModelsProvider;

    #[async_trait]
    impl ModelProvider for NoModelsProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            unimplemented!()
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            stub_chat_response()
        }
    }

    #[async_trait]
    impl ModelProvider for StubProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            unimplemented!()
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            stub_chat_response()
        }
        fn models(&self) -> Vec<String> {
            self.0.clone().unwrap_or_default()
        }
    }

    #[test]
    fn provider_models_default_is_empty() {
        assert!(NoModelsProvider.models().is_empty());
    }

    #[test]
    fn registry_get_returns_registered_provider() {
        let reg = ProviderRegistry::new();
        reg.register("mock".into(), Arc::new(StubProvider(None)));
        assert!(reg.get("mock").is_some());
    }

    #[test]
    fn registry_get_unknown_name_is_none() {
        let reg = ProviderRegistry::new();
        reg.register("mock".into(), Arc::new(StubProvider(None)));
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn list_with_models_is_sorted_and_carries_reported_models() {
        let reg = ProviderRegistry::new();
        reg.register(
            "b".into(),
            Arc::new(StubProvider(Some(vec!["m2".into(), "m1".into()]))),
        );
        reg.register("a".into(), Arc::new(NoModelsProvider));
        assert_eq!(
            reg.list_with_models(),
            vec![
                ("a".to_string(), vec![]),
                ("b".to_string(), vec!["m2".into(), "m1".into()])
            ]
        );
    }

    #[test]
    fn remove_drops_only_the_named_provider() {
        let reg = ProviderRegistry::new();
        reg.register("opencode-go".into(), Arc::new(StubProvider(None)));
        reg.register("deepseek".into(), Arc::new(NoModelsProvider));
        assert!(reg.remove("opencode-go"));
        assert!(reg.get("opencode-go").is_none());
        assert_eq!(reg.list().len(), 1);
        assert!(!reg.remove("opencode-go"));
    }

    #[test]
    fn removing_the_default_provider_leaves_get_default_none() {
        let reg = ProviderRegistry::new();
        reg.register("only".into(), Arc::new(StubProvider(None)));
        assert!(reg.remove("only"));
        assert!(reg.get_default().is_none());
    }

    #[test]
    fn user_request_constructor_builds_the_variant() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        match KernelMessage::user_request("s1", "hi", tx) {
            KernelMessage::UserRequest {
                session_id,
                content,
                attachments,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(content, "hi");
                assert!(attachments.is_empty());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn user_request_with_overrides_carries_them() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let overrides = RunOverrides {
            provider: Some("mock".into()),
            model: Some("m1".into()),
        };
        match KernelMessage::user_request_with_overrides("s1", "hi", overrides, tx) {
            KernelMessage::UserRequest { overrides, .. } => {
                assert_eq!(overrides.provider.as_deref(), Some("mock"));
                assert_eq!(overrides.model.as_deref(), Some("m1"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn user_request_defaults_to_no_overrides() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        match KernelMessage::user_request("s1", "hi", tx) {
            KernelMessage::UserRequest { overrides, .. } => {
                assert!(overrides.provider.is_none() && overrides.model.is_none())
            }
            _ => unreachable!(),
        }
    }
}
