//! Crate-internal `agentj-ipc` tests, split out of `lib.rs`.

mod dispatch;
mod file;
mod frames;
mod lifecycle;
mod pending;
mod pins;
mod provider;
mod run;
mod session;
mod stdio;

use super::*;
use crate::test_support::{MockConfig, stub_session};
use agentj_context::ContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::*;
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::*;
use agentj_model::mock::MockModelProvider;
use agentj_session::InMemoryStore;
use std::sync::Arc;

/// Crate-internal fixtures shared by lib and tcp test modules.
pub(crate) mod test_support {
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop_module::AgentLoopModule;
    use agentj_core::module::*;
    use agentj_core::tool_executor::ToolExecutorModule;
    use agentj_core::turn_runner::TurnRunner;
    use agentj_core::types::*;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

    pub(crate) struct MockConfig;
    impl ConfigProvider for MockConfig {
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

    pub(crate) fn stub_session() -> Arc<InMemoryStore> {
        Arc::new(InMemoryStore::new())
    }

    #[allow(dead_code)]
    pub(crate) fn stub_server() -> crate::IpcServer {
        let session = stub_session();
        crate::IpcServer::new(agentj_core::kernel::Kernel::new(), session)
    }

    /// Build an IpcServer whose kernel has AgentLoopModule + ToolExecutorModule
    /// wired to a MockModelProvider emitting `hello`.
    pub(crate) fn stub_bus_server() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_bus_server_with_usage() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Usage(TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                })),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_agent_loop() -> TurnRunner {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn,
            tools,
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let kernel = agentj_core::kernel::Kernel::new();
        TurnRunner::new(
            ctx,
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        )
    }
}

/// `permission_server` with an explicit approval timeout, so a test can hold
/// an approval pending long enough to observe *how* it gets settled.
fn permission_server_with(
    timeout: std::time::Duration,
) -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    let module = Arc::new(agentj_permission::PermissionModule::new(policies, timeout));
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    server.permission_module = Some(module.clone());
    (server, module)
}

async fn call(server: &IpcServer, method: &str, params: serde_json::Value) -> serde_json::Value {
    let raw =
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":method,"params":params}).to_string();
    serde_json::from_str(&server.handle_request_from(&raw, "c1").await.unwrap()).unwrap()
}

/// Fixture for the cancel regression test: the AgentLoop's ToolRegistry is
/// wired to the SAME PermissionModule the server dispatches into, and the
/// mock script emits a `bash` ToolCall — so a session/run genuinely blocks
/// on a permission Ask while in-flight (the real cancel scenario).
fn blocking_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    blocking_run_server_with(1)
}

/// As [`blocking_run_server`], but the mock script emits `tool_calls` `bash` calls
/// in ONE model round: the run then asks for that many approvals in sequence (a
/// later one arrives only after the previous is settled, i.e. during the drain).
fn blocking_run_server_with(
    tool_calls: usize,
) -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    let module = Arc::new(agentj_permission::PermissionModule::new(
        policies,
        std::time::Duration::from_secs(10),
    ));
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    tools.set_permission_checker(module.clone());
    let providers = Arc::new(ProviderRegistry::new());
    let script = (0..tool_calls)
        .map(|i| {
            Ok(ModelEvent::ToolCall(ToolCall {
                id: format!("call_{i}"),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "curl x"}),
            }))
        })
        .collect();
    providers.register("mock".into(), Arc::new(MockModelProvider::new(script)));
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut register =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut register,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    let mut server = IpcServer::new(kernel, session_dyn);
    server.permission_module = Some(module.clone());
    (server, module)
}

async fn wait_for_pending_run(
    server: &Arc<IpcServer>,
    sid: &str,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if read_or_recover(&server.registry).pending_runs.contains(sid) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
}

