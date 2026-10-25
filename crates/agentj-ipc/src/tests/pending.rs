//! `pending` area tests.

use super::*;
use crate::test_support::{MockConfig, stub_session};
use crate::*;
use agentj_core::DefaultContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::*;
use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::*;
use agentj_model::mock::MockModelProvider;
use agentj_session::InMemoryStore;
use std::sync::Arc;

fn permission_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    permission_server_with(std::time::Duration::from_millis(50))
}

#[tokio::test]
async fn permission_respond_owner_gate_is_per_session() {
    let (server, module) = permission_server();
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "conn-a");
    write_or_recover(&server.registry).set_session_owner(&"s2".to_string(), "conn-b");
    let ctx1 = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d1 = module.check(&ctx1);
    let PermissionDecision::Ask { request_id: rid1 } = d1 else {
        panic!("expected Ask")
    };
    let raw1 = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid1.0
    );
    let resp = server.handle_request_from(&raw1, "conn-b").await.unwrap();
    assert!(
        resp.contains("-32806"),
        "non-owner must get NotAuthorized: {resp}"
    );
    let resp = server.handle_request_from(&raw1, "conn-a").await.unwrap();
    assert!(resp.contains("\"result\""), "owner must resolve: {resp}");
    let ctx2 = PermissionContext {
        session_id: "s2".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl y"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d2 = module.check(&ctx2);
    let PermissionDecision::Ask { request_id: rid2 } = d2 else {
        panic!("expected Ask")
    };
    let raw2 = format!(
        r#"{{"jsonrpc":"2.0","id":8,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid2.0
    );
    let resp = server.handle_request_from(&raw2, "conn-b").await.unwrap();
    assert!(resp.contains("\"result\""), "s2 owner must resolve: {resp}");
}

#[tokio::test]
async fn permission_respond_resolves_pending_approval() {
    let (server, module) = permission_server();
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d = module.check(&ctx);
    if let PermissionDecision::Ask { request_id } = d {
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw = format!(
            concat!(
                r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","#,
                r#""params":{{"request_id":"{}","decision":"allow"}}}}"#,
            ),
            request_id.0
        );
        let resp = server.handle_request(&raw).await.unwrap();
        assert!(resp.contains("\"result\""));
        assert!(matches!(handle.await.unwrap(), PermissionDecision::Allow));
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn permission_respond_unknown_request_id_errors() {
    let (server, _module) = permission_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":8,"method":"permission/respond","#,
        r#""params":{"request_id":"nope","decision":"allow"}}"#,
    );
    let resp = server.handle_request(raw).await.unwrap();
    assert!(resp.contains("\"error\""));
    assert!(resp.contains("-32600") || resp.contains("InvalidRequest") || resp.contains("unknown"));
}

#[tokio::test]
async fn cancel_request_resolves_pending_to_deny() {
    let (server, _module) = permission_server();
    let module = server.permission_module.clone().unwrap();
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
        let raw =
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
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

/// BUG-004: only the session owner may answer an approval (design 决策 #5),
/// so once that connection is gone nobody ever can — the approval must be
/// settled at disconnect (`PermissionModule::cancel_session` documents
/// "session termination / $/cancelRequest / client disconnect"). Left
/// pending, the orphan run blocks for the whole approval timeout (60s
/// default) and the session stays busy for a reconnecting client.
#[tokio::test]
async fn owner_disconnect_denies_pending_approval() {
    let (server, _module) = permission_server_with(std::time::Duration::from_secs(30));
    let module = server.permission_module.clone().unwrap();
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "conn-a");
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask");
    };
    let module2 = module.clone();
    let handle = tokio::spawn(async move { module2.await_approval(&request_id).await });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    server.cleanup_connection("conn-a");
    let decision = tokio::time::timeout(std::time::Duration::from_millis(500), handle)
        .await
        .expect("owner disconnect must settle the pending approval")
        .unwrap();
    assert_eq!(
        decision,
        PermissionDecision::Deny {
            reason: "disconnected".into()
        }
    );
}

/// Fixture: an IpcServer wired to a fresh QuestionModule (notifier not yet attached).
fn question_server() -> (IpcServer, Arc<agentj_question::QuestionModule>) {
    let module = Arc::new(agentj_question::QuestionModule::new());
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    server.question_module = Some(module.clone());
    (server, module)
}

