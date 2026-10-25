//! `lifecycle` area tests.

use super::*;
use crate::test_support::{MockConfig, stub_bus_server, stub_session};
use crate::*;
use agentj_core::DefaultContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::*;
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_session::InMemoryStore;
use std::sync::Arc;

/// BUG-005: `kernel/status` must be dispatched by the IPC server — the
/// dashboard polls it on load, and -32601 made the panel show "kernel
/// unreachable". Wire shape per the KernelStatusResult schema: `version`
/// string + `modules` array.
#[tokio::test]
async fn kernel_status_returns_version_and_modules_array() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":5,"method":"kernel/status"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"],
        serde_json::Value::Null,
        "kernel/status must not error: {resp}"
    );
    assert!(
        v["result"]["version"].is_string(),
        "kernel/status must return a version string: {resp}"
    );
    assert_eq!(
        v["result"]["version"],
        env!("CARGO_PKG_VERSION"),
        "version must reflect the daemon build"
    );
    assert!(
        v["result"]["modules"].is_array(),
        "kernel/status must return a modules array: {resp}"
    );
    assert_eq!(v["id"], 5, "response must echo request id");
}

#[tokio::test]
async fn kernel_status_reports_registered_modules() {
    let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = session.clone();
    let tools = Arc::new(ToolRegistry::new());
    let providers = Arc::new(ProviderRegistry::new());
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
        Arc::new(DefaultContextPipeline::new()),
        "sys".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    let server = IpcServer::new(kernel, session_dyn);
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":6,"method":"kernel/status"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["result"]["modules"].is_array());
    let modules = v["result"]["modules"].as_array().unwrap();
    assert!(!modules.is_empty(), "modules must be non-empty");
    for m in modules {
        assert!(m["id"].is_string());
        assert_eq!(m["state"], "ready");
    }
}

#[tokio::test]
async fn shutdown_runs_kernel_graceful_shutdown() {
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "ok");

    // the kernel loop must actually terminate (graceful shutdown ran)
    server.await_shutdown().await.unwrap();
}

#[test]
fn registry_routes_broadcast_to_subscribers_only() {
    let mut reg = ConnectionRegistry::new();
    let (conn_a, log_a) = fake_conn("bcast-a");
    let (conn_b, log_b) = fake_conn("bcast-b");
    let a = reg.register(Box::new(conn_a));
    let _b = reg.register(Box::new(conn_b));
    reg.subscribe(&"s1".to_string(), &a);
    reg.broadcast_to_session(&"s1".to_string(), "hello");
    assert_eq!(
        lock_or_recover(&log_a).len(),
        1,
        "subscriber a must receive the broadcast"
    );
    assert_eq!(
        lock_or_recover(&log_b).len(),
        0,
        "non-subscriber b must NOT receive it"
    );
}

#[test]
fn registry_unregister_releases_owner_and_subscriptions() {
    let mut reg = ConnectionRegistry::new();
    let (conn_a, log_a) = fake_conn("own-a");
    let a = reg.register(Box::new(conn_a));
    reg.set_session_owner(&"s1".to_string(), &a);
    reg.subscribe(&"s1".to_string(), &a);
    reg.unregister(&a);
    assert!(
        reg.session_owner("s1").is_none(),
        "owner released on disconnect"
    );
    // broadcast after unregister is a no-op (no panic, no delivery)
    reg.broadcast_to_session(&"s1".to_string(), "hello");
    assert_eq!(lock_or_recover(&log_a).len(), 0);
}

#[test]
fn registry_run_mutex_is_exclusive() {
    let mut reg = ConnectionRegistry::new();
    assert!(reg.try_acquire_run(&"s1".to_string()));
    assert!(
        !reg.try_acquire_run(&"s1".to_string()),
        "second run must be rejected (busy)"
    );
    reg.release_run(&"s1".to_string());
    assert!(reg.try_acquire_run(&"s1".to_string()));
}