struct RepeatingMock {
    c1: std::sync::atomic::AtomicUsize,
    c2: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for RepeatingMock {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        let is_s2 = messages.iter().any(|m| m.content.contains("run2"));
        let n = if is_s2 {
            self.c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        } else {
            self.c1.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        };
        if n == 0 {
            let (tn, args) = if is_s2 {
                ("blocker", serde_json::json!({}))
            } else {
                ("bash", serde_json::json!({"command": "echo hi"}))
            };
            let ev = vec![Ok(ModelEvent::ToolCall(ToolCall {
                id: "call_1".into(),
                name: tn.into(),
                arguments: args,
            }))];
            Ok(Box::pin(futures::stream::iter(ev)))
        } else {
            let ev = vec![Ok(ModelEvent::Delta("done".into())), Ok(ModelEvent::Done)];
            Ok(Box::pin(futures::stream::iter(ev)))
        }
    }
    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<ChatResponse, agentj_core::error::ModelError> {
        Ok(ChatResponse {
            content: "".into(),
            tool_calls: vec![],
        })
    }
}

type SharedLog = Arc<std::sync::Mutex<Vec<String>>>;

/// T5 fixture: `send` appends to a log the test keeps a handle to (the
/// boxed conn inside the registry cannot be downcast, so logs live outside).
struct FakeConn {
    id: String,
    log: SharedLog,
}

fn fake_conn(id: &str) -> (FakeConn, SharedLog) {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    (
        FakeConn {
            id: id.to_string(),
            log: std::sync::Arc::clone(&log),
        },
        log,
    )
}

#[async_trait]
impl Connection for FakeConn {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Stdio
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        if let Ok(mut log) = self.log.lock() {
            log.push(msg.to_string());
        }
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        None
    }
    fn peer_desc(&self) -> String {
        self.id.clone()
    }
    fn writer(&self) -> ConnectionWriter {
        let log = std::sync::Arc::clone(&self.log);
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(move |msg: &str| {
                if let Ok(mut log) = log.lock() {
                    log.push(msg.to_string());
                }
                Ok(())
            }),
        )
    }
}

/// Test connection driven by an mpsc channel: feed lines via `tx`, read
/// sends from `log`, drop `tx` to EOF (recv then returns None).
struct ScriptedConn {
    id: String,
    rx: tokio::sync::mpsc::Receiver<String>,
    log: SharedLog,
}

fn scripted_conn(id: &str) -> (ScriptedConn, SharedLog, tokio::sync::mpsc::Sender<String>) {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    (
        ScriptedConn {
            id: id.to_string(),
            rx,
            log: std::sync::Arc::clone(&log),
        },
        log,
        tx,
    )
}

#[async_trait]
impl Connection for ScriptedConn {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        if let Ok(mut log) = self.log.lock() {
            log.push(msg.to_string());
        }
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        self.rx.recv().await.map(Ok)
    }
    fn peer_desc(&self) -> String {
        self.id.clone()
    }
    fn writer(&self) -> ConnectionWriter {
        let log = std::sync::Arc::clone(&self.log);
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(move |msg: &str| {
                if let Ok(mut log) = log.lock() {
                    log.push(msg.to_string());
                }
                Ok(())
            }),
        )
    }
}

/// Connection-id → sent-message log shared between register_fake_conn and
/// drain_sent (the registry owns the boxed conn; logs live outside it).
static FAKE_LOGS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, SharedLog>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn register_fake_conn(server: &Arc<IpcServer>) -> String {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let id = format!(
        "fake-{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let (conn, log) = fake_conn(&id);
    if let Ok(mut reg) = server.registry.write() {
        reg.register(Box::new(conn));
    }
    if let Ok(mut logs) = FAKE_LOGS.lock() {
        logs.insert(id.clone(), log);
    }
    id
}

fn drain_sent(_server: &Arc<IpcServer>, id: &str) -> Vec<String> {
    FAKE_LOGS
        .lock()
        .ok()
        .and_then(|logs| logs.get(id).cloned())
        .map(|log| lock_or_recover(&log).clone())
        .unwrap_or_default()
}

/// Parsed `$/progress` frames a fake connection received (stream order).
fn progress_frames(server: &Arc<IpcServer>, id: &str) -> Vec<serde_json::Value> {
    drain_sent(server, id)
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .filter(|v| v["method"] == "$/progress")
        .collect()
}

fn extract_sid(resp: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(resp).unwrap();
    v["result"]["session_id"].as_str().unwrap().to_string()
}

/// `request_id` of a `$/permissionRequest` notification.
fn request_id(notification: &str) -> String {
    serde_json::from_str::<serde_json::Value>(notification).unwrap()["params"]["request_id"]
        .as_str()
        .expect("request_id")
        .to_string()
}

/// Wait until `predicate` matches a message the connection has sent.
async fn wait_for_line(log: &SharedLog, predicate: impl Fn(&str) -> bool) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(m) = agentj_core::sync::lock_or_recover(log)
                .iter()
                .find(|m| predicate(m))
            {
                return m.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed out waiting for a connection log line")
}

/// T23 fixture: accepts any `todo` args; the executor derives the snapshot
/// artifact itself (that production path is what T23 broadcasts).
struct TodoMock;

impl Tool for TodoMock {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "test todo"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        let count = input["items"].as_array().map(Vec::len).unwrap_or(0);
        Ok(serde_json::json!({"ok": true, "count": count}))
    }
}