fn ask_params(sid: &str) -> agentj_protocol::question::AskQuestionParams {
    use agentj_protocol::question::AskOption;
    agentj_protocol::question::AskQuestionParams {
        question_id: String::new(), // ask_blocking generates the real id
        session_id: sid.to_string(),
        question: "Pick a color".into(),
        header: None,
        options: vec![
            AskOption {
                id: "red".into(),
                label: "Red".into(),
                hint: None,
            },
            AskOption {
                id: "blue".into(),
                label: "Blue".into(),
                hint: None,
            },
        ],
        allow_multiple: false,
        allow_other: true,
        timeout_s: None,
    }
}

fn question_id_of(msg: &str) -> String {
    serde_json::from_str::<serde_json::Value>(msg).unwrap()["params"]["question_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn respond_request(qid: &str, answers: &str) -> String {
    format!(
        concat!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"question/respond","#,
            r#""params":{{"question_id":"{}","answers":{}}}}}"#
        ),
        qid, answers
    )
}

/// Wait for the owner's `$/askQuestion` notification; return the generated id.
async fn wait_for_ask_question(server: &Arc<IpcServer>, owner: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(msg) = drain_sent(server, owner)
                .into_iter()
                .find(|m| m.contains("$/askQuestion"))
            {
                return question_id_of(&msg);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed out waiting for $/askQuestion")
}

#[tokio::test]
async fn question_respond_non_owner_rejected() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let respond = respond_request(&qid, r#"["red"]"#);
    let resp = server.handle_request_from(&respond, &b).await.unwrap();
    assert!(
        resp.contains("-32806") && resp.contains("NotAuthorized"),
        "non-owner must be rejected with -32806: {resp}"
    );
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(
        resp.contains("\"accepted\":true"),
        "owner must resolve: {resp}"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(
        outcome,
        agentj_question::QuestionOutcome::Answered {
            answers: vec!["red".into()]
        }
    );
}

#[tokio::test]
async fn question_respond_unknown_id_is_idempotent_false() {
    let (server, module) = question_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":4,"method":"question/respond","#,
        r#""params":{"question_id":"nope","answers":[]}}"#,
    );
    let resp = server.handle_request_from(raw, "conn-x").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["accepted"], false, "{resp}");
    assert!(
        v.get("error").is_none(),
        "unknown id must not error: {resp}"
    );
    assert_eq!(module.pending_count(), 0);
}

#[tokio::test]
async fn cancel_request_settles_pending_question() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    assert_eq!(module.pending_count(), 1);
    let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
    let _ = server.handle_request_from(raw, &a).await.unwrap();
    assert_eq!(
        module.pending_count(),
        0,
        "cancel must settle the pending question"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
}

#[tokio::test]
async fn notifier_targets_owner_connection() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let ask = drain_sent(&server, &a)
        .into_iter()
        .find(|m| m.contains("$/askQuestion"))
        .expect("owner must receive $/askQuestion");
    let v: serde_json::Value = serde_json::from_str(&ask).unwrap();
    assert_eq!(v["method"], "$/askQuestion");
    assert_eq!(v["params"]["question_id"], qid);
    assert_eq!(v["params"]["session_id"], "s1");
    assert_eq!(v["params"]["question"], "Pick a color");
    assert_eq!(v["params"]["options"].as_array().unwrap().len(), 2);
    let b_sent = drain_sent(&server, &b);
    assert!(
        !b_sent.iter().any(|m| m.contains("$/askQuestion")),
        "non-owner must NOT receive $/askQuestion: {b_sent:?}"
    );
    let _ = module.cancel_session("s1");
    assert_eq!(
        handle.join().unwrap().1,
        agentj_question::QuestionOutcome::Skipped
    );
}

#[tokio::test]
async fn question_respond_maps_validation_errors_to_error_data() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let unknown = respond_request(&qid, r#"["purple"]"#);
    let resp = server.handle_request_from(&unknown, &a).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "{resp}");
    assert_eq!(v["error"]["data"]["unknown_options"][0], "purple", "{resp}");
    let too_many = respond_request(&qid, r#"["red","blue"]"#);
    let resp = server.handle_request_from(&too_many, &a).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "{resp}");
    assert_eq!(v["error"]["data"]["reason"], "too_many_answers", "{resp}");
    assert_eq!(
        module.pending_count(),
        1,
        "rejected validation must leave the question pending"
    );
    let _ = module.cancel_session("s1");
    handle.join().unwrap();
}

