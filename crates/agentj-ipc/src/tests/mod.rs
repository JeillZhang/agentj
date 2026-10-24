//! Crate-internal `agentj-ipc` tests, split out of `lib.rs`.

mod file;
mod lifecycle;
mod pending;
mod provider;
mod run;
mod session;
mod stdio;

use super::*;
use crate::test_support::{MockConfig, stub_bus_server, stub_session};
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

#[test]
fn dispatch_error_is_constructible_from_outside_the_crate() {
    let e = DispatchError::invalid_params_with_data(
        "InvalidParams",
        serde_json::json!({"field": "provider"}),
    );
    assert_eq!(e.code, error_codes::INVALID_PARAMS);
    assert_eq!(e.data.unwrap()["field"], "provider");
    assert!(
        crate::tcp::TcpBindOptions::default()
            .provider_service
            .is_none()
    );
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

/// M1 regression: JSON-RPC 2.0 §2.1 — a notification (request without an
/// id, e.g. $/cancelRequest) must produce NO response line, while a
/// request WITH an id still gets its response.
#[tokio::test]
async fn id_less_notification_gets_no_response() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","method":"session/new"}"#)
        .await
        .unwrap();
    assert!(resp.is_empty(), "notification must not respond: {resp}");
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    assert!(!resp.is_empty(), "id'd request must respond");
    assert!(resp.contains("\"result\""));
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

/// T20: the optional `data` key is omitted from the wire when None (additive
/// discipline), and carried when a parameterized error sets it.
#[test]
fn error_object_omits_data_when_none() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: serde_json::json!(9),
        result: None,
        error: Some(JsonRpcError {
            code: -32602,
            message: "bad params".into(),
            data: None,
        }),
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert!(
        v["error"].get("data").is_none(),
        "data key must be omitted when None: {v}"
    );
    assert_eq!(v["error"]["code"], -32602);
    assert_eq!(v["error"]["message"], "bad params");

    let err = DispatchError::invalid_params_with_data(
        "bad root",
        serde_json::json!({ "path": "/x", "reason": "invalid_project_root" }),
    );
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["code"], -32602);
    assert_eq!(v["data"]["reason"], "invalid_project_root");
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_object());
    assert_eq!(v["error"]["code"], -32601); // Method not found
    assert_eq!(v["id"], 3, "error response must echo request id");
}

#[tokio::test]
async fn invalid_json_returns_parse_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("not json").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32700); // Parse error
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[tokio::test]
async fn schema_invalid_request_echoes_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], 10);
}

#[tokio::test]
async fn schema_invalid_request_echoes_string_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":"abc","params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], "abc");
}

