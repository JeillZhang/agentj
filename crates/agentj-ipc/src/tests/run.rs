//! `run` area tests.

use super::*;
use crate::handlers::run::build_run_reply;
use crate::test_support::{MockConfig, stub_bus_server, stub_bus_server_with_usage, stub_session};
use crate::*;
use agentj_core::DefaultContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::*;
use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::*;
use agentj_protocol::session::SessionRunResult;
use agentj_session::InMemoryStore;
use std::sync::Arc;

struct BlockerTool;
impl agentj_core::module::Tool for BlockerTool {
    fn name(&self) -> &str {
        "blocker"
    }
    fn description(&self) -> &str {
        "blocks"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn execute(
        &self,
        _i: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        std::thread::sleep(std::time::Duration::from_secs(1));
        Ok(serde_json::json!({"ok": 1}))
    }
}

fn repeating_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    policies.insert(
        "blocker".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Allow,
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
    tools.register(Arc::new(BlockerTool));
    tools.set_permission_checker(module.clone());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register(
        "mock".into(),
        Arc::new(RepeatingMock {
            c1: std::sync::atomic::AtomicUsize::new(0),
            c2: std::sync::atomic::AtomicUsize::new(0),
        }),
    );
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut reg =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut reg,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(DefaultContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut reg)
        .unwrap();
    let mut server = IpcServer::new(kernel, session_dyn);
    server.permission_module = Some(module.clone());
    (server, module)
}

/// Provider double recording every `ModelOptions` it is handed, so the
/// `session/run` dispatch tests can assert which model actually reached the
/// provider (T9: per-run `provider`/`model` overrides).
struct CapturingMock(Arc<std::sync::Mutex<Vec<ModelOptions>>>);

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for CapturingMock {
    async fn chat_stream(
        &self,
        _m: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        lock_or_recover(&self.0).push(options);
        let ev = vec![Ok(ModelEvent::Delta("ok".into())), Ok(ModelEvent::Done)];
        Ok(Box::pin(futures::stream::iter(ev)))
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

/// Per-provider options logs of one capturing server. `mock` is registered
/// first and is therefore the *default* provider, `other` the alternative:
/// two separate sinks are what makes a `provider` override observable (a
/// turn served by `other` must leave `mock` empty, and vice versa).
struct CapturedRuns {
    mock: Arc<std::sync::Mutex<Vec<ModelOptions>>>,
    other: Arc<std::sync::Mutex<Vec<ModelOptions>>>,
}

/// Same wiring as `repeating_run_server` (session store + ToolRegistry +
/// ProviderRegistry + kernel modules), with TWO capturing providers — `mock`
/// first (hence the default), `other` second — each behind its own options
/// log, and no permission policies (the mocks never call a tool). Returns
/// the server and both logs.
fn capturing_run_server() -> (IpcServer, CapturedRuns) {
    let mock = Arc::new(std::sync::Mutex::new(Vec::new()));
    let other = Arc::new(std::sync::Mutex::new(Vec::new()));
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register("mock".into(), Arc::new(CapturingMock(Arc::clone(&mock))));
    providers.register("other".into(), Arc::new(CapturingMock(Arc::clone(&other))));
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut reg =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut reg,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(DefaultContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut reg)
        .unwrap();
    (
        IpcServer::new(kernel, session_dyn),
        CapturedRuns { mock, other },
    )
}

/// Dispatch one `session/run` request against a fresh capturing server and
/// return the parsed response plus both providers' recorded `ModelOptions`.
async fn call_session_run(params: Value) -> (Value, CapturedRuns) {
    let (server, caps) = capturing_run_server();
    let raw = serde_json::json!({
        "jsonrpc": "2.0", "id": 9, "method": "session/run", "params": params,
    })
    .to_string();
    let line = server
        .handle_request_from(&raw, "conn-capture")
        .await
        .expect("dispatch returns a JSON-RPC line");
    let resp: Value = serde_json::from_str(&line).expect("response is JSON");
    (resp, caps)
}

#[tokio::test]
async fn run_turn_stream_with_routes_to_explicit_provider() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream_with(
        &sid,
        "hi",
        RunOverrides {
            provider: Some("other".into()),
            model: Some("m1".into()),
        },
    );
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(lock_or_recover(&caps.other).last().unwrap().model, "m1");
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "the default provider must not serve a turn named for another provider"
    );
}

#[tokio::test]
// Regression pin (also green before this task): proves the default path is unchanged.
async fn run_turn_stream_default_matches_overrides_default() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream(&sid, "hi");
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
    assert!(lock_or_recover(&caps.other).is_empty());
}