#[tokio::test]
async fn owner_disconnect_settles_pending_question_skipped() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let (conn, log, tx) = scripted_conn("tcp-owner");
    let s = Arc::clone(&server);
    let task = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });
    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |m| m.contains("\"session_id\"")).await;
    let sid = extract_sid(&new_resp);
    let m = module.clone();
    let params = ask_params(&sid);
    let handle = std::thread::spawn(move || m.ask_blocking(&sid, &params));
    let ask_line = wait_for_line(&log, |m| m.contains("$/askQuestion")).await;
    let qid = question_id_of(&ask_line);
    assert_eq!(module.pending_count(), 1);
    drop(tx); // EOF -> serve_connection unregisters and settles owned sessions
    task.await.unwrap().unwrap();
    assert_eq!(
        module.pending_count(),
        0,
        "disconnect must settle pending questions"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
}

#[tokio::test]
async fn question_settle_records_ask_and_answer_trajectory_once() {
    let dir = temp_dir_traj("question");
    let (mut server, module) = question_server();
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let respond = respond_request(&qid, r#"["red"]"#);
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(resp.contains("\"accepted\":true"), "{resp}");
    assert_eq!(
        handle.join().unwrap().1,
        agentj_question::QuestionOutcome::Answered {
            answers: vec!["red".into()]
        }
    );
    // Duplicate respond is idempotent and must NOT record a second answer.
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(resp.contains("\"accepted\":false"), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["ask_question", "question_answer"]);
    std::fs::remove_dir_all(&dir).ok();
}

/// Register a pending Ask approval for `sid` and await it in a task.
fn spawn_pending_approval(
    module: &Arc<agentj_permission::PermissionModule>,
    sid: &str,
) -> tokio::task::JoinHandle<PermissionDecision> {
    let ctx = PermissionContext {
        session_id: sid.to_string(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
        let module2 = module.clone();
        let rid = request_id.clone();
        tokio::spawn(async move { module2.await_approval(&rid).await })
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn session_run_records_inflight_session_for_cancel() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&new_resp).unwrap();
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move { server.handle_request(&raw).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("session/run must record the in-flight session id");
    let pending = spawn_pending_approval(&module, &sid);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let cancel = format!(
        r#"{{"jsonrpc":"2.0","method":"$/cancelRequest","params":{{"id":5,"session_id":"{sid}"}}}}"#
    );
    let _ = server.handle_request(&cancel).await.unwrap();
    assert_eq!(
        pending.await.unwrap(),
        PermissionDecision::Deny {
            reason: "cancelled".into()
        }
    );
    let run_resp = run_task.await.unwrap();
    assert!(
        run_resp.contains("\"result\"") || run_resp.contains("-32800"),
        "cancel should either complete or return RequestCancelled: {run_resp}"
    );
    assert!(
        !read_or_recover(&server.registry)
            .pending_runs
            .contains(&sid),
        "pending run cleared when the run finished"
    );
}

#[tokio::test]
async fn cancel_returns_minus_32800_when_tool_never_replies() {
    use agentj_core::module::{KernelModule, Module, ModuleMetadata};
    struct BlockingTool;
    impl Module for BlockingTool {
        fn id(&self) -> String {
            "tool_executor".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "tool_executor".into(),
                name: "blocking".into(),
                version: "0.1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(&mut self, _: &KernelContext, _: &mut ModuleRegister) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl KernelModule for BlockingTool {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<agentj_core::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg
                && let Some(tx) = lock_or_recover(reply).take()
            {
                std::mem::forget(tx);
            }
            Ok(agentj_core::module::KernelResponse::Handled)
        }
    }
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register(
        "mock".into(),
        Arc::new(MockModelProvider::new(vec![Ok(ModelEvent::ToolCall(
            ToolCall {
                id: "c1".into(),
                name: "dummy".into(),
                arguments: serde_json::json!({}),
            },
        ))])),
    );
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
        .register_module(Box::new(BlockingTool), &ctx, &mut register)
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(DefaultContextPipeline::new()),
        "sys".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    let server = Arc::new(IpcServer::new(kernel, session_dyn));
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        async move { s.handle_request(&run_raw).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = server
        .handle_request(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#)
        .await
        .unwrap();
    let resp = run_task.await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32800,
        "cancel must return -32800: {resp}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("RequestCancelled")
    );
}

#[tokio::test]
async fn non_owner_permission_respond_is_rejected() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(move |p: &PermissionRequestParams, _sid: &str| {
        let _ = tx.send(p.request_id.clone());
        true // handed to the test's observer (the non-owner respond below must still be rejected)
    }));
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
        .expect("run must record in-flight session");
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("permission request must arrive");
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
    );
    let resp = server.handle_request_from(&raw, &b).await.unwrap();
    assert!(
        resp.contains(r#""code":-32806"#),
        "non-owner respond must be NotAuthorized: {resp}"
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
async fn permission_request_is_routed_to_owner_only() {
    let (server, _module) = blocking_run_server();
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
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
        .expect("run must be in flight");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let a_sent = drain_sent(&server, &a);
    let b_sent = drain_sent(&server, &b);
    assert!(
        a_sent
            .iter()
            .any(|m| m.contains("$/permissionRequest") && m.contains("\"origin_client\":\"")),
        "owner A must receive $/permissionRequest with origin_client: {a_sent:?}"
    );
    assert!(
        !b_sent.iter().any(|m| m.contains("$/permissionRequest")),
        "non-owner B must NOT receive $/permissionRequest: {b_sent:?}"
    );
    let _ = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":9,"method":"$/cancelRequest","params":{"id":5}}"#,
            &a,
        )
        .await
        .unwrap();
    let _ = run_task.await;
}

/// BUG-004 (backend half): the owner is the only connection allowed to answer an
/// approval (设计决策 #5), and it is the in-flight `session/run` that waits on it.
/// So the approval must be settled the moment the connection ends — not after
/// `serve_connection_requests` drains that very run, which would leave an
/// orphaned run blocked until the approval timeout.
#[tokio::test]
async fn serve_connection_end_of_input_denies_pending_approval() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let (conn, log, tx) = scripted_conn("tcp-A");
    let s = Arc::clone(&server);
    let serving = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });

    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |l| l.contains("session_id")).await;
    let sid = extract_sid(&new_resp);
    tx.send(format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    ))
    .await
    .unwrap();
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in flight");
    let approval = wait_for_line(&log, |l| l.contains("$/permissionRequest")).await;
    let rid = request_id(&approval);

    drop(tx); // EOF — the client goes away while the run waits for approval

    // 1) the approval is settled by the disconnect, not by the 10s timeout
    let settled = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while module.view(&rid).is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        settled.is_ok(),
        "owner disconnect must settle the pending approval at once"
    );
    // 2) and the orphaned run is released, so a reconnect can run again
    let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while read_or_recover(&server.registry)
            .pending_runs
            .contains(&sid)
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        released.is_ok(),
        "the run must be released at disconnect, not by the approval timeout"
    );
    let finished = tokio::time::timeout(std::time::Duration::from_secs(5), serving).await;
    assert!(
        finished.is_ok(),
        "serve_connection must return once the orphaned run is released"
    );
    finished.unwrap().unwrap().unwrap();
    let log_dump = lock_or_recover(&log).clone();
    assert!(
        log_dump.iter().any(|l| l.contains("\"id\":2")),
        "the released run must still answer its caller: {log_dump:?}"
    );
}