#[tokio::test]
async fn initialize_returns_connection_id_and_multi_client() {
    let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
    let conn_id = register_fake_conn(&server);
    let resp = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#,
            &conn_id,
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["connection_id"], conn_id);
    assert_eq!(v["result"]["capabilities"]["multi_client"], true);
}

#[tokio::test]
async fn initialize_reports_protocol_version_not_product_version() {
    let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
    let conn_id = register_fake_conn(&server);
    let resp = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.13.0"}}"#,
            &conn_id,
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["result"]["protocol_version"],
        agentj_protocol::PROTOCOL_VERSION
    );
    assert_eq!(
        v["result"]["server_info"]["version"],
        env!("CARGO_PKG_VERSION")
    );
}

#[tokio::test]
async fn initialize_with_wrong_token_is_rejected() {
    let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
    server.set_token(Some("secret".into()));
    let conn_id = register_fake_conn(&server);
    let resp = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0","token":"wrong"}}"#,
            &conn_id,
        )
        .await
        .unwrap();
    assert!(
        resp.contains(r#""code":-32806"#),
        "wrong token must be NotAuthorized: {resp}"
    );
}

#[tokio::test]
async fn subscriber_receives_progress_broadcast() {
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
                r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
            &a,
        )
        .await
        .unwrap();
    let b_sent = drain_sent(&server, &b);
    assert!(
        b_sent
            .iter()
            .any(|m| m.contains("$/progress") && m.contains("hello")),
        "subscriber B must receive $/progress deltas: {b_sent:?}"
    );
}

#[tokio::test]
async fn serve_connection_two_clients_get_unique_connection_ids() {
    let server = Arc::new(IpcServer::new(Kernel::new(), stub_session()));
    let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
    let (conn_b, log_b, tx_b) = scripted_conn("tcp-B");
    let s = Arc::clone(&server);
    let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
    let s = Arc::clone(&server);
    let task_b = tokio::spawn(async move { s.serve_connection(Box::new(conn_b)).await });
    let init =
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#;
    tx_a.send(init.to_string()).await.unwrap();
    tx_b.send(init.to_string()).await.unwrap();
    drop(tx_a);
    drop(tx_b);
    task_a.await.unwrap().unwrap();
    task_b.await.unwrap().unwrap();
    let va: serde_json::Value = serde_json::from_str(&lock_or_recover(&log_a)[0]).unwrap();
    let vb: serde_json::Value = serde_json::from_str(&lock_or_recover(&log_b)[0]).unwrap();
    assert_eq!(va["result"]["connection_id"], "tcp-A");
    assert_eq!(vb["result"]["connection_id"], "tcp-B");
    assert_ne!(
        va["result"]["connection_id"], vb["result"]["connection_id"],
        "each TCP client must receive its own connection id"
    );
}

#[tokio::test]
async fn serve_connection_broadcasts_progress_to_real_connection() {
    let server = Arc::new(stub_bus_server());
    let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
    let s = Arc::clone(&server);
    let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
    tx_a.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    // Wait for the session/new response so the run dispatches against a real sid.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if !lock_or_recover(&log_a).is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("session/new response never arrived");
    let sid = extract_sid(&lock_or_recover(&log_a)[0]);
    tx_a
        .send(format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
        ))
        .await
        .unwrap();
    drop(tx_a); // EOF after the run so the serve loop returns
    task_a.await.unwrap().unwrap();
    let a_sent = lock_or_recover(&log_a).clone();
    assert!(
        a_sent
            .iter()
            .any(|m| m.contains("$/progress") && m.contains("hello")),
        "run initiator must receive $/progress through the registry: {a_sent:?}"
    );
    assert!(
        read_or_recover(&server.registry).conns.is_empty(),
        "disconnected connection must be unregistered"
    );
    assert!(
        read_or_recover(&server.registry).writers.is_empty(),
        "disconnected connection's writer handle must be unregistered"
    );
}