#[tokio::test]
async fn run_turn_stream_with_blank_overrides_are_absent() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream_with(
        &sid,
        "hi",
        RunOverrides {
            provider: Some("   ".into()),
            model: Some(String::new()),
        },
    );
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
    assert!(lock_or_recover(&caps.other).is_empty());
}

#[tokio::test]
async fn session_run_passes_model_override_to_provider() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "mock", "model": "custom-model"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "custom-model"
    );
}

/// The default provider is `mock` (registered first), so naming `other`
/// proves the wire `provider` string really reaches provider selection.
#[tokio::test]
async fn session_run_provider_override_selects_that_provider() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "other", "model": "other-model"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.other).last().unwrap().model,
        "other-model",
        "the named provider must serve the turn"
    );
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "the default provider must not serve a turn named for another provider"
    );
}

/// A well-formed but unregistered provider name must fail the run on the
/// wire with the actionable message the turn runner builds. Turn errors
/// reach the client as `AgentEvent::Error` -> `DispatchError::internal`
/// (code `-32603`); the registered-name list order is a `HashMap`
/// iteration, so only membership is asserted.
#[tokio::test]
async fn session_run_unknown_provider_fails_on_the_wire() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "nope"
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32603, "{resp}");
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("unknown provider: nope"), "{msg}");
    assert!(msg.contains("mock"), "{msg}");
    assert!(msg.contains("other"), "{msg}");
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "no provider may serve a turn with an unknown provider name"
    );
    assert!(
        lock_or_recover(&caps.other).is_empty(),
        "no provider may serve a turn with an unknown provider name"
    );
}

#[tokio::test]
async fn session_run_without_overrides_uses_default_model() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
}

#[tokio::test]
async fn session_run_null_fields_are_absent() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": null, "model": null
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
}

#[tokio::test]
async fn session_run_blank_provider_is_treated_as_absent() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "   "
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert!(
        !lock_or_recover(&caps.mock).is_empty(),
        "default provider must still serve the turn"
    );
}

#[tokio::test]
async fn session_run_non_string_model_is_invalid_params() {
    let (resp, _caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "model": 123
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32602);
    assert_eq!(resp["error"]["data"]["field"], "model");
}

#[tokio::test]
async fn session_run_non_string_provider_is_invalid_params() {
    let (resp, _caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": {}
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32602);
    assert_eq!(resp["error"]["data"]["field"], "provider");
}

#[tokio::test]
async fn cancel_request_with_session_id_routes_precisely() {
    let (server, module) = repeating_run_server();
    let server = Arc::new(server);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    module.set_notifier(Arc::new(move |p: &PermissionRequestParams, sid: &str| {
        let _ = tx.send((sid.to_string(), p.request_id.clone()));
    }));
    let raw1 = r#"{"jsonrpc":"2.0","id":10,"method":"session/run","params":{"session_id":"s1","content":"run1"}}"#;
    let raw2 = r#"{"jsonrpc":"2.0","id":11,"method":"session/run","params":{"session_id":"s2","content":"run2"}}"#;
    let run1 = {
        let s = Arc::clone(&server);
        tokio::spawn(async move { s.handle_request_from(raw1, "conn-a").await.unwrap() })
    };
    let run2 = {
        let s = Arc::clone(&server);
        tokio::spawn(async move { s.handle_request_from(raw2, "conn-b").await.unwrap() })
    };
    wait_for_pending_run(&server, "s1")
        .await
        .expect("s1 must be in-flight");
    wait_for_pending_run(&server, "s2")
        .await
        .expect("s2 must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (sid, rid) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("s1 must block on Ask")
        .expect("channel closed");
    assert_eq!(sid, "s1", "first Ask must be s1");
    let rid_s1 = Some(rid);
    let cancel =
        r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s2"}}"#;
    let _ = server.handle_request_from(cancel, "conn-a").await.unwrap();
    let resp2 = tokio::time::timeout(std::time::Duration::from_secs(5), run2)
        .await
        .expect("cancelled run must finish")
        .unwrap();
    assert!(
        resp2.contains("-32800"),
        "cancelled must get -32800: {resp2}"
    );
    assert!(
        read_or_recover(&server.registry)
            .pending_runs
            .contains("s1"),
        "s1 stays"
    );
    let rid1 = rid_s1.clone().expect("s1 rid");
    let raw_ok = format!(
        r#"{{"jsonrpc":"2.0","id":12,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid1
    );
    let _ = server.handle_request_from(&raw_ok, "conn-a").await.unwrap();
    let resp1 = tokio::time::timeout(std::time::Duration::from_secs(5), run1)
        .await
        .expect("s1 must finish")
        .unwrap();
    assert!(resp1.contains("\"result\""), "s1 must complete: {resp1}");
}

#[tokio::test]
async fn cancel_request_without_session_id_single_inflight_falls_back() {
    let (server, module) = blocking_run_server();
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
        let _ = server.handle_request(raw).await.unwrap();
        assert_eq!(
            handle.await.unwrap(),
            PermissionDecision::Deny {
                reason: "cancelled".into()
            }
        );
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn cancel_request_without_session_id_multiple_inflight_is_noop() {
    let (server, module) = blocking_run_server();
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s2".to_string()));
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask")
    };
    let module2 = module.clone();
    let rid = request_id.clone();
    let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
    let _ = server.handle_request(raw).await.unwrap();
    let out = tokio::time::timeout(std::time::Duration::from_millis(200), handle).await;
    assert!(out.is_err(), "ambiguous cancel must NOT resolve approvals");
}

#[tokio::test]
async fn session_run_streams_delta() {
    let server = stub_bus_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":2,"method":"session/run","#,
        r#""params":{"session_id":"s1","content":"hi"}}"#,
    );
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    // when: session/run responds, then: result carries `content` per SessionRunResult
    assert_eq!(v["result"]["content"], "hello");
    assert!(v["result"].get("response").is_none());
}