/// BUG-004 (backend half): the disconnect must also cover approvals raised *while*
/// the run is being drained. The mock script emits two `bash` calls in one model
/// round, so once the first approval is settled at EOF the run immediately asks for
/// a second one — with no owner left it must be denied, not routed to the dead
/// connection (which would stall the run for the whole approval timeout and keep
/// the session busy after a reconnect).
#[tokio::test]
async fn approvals_during_drain_are_denied_at_once() {
    let (server, module) = blocking_run_server_with(2);
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let (conn, log, tx) = scripted_conn("tcp-A");
    let s = Arc::clone(&server);
    let serving = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });

    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |l| l.contains("session_id")).await;
    let sid = extract_sid(&new_resp);
    tx.send(format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    ))
    .await
    .unwrap();
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in flight");
    let approval = wait_for_line(&log, |l| l.contains("$/permissionRequest")).await;
    let rid = request_id(&approval);

    drop(tx); // EOF while the run waits on the first of the two approvals

    // The run can only answer `id:2` once it is done, i.e. once BOTH approvals are
    // settled — and `wait_for_line` gives up after 5s, far below the fixture's 10s
    // approval timeout, so a stall shows up as a failed wait.
    let done = wait_for_line(&log, |l| l.contains("\"id\":2")).await;
    assert!(
        module.view(&rid).is_none(),
        "first approval must be settled"
    );
    assert!(
        !done.contains("\"error\""),
        "the orphaned run must finish normally: {done}"
    );
    let released = tokio::time::timeout(std::time::Duration::from_secs(5), serving).await;
    assert!(released.is_ok(), "serve_connection must return");
    released.unwrap().unwrap().unwrap();
}