/// T23 fixture: really writes the file so the executor's pre/post-read pair
/// yields a `file_change` artifact (op `add` for a fresh target).
struct WriteMock;

impl Tool for WriteMock {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "test write"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        let path = input["path"].as_str().unwrap_or("");
        let content = input["content"].as_str().unwrap_or("");
        std::fs::write(path, content).map_err(|e| {
            agentj_core::error::ToolError::ExecutionFailed {
                name: "write".into(),
                reason: e.to_string(),
            }
        })?;
        Ok(serde_json::json!({"ok": true}))
    }
}

fn temp_dir_traj(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-traj-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wired_store(dir: &std::path::Path) -> Arc<crate::trajectory::TrajectoryStore> {
    Arc::new(crate::trajectory::TrajectoryStore::new(
        dir.to_path_buf(),
        None,
        None,
    ))
}

fn read_traj_events(
    dir: &std::path::Path,
    sid: &str,
) -> Vec<agentj_protocol::trajectory::TrajectoryEvent> {
    let path = dir.join(format!("{sid}.trajectory"));
    if !path.exists() {
        return vec![];
    }
    let f = std::fs::File::open(path).unwrap();
    std::io::BufRead::lines(std::io::BufReader::new(f))
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect()
}

fn traj_kinds(events: &[agentj_protocol::trajectory::TrajectoryEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(&e.value).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// T5 fixture: serves `script` on the first `chat_stream` of each run and a
/// bare `Done` afterwards, keyed by the run's user content, so two
/// sequential runs on one session each replay one tool batch.
struct TwoRunMockProvider {
    script: Vec<ModelEvent>,
    first_run_calls: std::sync::atomic::AtomicUsize,
    second_run_calls: std::sync::atomic::AtomicUsize,
}

impl TwoRunMockProvider {
    fn new(script: Vec<ModelEvent>) -> Self {
        Self {
            script,
            first_run_calls: std::sync::atomic::AtomicUsize::new(0),
            second_run_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for TwoRunMockProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        let run_calls = if messages.iter().any(|m| m.content.contains("second-turn")) {
            &self.second_run_calls
        } else {
            &self.first_run_calls
        };
        let n = run_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events: Vec<Result<ModelEvent, agentj_core::error::ModelError>> = if n == 0 {
            self.script.iter().cloned().map(Ok).collect()
        } else {
            vec![Ok(ModelEvent::Done)]
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<ChatResponse, agentj_core::error::ModelError> {
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![],
        })
    }
}

fn scripted_run_server(
    script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
) -> IpcServer {
    scripted_run_server_with_tools(script, |_| {})
}

/// Common test-kernel wiring: ToolExecutor + AgentLoop over `provider`.
fn run_server_with_provider(
    provider: Arc<dyn agentj_core::module::ModelProvider>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    register_tools(&tools);
    let providers = Arc::new(ProviderRegistry::new());
    providers.register("mock".into(), provider);
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut register =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut register,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    IpcServer::new(kernel, session_dyn)
}

/// T23: like `scripted_run_server`, but lets the caller register test tools so
/// tool artifacts (todo / write) flow as real `AgentEvent`s through the bus.
fn scripted_run_server_with_tools(
    script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    run_server_with_provider(Arc::new(MockModelProvider::new(script)), register_tools)
}

fn two_run_server(
    script: Vec<agentj_core::types::ModelEvent>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    run_server_with_provider(Arc::new(TwoRunMockProvider::new(script)), register_tools)
}

/// Build the `fn <name>(` needle at runtime: this assertion lives in
/// `lib.rs`, so an inlined needle would match its own source text.
fn fn_needle(name: &str) -> String {
    format!("fn {name}(")
}