#[test]
fn run_reply_omits_identity_when_unknown() {
    let result = SessionRunResult {
        content: "ok".into(),
        tool_calls: None,
        usage: None,
        run_id: None,
        provider: None,
        model: None,
    };
    let value = build_run_reply(&result);
    assert!(
        value.get("provider").is_none(),
        "must be absent, not null: {value}"
    );
    assert!(value.get("model").is_none());
}

#[test]
fn run_reply_carries_identity_when_known() {
    let result = SessionRunResult {
        content: "ok".into(),
        tool_calls: None,
        usage: None,
        run_id: None,
        provider: Some("a".into()),
        model: Some("m".into()),
    };
    let value = build_run_reply(&result);
    assert_eq!(value["provider"], "a");
    assert_eq!(value["model"], "m");
}

#[tokio::test]
async fn session_run_result_includes_usage_when_reported() {
    let server = stub_bus_server_with_usage();
    let raw = r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"], "hello");
    assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
    assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
}

#[tokio::test]
async fn session_run_missing_params_echoes_id_and_invalid_params() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/run"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["id"], 10,
        "error response must echo request id, got: {resp}"
    );
    assert_eq!(
        v["error"]["code"], -32602,
        "missing params is Invalid params, got: {resp}"
    );
    assert!(v["error"]["message"].is_string());
}

#[tokio::test]
async fn session_run_missing_session_id_echoes_id_and_invalid_params() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(
            r#"{"jsonrpc":"2.0","id":7,"method":"session/run","params":{"content":"hi"}}"#,
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["id"], 7,
        "error response must echo request id, got: {resp}"
    );
    assert_eq!(
        v["error"]["code"], -32602,
        "missing session_id is Invalid params, got: {resp}"
    );
}

#[tokio::test]
async fn concurrent_run_on_same_session_is_rejected() {
    // stub mock finishes instantly — the blocking fixture keeps run1 in
    // flight deterministically while run2 arrives.
    let (server, _module) = blocking_run_server();
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        let conn = a.clone();
        async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run1 must be in flight");
    let run2 = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"again"}}}}"#
    );
    let r2 = server.handle_request_from(&run2, &a).await.unwrap();
    assert!(
        r2.contains(r#""code":-32805"#),
        "second run must be SessionBusy: {r2}"
    );
    let _ = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":10,"method":"$/cancelRequest","params":{"id":5}}"#,
            &a,
        )
        .await
        .unwrap();
    let _ = run_task.await;
}