/// BUG-004 (backend half): an approval that cannot reach its owner can never be
/// answered, so it must be denied at once instead of stalling the run for the
/// whole approval timeout.
#[tokio::test]
async fn approval_without_reachable_owner_is_denied_immediately() {
    let (server, module) = permission_server_with(std::time::Duration::from_secs(30));
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask");
    };
    let decision = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        module.await_approval(&request_id),
    )
    .await
    .expect("no owner must be settled at once, not after the 30s timeout");
    // A deny is the only possible outcome — and it must not be the timeout one.
    // (`register_ask` invokes the notifier synchronously, so the request is
    // already consumed by the time `await_approval` looks it up: whatever the
    // settling caller, that lookup reports a plain deny rather than a stall.)
    let PermissionDecision::Deny { reason } = &decision else {
        panic!("no reachable owner must deny at once, got {decision:?}");
    };
    assert_ne!(
        reason, "timeout",
        "no reachable owner must deny at once, not time out"
    );
    // The owner-less settlement is the disconnect path, NOT the permission
    // module's "nobody to ask" path: the IPC notifier reports delivery = true
    // and settles this session itself (design decision P4).
    assert_ne!(
        reason,
        agentj_permission::NO_APPROVER_REASON,
        "the IPC notifier settles via cancel_session(DISCONNECTED_REASON), not 'no approver'"
    );
    assert!(
        module.view(&request_id).is_none(),
        "the approval must be settled, not left pending"
    );
}

#[tokio::test]
async fn respond_allow_records_permission_node_between_call_and_result() {
    let dir = temp_dir_traj("settle");
    let (mut server, module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(
        move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
            let _ = ask_tx.send(p.request_id.clone());
            true // handed to the test's observer, which answers via permission/respond
        },
    ));
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                .await
                .unwrap()
        }
    });
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = ask_rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the run's bash execution must register an Ask");
    let respond = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
    );
    let resp = server.handle_request(&respond).await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "owner respond accepted: {resp}"
    );
    let resp = run.await.unwrap();
    assert!(resp.contains("\"result\""), "run completed: {resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "permission",
            "tool_result",
            "run_model"
        ]
    );
    let agentj_protocol::trajectory::TrajectoryValue::Permission {
        request_id,
        tool,
        decision,
    } = &events[2].value
    else {
        panic!("permission node at index 2: {events:?}")
    };
    assert_eq!(request_id, &rid);
    assert_eq!(tool, "bash");
    assert_eq!(
        *decision,
        agentj_protocol::permission::PermissionDecision::Allow
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn respond_deny_records_deny_and_run_still_completes() {
    let dir = temp_dir_traj("deny");
    let (mut server, module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(
        move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
            let _ = ask_tx.send(p.request_id.clone());
            true // handed to the test's observer, which answers via permission/respond
        },
    ));
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                .await
                .unwrap()
        }
    });
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = ask_rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the run's bash execution must register an Ask");
    let respond = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"deny"}}}}"#
    );
    let resp = server.handle_request(&respond).await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "owner respond accepted: {resp}"
    );
    let resp = run.await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "denied tool is a content error, not a run error: {resp}"
    );
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "permission",
            "tool_result",
            "run_model"
        ]
    );
    let agentj_protocol::trajectory::TrajectoryValue::Permission { decision, .. } =
        &events[2].value
    else {
        panic!("permission node at index 2: {events:?}")
    };
    assert_eq!(
        *decision,
        agentj_protocol::permission::PermissionDecision::Deny
    );
    let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } = &events[3].value
    else {
        panic!("tool_result at index 3: {events:?}")
    };
    assert!(
        content.starts_with("Error:"),
        "executor denial flows into tool_result content (C9): {content}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Pending-request handlers (`permission/respond`, `question/respond`,
/// owner-disconnect settlement and the question notifier hooks) live in
/// `handlers/pending.rs`.
#[test]
fn pending_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_permission_respond",
        "dispatch_question_respond",
        "cleanup_connection",
        "settle_owner_pending",
        "question_notifier",
        "question_settle_hook",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/pending.rs").contains(&needle),
            "{name} missing from pending.rs"
        );
    }
}