#[tokio::test]
async fn non_object_json_returns_invalid_request_with_null_id() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("42").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600);
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[test]
fn request_id_extraction_accepts_only_jsonrpc_id_types() {
    let cases = [
        (
            serde_json::json!({"id": "abc"}),
            Some(serde_json::json!("abc")),
        ),
        (serde_json::json!({"id": 7}), Some(serde_json::json!(7))),
        (
            serde_json::json!({"id": null}),
            Some(serde_json::Value::Null),
        ),
        (serde_json::json!({}), None),
        (serde_json::json!({"id": {"x": 1}}), None),
        (serde_json::json!({"id": [1]}), None),
        (serde_json::json!({"id": true}), None),
        (serde_json::json!("42"), None),
    ];
    for (value, expected) in cases {
        assert_eq!(extract_request_id(&value), expected, "value: {value}");
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

#[test]
fn todo_event_to_protocol_value_roundtrip() {
    use agentj_core::artifacts::{TodoItemArtifact, TodoStatusArtifact};
    use agentj_protocol::streaming::{ProgressValue, TodoItem, TodoStatus};

    let wire = todo_items_into_wire(vec![
        TodoItemArtifact {
            id: "t1".into(),
            content: "first".into(),
            status: TodoStatusArtifact::Pending,
        },
        TodoItemArtifact {
            id: "t2".into(),
            content: "second".into(),
            status: TodoStatusArtifact::InProgress,
        },
        TodoItemArtifact {
            id: "t3".into(),
            content: "third".into(),
            status: TodoStatusArtifact::Completed,
        },
        TodoItemArtifact {
            id: "t4".into(),
            content: "fourth".into(),
            status: TodoStatusArtifact::Cancelled,
        },
    ]);
    assert_eq!(
        wire,
        vec![
            TodoItem {
                id: "t1".into(),
                content: "first".into(),
                status: TodoStatus::Pending,
            },
            TodoItem {
                id: "t2".into(),
                content: "second".into(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                id: "t3".into(),
                content: "third".into(),
                status: TodoStatus::Completed,
            },
            TodoItem {
                id: "t4".into(),
                content: "fourth".into(),
                status: TodoStatus::Cancelled,
            },
        ],
        "all four statuses map 1:1"
    );

    let value = ProgressValue::Todo {
        session_id: "s1".into(),
        run_id: None,
        revision: 7,
        items: wire.clone(),
    };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["kind"], "todo");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["revision"], 7);
    assert_eq!(json["items"][1]["status"], "in_progress");
    let back: ProgressValue = serde_json::from_value(json).unwrap();
    match back {
        ProgressValue::Todo {
            session_id,
            revision,
            items,
            ..
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(revision, 7);
            assert_eq!(items, wire, "items survive the wire roundtrip");
        }
        other => panic!("expected todo, got {other:?}"),
    }
}

#[test]
fn file_change_event_to_protocol_value_roundtrip() {
    use agentj_core::artifacts::{FileChangeArtifact, FileChangeOp};
    use agentj_protocol::streaming::{FileChangeKind, ProgressValue};

    let change = file_change_into_wire(FileChangeArtifact {
        path: "src/lib.rs".into(),
        op: FileChangeOp::Modify,
        additions: 3,
        deletions: 1,
        tool_call_id: Some("c1".into()),
        diff: Some("@@ -1,2 +1,2 @@\n-a\n+b\n".into()),
        diff_truncated: false,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(change.op, FileChangeKind::Modify);
    assert_eq!(change.diff_truncated, None, "false must stay omitted");
    let value = ProgressValue::FileChange {
        session_id: "s1".into(),
        run_id: None,
        change,
    };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["kind"], "file_change");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["change"]["path"], "src/lib.rs");
    assert_eq!(json["change"]["op"], "modify");
    assert_eq!(json["change"]["additions"], 3);
    assert_eq!(json["change"]["deletions"], 1);
    assert_eq!(json["change"]["tool_call_id"], "c1");
    assert!(
        json["change"]["diff"].as_str().unwrap().contains("+b"),
        "diff preserved: {json}"
    );
    assert!(
        json["change"].get("diff_truncated").is_none(),
        "false diff_truncated must be skipped: {json}"
    );
    let back: ProgressValue = serde_json::from_value(json).unwrap();
    match back {
        ProgressValue::FileChange {
            session_id, change, ..
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(change.op, FileChangeKind::Modify);
            assert_eq!(change.diff_truncated, None);
            assert_eq!(change.path, "src/lib.rs");
        }
        other => panic!("expected file_change, got {other:?}"),
    }

    let add = file_change_into_wire(FileChangeArtifact {
        path: "new.txt".into(),
        op: FileChangeOp::Add,
        additions: 1,
        deletions: 0,
        tool_call_id: None,
        diff: None,
        diff_truncated: false,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(add.op, FileChangeKind::Add);
    let add_json = serde_json::to_value(&add).unwrap();
    assert!(add_json.get("tool_call_id").is_none());
    assert!(add_json.get("diff").is_none());

    let delete = file_change_into_wire(FileChangeArtifact {
        path: "gone.txt".into(),
        op: FileChangeOp::Delete,
        additions: 0,
        deletions: 4,
        tool_call_id: Some("c9".into()),
        diff: None,
        diff_truncated: true,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(delete.op, FileChangeKind::Delete);
    let delete_json = serde_json::to_value(&delete).unwrap();
    assert_eq!(delete_json["diff_truncated"], true, "truncation surfaced");
}

/// T5: `is_valid_run_id` requires a parseable UUID within the 128-char cap;
/// `sanitize_run_id` maps every reject (including `None`) to `None`.
#[test]
fn run_id_validators_accept_uuids_and_reject_malformed() {
    let valid = Uuid::new_v4().to_string();
    assert!(is_valid_run_id(&valid), "a real UUID is valid: {valid}");
    assert!(!is_valid_run_id("not-a-uuid"));
    assert!(
        !is_valid_run_id(&"x".repeat(129)),
        "over the 128-char cap is invalid"
    );
    assert_eq!(sanitize_run_id(None), None);
    assert_eq!(sanitize_run_id(Some("not-a-uuid".into())), None);
    assert_eq!(sanitize_run_id(Some(valid.clone())), Some(valid));
}

/// T23 AC: a run yielding todo + write artifacts must broadcast both
/// `$/progress` kinds to subscribers and record them once for replay.
#[tokio::test]
async fn subscriber_receives_todo_and_file_change_broadcast() {
    let dir = temp_dir_traj("t23-artifacts");
    let target = std::env::temp_dir().join(format!("aj-ipc-t23-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let mut server = scripted_run_server_with_tools(
        vec![
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_todo".into(),
                    name: "todo".into(),
                    arguments: serde_json::json!({"items": [
                        {"content": "write tests", "status": "in_progress"},
                        {"content": "implement", "status": "pending"}
                    ]}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_write".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::Done),
        ],
        |tools| {
            tools.register(Arc::new(TodoMock));
            tools.register(Arc::new(WriteMock));
        },
    );
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    server.handle_request_from(&run_raw, &a).await.unwrap();

    let frames: Vec<serde_json::Value> = drain_sent(&server, &b)
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .filter(|v| v["method"] == "$/progress")
        .collect();
    let todo = frames
        .iter()
        .find(|f| f["params"]["value"]["kind"] == "todo")
        .unwrap_or_else(|| panic!("subscriber must receive a todo frame: {frames:?}"));
    assert_eq!(todo["params"]["token"], 3, "token echoes the request id");
    assert_eq!(todo["params"]["value"]["session_id"], sid);
    assert_eq!(todo["params"]["value"]["revision"], 1);
    assert_eq!(todo["params"]["value"]["items"][0]["status"], "in_progress");
    assert_eq!(todo["params"]["value"]["items"][1]["status"], "pending");

    let change = frames
        .iter()
        .find(|f| f["params"]["value"]["kind"] == "file_change")
        .unwrap_or_else(|| panic!("subscriber must receive a file_change frame: {frames:?}"));
    assert_eq!(change["params"]["token"], 3);
    assert_eq!(change["params"]["value"]["session_id"], sid);
    assert_eq!(change["params"]["value"]["change"]["op"], "add");
    assert_eq!(change["params"]["value"]["change"]["path"], path);
    assert_eq!(change["params"]["value"]["change"]["additions"], 2);
    assert_eq!(
        change["params"]["value"]["change"]["tool_call_id"],
        "call_write"
    );
    assert!(
        change["params"]["value"]["change"]["diff"]
            .as_str()
            .unwrap()
            .contains("+b"),
        "diff preserved: {change}"
    );
    assert!(
        change["params"]["value"]["change"]
            .get("diff_truncated")
            .is_none(),
        "false diff_truncated stays omitted: {change}"
    );

    let events = read_traj_events(&dir, &sid);
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "tool_result",
            "todo",
            "tool_call",
            "tool_result",
            "file_change",
            "run_model"
        ],
        "each artifact is recorded exactly once, in stream order"
    );
    let agentj_protocol::trajectory::TrajectoryValue::Todo {
        revision, items, ..
    } = &events[3].value
    else {
        panic!("todo node at index 3: {events:?}")
    };
    assert_eq!(*revision, 1);
    assert_eq!(items[0].content, "write tests");
    assert_eq!(
        items[1].status,
        agentj_protocol::streaming::TodoStatus::Pending
    );
    let agentj_protocol::trajectory::TrajectoryValue::FileChange { change, .. } = &events[6].value
    else {
        panic!("file_change node at index 6: {events:?}")
    };
    assert_eq!(change.op, agentj_protocol::streaming::FileChangeKind::Add);
    assert_eq!(change.path, path);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// T5 AC: a live run broadcasts Todo/FileChange frames tagged with the same
/// `run_id` as `SessionRunResult`, plus per-run `seq`/`source`.
#[tokio::test]
async fn broadcast_includes_run_id() {
    let dir = temp_dir_traj("run-id-broadcast");
    let target = std::env::temp_dir().join(format!("aj-ipc-runid-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let mut server = scripted_run_server_with_tools(
        vec![
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_todo".into(),
                    name: "todo".into(),
                    arguments: serde_json::json!({"items": [
                        {"content": "write tests", "status": "in_progress"}
                    ]}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_write".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::Done),
        ],
        |tools| {
            tools.register(Arc::new(TodoMock));
            tools.register(Arc::new(WriteMock));
        },
    );
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_resp = server.handle_request_from(&run_raw, &a).await.unwrap();
    let run_v: serde_json::Value = serde_json::from_str(&run_resp).unwrap();
    let run_id = run_v["result"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("SessionRunResult must carry run_id: {run_resp}"))
        .to_string();
    assert!(Uuid::parse_str(&run_id).is_ok(), "real UUID: {run_id}");

    let frames = progress_frames(&server, &b);
    let todos: Vec<_> = frames
        .iter()
        .filter(|f| f["params"]["value"]["kind"] == "todo")
        .collect();
    assert!(!todos.is_empty(), "todo frame expected: {frames:?}");
    assert!(
        todos
            .iter()
            .all(|f| f["params"]["value"]["run_id"].as_str() == Some(run_id.as_str())),
        "every todo frame carries the run id: {todos:?}"
    );
    let changes: Vec<_> = frames
        .iter()
        .filter(|f| f["params"]["value"]["kind"] == "file_change")
        .collect();
    assert!(
        !changes.is_empty(),
        "file_change frame expected: {frames:?}"
    );
    for c in &changes {
        let change = &c["params"]["value"]["change"];
        assert_eq!(
            c["params"]["value"]["run_id"].as_str(),
            Some(run_id.as_str()),
            "outer frame run_id: {c}"
        );
        assert_eq!(
            change["run_id"].as_str(),
            Some(run_id.as_str()),
            "wire change run_id: {c}"
        );
        assert_eq!(change["seq"], 1, "first change of the run: {c}");
        assert_eq!(change["source"], "write", "write-sourced change: {c}");
    }
    let recorded = read_traj_events(&dir, &sid);
    let todo_run = recorded.iter().find_map(|e| match &e.value {
        agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. } => Some(run_id.clone()),
        _ => None,
    });
    assert_eq!(
        todo_run.flatten().as_deref(),
        Some(run_id.as_str()),
        "replay node shares the broadcast run id"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// BUG-004 regression: §7.2 — when the client omits `progress_token`, the
/// `$/progress` `params.token` must echo the request id, never null.
#[tokio::test]
async fn progress_broadcast_uses_request_id_token_when_token_omitted() {
    let server = Arc::new(stub_bus_server());
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
            &a,
        )
        .await
        .unwrap();
    let b_sent = drain_sent(&server, &b);
    let progress = b_sent
        .iter()
        .find(|m| m.contains("$/progress"))
        .unwrap_or_else(|| panic!("subscriber must receive $/progress: {b_sent:?}"));
    let v: serde_json::Value = serde_json::from_str(progress).unwrap();
    assert!(
        !v["params"]["token"].is_null(),
        "token must never be null: {progress}"
    );
    assert_eq!(
        v["params"]["token"], 2,
        "§7.2: omitted progress_token must fall back to the request id: {progress}"
    );
}

/// BUG-004 edge: a `session/run` notification (no request id) still
/// broadcasts `$/progress`; the token falls back to a deterministic
/// schema-valid number instead of null.
#[tokio::test]
async fn progress_broadcast_notification_without_id_keeps_token_valid() {
    let server = Arc::new(stub_bus_server());
    let a = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
            &a,
        )
        .await
        .unwrap();
    let a_sent = drain_sent(&server, &a);
    let progress = a_sent
        .iter()
        .find(|m| m.contains("$/progress"))
        .unwrap_or_else(|| panic!("initiator must receive $/progress: {a_sent:?}"));
    let v: serde_json::Value = serde_json::from_str(progress).unwrap();
    assert!(
        v["params"]["token"].is_i64() || v["params"]["token"].is_string(),
        "token must stay ProgressToken (integer|string), got: {progress}"
    );
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

#[tokio::test]
async fn a_panicking_dispatch_becomes_an_internal_panic_error() {
    let err = isolate_panic("kernel/status", async { panic!("boom") })
        .await
        .expect_err("a panicking dispatch must not propagate");
    assert_eq!(err.code, -32603);
    assert_eq!(err.message, "InternalPanic");
    let data = err.data.expect("error.data carries the panic details");
    assert_eq!(data["method"], "kernel/status");
    assert_eq!(data["reason"], "boom");
}

#[tokio::test]
async fn a_normal_dispatch_passes_through_the_panic_guard() {
    let ok = isolate_panic("kernel/status", async { Ok(serde_json::json!({ "a": 1 })) })
        .await
        .expect("a successful dispatch keeps its result");
    assert_eq!(ok["a"], 1);
}

/// The route table must stay in lockstep with the protocol registry: the
/// client-callable methods of `protocol/methods.json` (minus the two that
/// are handled outside dispatch: `initialized` is a notification, `exit`
/// closes the transport) are exactly the methods `dispatch_request` routes.
#[test]
fn dispatch_table_matches_methods_json() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/methods.json"
    ))
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut expected: Vec<String> = json["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            matches!(
                m["direction"].as_str(),
                Some("client_to_server") | Some("both")
            )
        })
        .map(|m| m["method"].as_str().unwrap().to_string())
        .filter(|m| m != "initialized" && m != "exit")
        .collect();
    expected.sort();
    let mut actual: Vec<String> = DISPATCH_METHODS.iter().map(|s| s.to_string()).collect();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(DISPATCH_METHODS.len(), 23);
    let unique: std::collections::BTreeSet<&String> = actual.iter().collect();
    assert_eq!(
        actual.len(),
        unique.len(),
        "duplicate method in DISPATCH_METHODS"
    );
}

/// Every method the table advertises must have a real match arm; a method
/// added to `DISPATCH_METHODS` without an arm would answer
/// `method_not_found` while the table claimed it was routed.
#[test]
fn every_dispatch_method_appears_in_the_match() {
    let src = include_str!("../dispatch.rs");
    for method in DISPATCH_METHODS {
        assert!(
            src.contains(&format!("\"{method}\" =>")),
            "{method} has no match arm"
        );
    }
}

/// Build the `fn <name>(` needle at runtime: this assertion lives in
/// `lib.rs`, so an inlined needle would match its own source text.
fn fn_needle(name: &str) -> String {
    format!("fn {name}(")
}

/// The three request-framing entry points live in `dispatch.rs`; `lib.rs`
/// keeps only the module declaration. Pinned so a later edit cannot quietly
/// grow the root file back.
#[test]
fn dispatch_functions_moved_out_of_lib() {
    for name in ["dispatch_request", "handle_request_from", "auth_gate"] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../dispatch.rs").contains(&needle),
            "{name} missing"
        );
    }
}

/// The `$/progress` frame builders and `attach_git_statuses` live in `frames.rs`.
#[test]
fn frame_builders_moved_out_of_lib() {
    for name in [
        "build_progress_frame",
        "todo_items_into_wire",
        "attach_git_statuses",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../frames.rs").contains(&needle),
            "{name} missing from frames.rs"
        );
    }
}