/// A run outlives a client disconnect (spec 8.11): when the owning
/// connection ends, ownership is dropped and every later approval in the
/// still-running turn would be denied for lack of a responder. The first
/// connection to re-attach to a session nobody owns must become its owner
/// again — without ever stealing a live owner's session.
#[tokio::test]
async fn subscribe_reclaims_ownership_of_an_unowned_session() {
    let (server, _module) = blocking_run_server();
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let owner = |conn: &str| {
        let reg = read_or_recover(&server.registry);
        reg.session_owner(&sid).cloned().as_deref() == Some(conn)
    };
    assert!(owner(&a), "session/new records the initiating connection");

    // The owner connection ends: ownership is dropped (design 决策 #5).
    server.cleanup_connection(&a);
    assert!(
        read_or_recover(&server.registry)
            .session_owner(&sid)
            .is_none(),
        "a disconnect drops ownership"
    );

    let subscribe = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
    );
    server.handle_request_from(&subscribe, &b).await.unwrap();
    assert!(owner(&b), "the re-attaching client owns the session again");

    let c = register_fake_conn(&server);
    let subscribe = subscribe.replace("\"id\":2", "\"id\":3");
    server.handle_request_from(&subscribe, &c).await.unwrap();
    assert!(
        owner(&b),
        "a subscription must never steal a live owner's session"
    );
}

/// BUG-006 regression: an idle subscriber (session/subscribe then no
/// further requests, blocked in recv) must still receive $/progress while
/// another client runs the session. The BUG-001-era detach window removed
/// connections from the registry during recv, so broadcasts were dropped.
#[tokio::test]
async fn serve_connection_broadcasts_progress_to_idle_subscriber() {
    let server = Arc::new(stub_bus_server());
    let (conn_a, log_a, tx_a) = scripted_conn("tcp-A");
    let (conn_b, _log_b, tx_b) = scripted_conn("tcp-B");
    let s = Arc::clone(&server);
    let task_a = tokio::spawn(async move { s.serve_connection(Box::new(conn_a)).await });
    let s = Arc::clone(&server);
    let task_b = tokio::spawn(async move { s.serve_connection(Box::new(conn_b)).await });

    // Subscriber A: initialize → session/new → session/subscribe, then idle.
    tx_a.send(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#
            .to_string(),
    )
    .await
    .unwrap();
    tx_a.send(r#"{"jsonrpc":"2.0","id":2,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let sid = extract_sid(&wait_for_line(&log_a, |m| m.contains("\"session_id\"")).await);
    tx_a.send(format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
    ))
    .await
    .unwrap();
    wait_for_line(&log_a, |m| m.contains("\"id\":3")).await;
    // Let A's serve loop settle into recv (detached from the registry in
    // the buggy build) before the initiator runs the session.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Initiator B: initialize → session/run on the same session.
    tx_b.send(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.4.0"}}"#
            .to_string(),
    )
    .await
    .unwrap();
    tx_b.send(format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    ))
    .await
    .unwrap();
    drop(tx_b); // EOF so B's serve loop returns after the run completes
    task_b.await.unwrap().unwrap();

    let a_sent = lock_or_recover(&log_a).clone();
    assert!(
        a_sent
            .iter()
            .any(|m| m.contains("$/progress") && m.contains("hello")),
        "idle subscriber must receive $/progress while blocked in recv: {a_sent:?}"
    );

    drop(tx_a);
    let _ = task_a.await;
}

#[test]
fn transport_kind_has_websocket_variant() {
    let ws = TransportKind::WebSocket;
    assert_ne!(ws, TransportKind::Tcp);
    assert_ne!(ws, TransportKind::Stdio);
    assert_ne!(ws, TransportKind::UnixSocket);
}

/// Lifecycle handlers (initialize / subscribe / unsubscribe / shutdown /
/// kernel status) live in `handlers/lifecycle.rs`.
#[test]
fn lifecycle_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_initialize",
        "dispatch_subscribe",
        "dispatch_unsubscribe",
        "dispatch_shutdown",
        "dispatch_kernel_status",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/lifecycle.rs").contains(&needle),
            "{name} missing from lifecycle.rs"
        );
    }
}