#[tokio::test]
async fn run_records_user_thinking_usage_and_keeps_response_shape() {
    let dir = temp_dir_traj("usage");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"], "hello");
    assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
    assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "thinking", "usage", "run_model"]
    );
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[1].seq, 2);
    assert_eq!(events[2].seq, 3);
    let agentj_protocol::trajectory::TrajectoryValue::User { content } = &events[0].value else {
        panic!()
    };
    assert_eq!(content, "hi");
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!()
    };
    assert_eq!(content, "hello");
    assert_eq!(events[2].session_id, "s1");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_records_one_merged_thinking_node_for_two_deltas() {
    let dir = temp_dir_traj("merge");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("hel".into())),
        Ok(agentj_core::types::ModelEvent::Delta("lo".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "thinking", "run_model"],
        "exactly two nodes"
    );
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!("expected one merged thinking node, got {events:?}")
    };
    assert_eq!(content, "hello", "both deltas concatenated into ONE node");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_drops_zero_width_only_deltas_instead_of_recording_blank_thinking() {
    let dir = temp_dir_traj("zwsp");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("\u{200b}".into())),
        Ok(agentj_core::types::ModelEvent::Delta(" \n".into())),
        Ok(agentj_core::types::ModelEvent::Delta("\u{feff}".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "run_model"],
        "no blank thinking node"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_keeps_thinking_that_contains_visible_glyphs() {
    let dir = temp_dir_traj("zwsp-visible");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("\u{200b}".into())),
        Ok(agentj_core::types::ModelEvent::Delta("ok".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["user", "thinking", "run_model"]);
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!("expected a thinking node, got {events:?}")
    };
    assert_eq!(content, "\u{200b}ok");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_visually_blank_covers_zero_width_and_whitespace_only_text() {
    for blank in [
        "",
        " ",
        "\n\t ",
        "\u{200b}",
        "\u{200b}\u{feff}",
        " \u{200b}\n",
    ] {
        assert!(IpcServer::is_visually_blank(blank), "{blank:?}");
    }
    for visible in ["a", "0", "-", "\u{200b}ok", "分析"] {
        assert!(!IpcServer::is_visually_blank(visible), "{visible:?}");
    }
}

#[tokio::test]
async fn run_records_tool_call_and_tool_result_nodes() {
    let dir = temp_dir_traj("tools");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::ToolCall(
            agentj_core::types::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "curl x"}),
            },
        )),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "tool_call", "tool_result", "run_model"]
    );
    let agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call } = &events[1].value
    else {
        panic!("tool_call node at index 1: {events:?}")
    };
    assert_eq!(tool_call.id, "call_1");
    assert_eq!(tool_call.name, "bash");
    assert_eq!(tool_call.arguments["command"], "curl x");
    let agentj_protocol::trajectory::TrajectoryValue::ToolResult {
        tool_call_id,
        content,
    } = &events[2].value
    else {
        panic!("tool_result node at index 2: {events:?}")
    };
    assert_eq!(tool_call_id, "call_1");
    assert!(
        content.starts_with("Error:"),
        "unregistered tool surfaces as executor error CONTENT (C9: content is a plain string): {content}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_records_error_node_on_provider_failure() {
    let dir = temp_dir_traj("boom");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("x".into())),
        Err(agentj_core::error::ModelError::Network("boom".into())),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(
        resp.contains("-32603"),
        "internal error, not cancel: {resp}"
    );
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["user", "thinking", "error"]);
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!()
    };
    assert_eq!(content, "x");
    let agentj_protocol::trajectory::TrajectoryValue::Error { message } = &events[2].value else {
        panic!()
    };
    assert!(
        message.contains("boom"),
        "error node carries the failure: {message}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn cancelled_run_trajectory_has_no_error_or_tail_nodes() {
    let dir = temp_dir_traj("cancel");
    let (mut server, _module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
                .await
                .unwrap()
        }
    });
    wait_for_pending_run(&server, "s1")
        .await
        .expect("run must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = server
        .handle_request(
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#,
        )
        .await
        .unwrap();
    let resp = run.await.unwrap();
    // Cancel should surface -32800; allow a brief retry if race produced tool_result instead.
    if !resp.contains("-32800") {
        // If we got a normal result, the permission deny path won; still verify trajectory
        // has no error/usage nodes and at least user+tool_call.
        let events = read_traj_events(&dir, "s1");
        assert!(
            traj_kinds(&events).starts_with(&["user".to_string(), "tool_call".to_string()]),
            "cancelled run must at least have user+tool_call: {events:?}"
        );
        assert!(
            !traj_kinds(&events).contains(&"error".to_string()),
            "cancelled run must not have error node: {events:?}"
        );
        assert!(
            !traj_kinds(&events).contains(&"usage".to_string()),
            "cancelled run must not have usage node: {events:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
        return;
    }
    let events = read_traj_events(&dir, "s1");
    // Strict expectation when -32800 wins: only user and tool_call, no tails.
    // Allow an extra tool_result with cancelled content if permission path raced,
    // but never error/usage.
    let kinds = traj_kinds(&events);
    assert!(
        kinds == vec!["user", "tool_call"] || kinds == vec!["user", "tool_call", "tool_result"],
        "tool_call was emitted at request-send (pre-Ask); the cancel discards pending thinking and records NO error/usage/permission tail: {events:?}"
    );
    if kinds.len() == 3 {
        let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } =
            &events[2].value
        else {
            panic!("third node must be tool_result: {events:?}");
        };
        assert!(
            content.contains("cancelled"),
            "extra tool_result must be cancelled deny: {content}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// `session/run` streaming, its frame helpers and cancellation live in
/// `handlers/run.rs`.
#[test]
fn run_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_session_run",
        "record_trajectory",
        "flush_thinking",
        "is_visually_blank",
        "build_run_reply",
        "dispatch_cancel_request",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/run.rs").contains(&needle),
            "{name} missing from run.rs"
        );
    }
}
