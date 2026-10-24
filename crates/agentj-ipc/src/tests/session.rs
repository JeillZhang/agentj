//! `session` area tests.

use super::*;
use crate::test_support::{stub_bus_server_with_usage, stub_session};
use crate::*;
use agentj_core::kernel::Kernel;
use agentj_core::types::*;
use std::sync::Arc;

#[tokio::test]
async fn session_new_returns_valid_id() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["result"]["session_id"].is_string());
    assert!(!v["result"]["session_id"].as_str().unwrap().is_empty());
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(
        v["result"]["project_root"],
        cwd.to_str().unwrap(),
        "session/new without project_root must return the canonicalized cwd"
    );
}

/// Temp tree for `session/new` project_root tests (mirrors `temp_dir_traj`).
fn temp_dir_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-root-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// T20: `session/new {project_root}` stores the canonicalized directory on
/// the created session (design item 6).
#[tokio::test]
async fn session_new_with_project_root_persists_it() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let dir = temp_dir_root("persist");
    let canonical = dir.canonicalize().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session/new",
        "params": { "project_root": dir.to_str().unwrap() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "valid project_root must not error: {resp}"
    );
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    assert_eq!(
        v["result"]["project_root"],
        canonical.to_str().unwrap(),
        "session/new must echo the canonicalized request value"
    );
    assert_eq!(store.project_root(&sid), Some(canonical));
    std::fs::remove_dir_all(&dir).ok();
}

/// T20: a relative `project_root` is rejected with -32602 carrying
/// `error.data {path, reason}`; no session is created (validate before create).
#[tokio::test]
async fn session_new_rejects_relative_root_with_32602() {
    let dir = temp_dir_root("reject");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let server = IpcServer::new(Kernel::new(), store.clone());
    let raw =
        r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"project_root":"rel/path"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
    assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
    assert_eq!(v["error"]["data"]["path"], "rel/path");
    assert!(v["result"].is_null(), "no result on error: {resp}");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "invalid project_root must not create a session"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// T20 AC guard: a missing path and a regular file are also rejected with
/// -32602 + `error.data {path, reason}`.
#[tokio::test]
async fn session_new_rejects_missing_and_file_root_with_32602() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let dir = temp_dir_root("badshape");
    let missing = dir.join("missing");
    let file = dir.join("not-a-dir.txt");
    std::fs::write(&file, "x").unwrap();
    for bad in [missing.as_path(), file.as_path()] {
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/new",
            "params": { "project_root": bad.to_str().unwrap() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
        assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
        assert_eq!(v["error"]["data"]["path"], bad.to_str().unwrap());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_list_returns_file_backed_rows() {
    let dir = temp_dir_traj("list");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let session: Arc<dyn SessionStore> = store.clone();
    let server = IpcServer::new(Kernel::new(), session);
    let sid = store.create().unwrap();
    store
        .append(
            &sid,
            ChatMessage {
                role: Role::User,
                content: "x".repeat(150),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    let raw = r#"{"jsonrpc":"2.0","id":4,"method":"session/list"}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let row = &v["result"]["sessions"][0];
    assert_eq!(row["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(row["state"], "active");
    assert!(row["updated_at"].is_u64());
    assert_eq!(row["preview"].as_str().unwrap().len(), 120);
    assert!(row["preview"].as_str().unwrap().starts_with("xxxxx"));
    store.archive(&sid).unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":5,"method":"session/list"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["sessions"][0]["state"], "archived");
    std::fs::remove_dir_all(&dir).ok();
}

/// Regression guard: `session/status` is declared in the registry, schema,
/// SDK and spec (§8.8) and is the existence probe every client fires
/// before `session/subscribe`. It must be dispatched — falling through to
/// `-32601 method not found` breaks every session click in Web/TUI.
#[tokio::test]
async fn session_status_returns_store_status() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/status must be implemented, not -32601: {resp}"
    );
    assert_eq!(v["result"]["status"], "active");
    store.archive(&sid).unwrap();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "archived");
}

/// T14: `session/status` reports the effective project root — the stored
/// canonicalized root when present, else the canonicalized cwd fallback.
#[tokio::test]
async fn dispatch_session_status_reports_effective_project_root() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let dir = temp_dir_root("status-eff-root");
    let canonical = dir.canonicalize().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "session/new",
        "params": { "project_root": dir.to_str().unwrap() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "session/status",
        "params": { "session_id": sid },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["result"]["project_root"],
        canonical.to_str().unwrap(),
        "status must report the stored canonicalized root"
    );

    let bare = store.create().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "session/status",
        "params": { "session_id": bare.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(
        v["result"]["project_root"],
        cwd.to_str().unwrap(),
        "status without a stored root must fall back to the canonicalized cwd"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Unknown session id maps to the documented `-32802 SessionNotFound`
/// with `error.data.session_id` (§8.8), never the generic `-32601`.
#[tokio::test]
async fn session_status_unknown_session_returns_32802() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let raw =
        r#"{"jsonrpc":"2.0","id":13,"method":"session/status","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32802,
        "unknown session must be SessionNotFound: {resp}"
    );
    assert_eq!(v["error"]["data"]["session_id"], "nope");
}

/// Regression guard: `session/archive` is declared in the registry/spec
/// §8.9 and exported by the TUI session hook; it must be dispatched.
/// Only active sessions may be archived — repeats are `-32803`.
#[tokio::test]
async fn session_archive_transitions_and_rejects_inactive() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let archive = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 14,
        "method": "session/archive",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&archive).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/archive must be implemented, not -32601: {resp}"
    );
    assert!(v["result"].is_null(), "archive result is null: {resp}");
    let status = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 15,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&status).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "archived");
    let resp = server.handle_request(&archive).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32803,
        "repeat archive must be SessionInactive: {resp}"
    );
    assert_eq!(v["error"]["data"]["status"], "archived");
    let unknown =
        r#"{"jsonrpc":"2.0","id":16,"method":"session/archive","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(unknown).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32802, "unknown archive: {resp}");
}

/// Regression guard: `session/delete` (§8.10) must be dispatched;
/// deleting a deleted session is `-32803 SessionInactive`.
#[tokio::test]
async fn session_delete_transitions_and_rejects_deleted() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let delete = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 17,
        "method": "session/delete",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&delete).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/delete must be implemented, not -32601: {resp}"
    );
    assert!(v["result"].is_null(), "delete result is null: {resp}");
    let status = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 18,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&status).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "deleted");
    let resp = server.handle_request(&delete).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32803,
        "repeat delete must be SessionInactive: {resp}"
    );
    assert_eq!(v["error"]["data"]["status"], "deleted");
    let unknown =
        r#"{"jsonrpc":"2.0","id":19,"method":"session/delete","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(unknown).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32802, "unknown delete: {resp}");
}

#[tokio::test]
async fn session_history_pages_recorded_events() {
    let dir = temp_dir_traj("history");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let raw = r#"{"jsonrpc":"2.0","id":6,"method":"session/history","params":{"session_id":"s1","limit":2}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let page1 = &v["result"];
    assert_eq!(page1["events"].as_array().unwrap().len(), 2);
    assert_eq!(page1["events"][0]["seq"], 1);
    assert_eq!(page1["events"][0]["value"]["kind"], "user");
    assert_eq!(page1["events"][1]["value"]["kind"], "thinking");
    assert_eq!(page1["next_cursor"], 3);
    let raw2 = r#"{"jsonrpc":"2.0","id":7,"method":"session/history","params":{"session_id":"s1","cursor":3,"limit":3}}"#;
    let resp2 = server.handle_request(raw2).await.unwrap();
    let v2: serde_json::Value = serde_json::from_str(&resp2).unwrap();
    assert_eq!(v2["result"]["events"][0]["value"]["kind"], "usage");
    assert_eq!(v2["result"]["events"][1]["value"]["kind"], "run_model");
    assert!(v2["result"].get("next_cursor").is_none(), "no next page");
    std::fs::remove_dir_all(&dir).ok();
}

/// Dispatch one `session/run` and return its `SessionRunResult.run_id`.
async fn run_and_run_id(server: &IpcServer, sid: &str, content: &str, id: u64) -> String {
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"session/run","params":{{"session_id":"{sid}","content":"{content}"}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    v["result"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("run must report a run_id: {resp}"))
        .to_string()
}

/// Mirror of the T5 run filter: Todo/FileChange keep only `run_id`, all
/// other trajectory kinds pass.
fn event_kept_for_run(value: &agentj_protocol::trajectory::TrajectoryValue, run_id: &str) -> bool {
    match value {
        agentj_protocol::trajectory::TrajectoryValue::Todo { run_id: r, .. }
        | agentj_protocol::trajectory::TrajectoryValue::FileChange { run_id: r, .. } => {
            r.as_deref() == Some(run_id)
        }
        _ => true,
    }
}

/// T5 AC: `session/history {run_id}` keeps that run's Todo/FileChange, lets
/// every other kind through, and filters before paginating.
#[tokio::test]
async fn history_filters_by_run_id() {
    let dir = temp_dir_traj("history-run-filter");
    let target = std::env::temp_dir().join(format!("aj-ipc-runfilter-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let script = vec![
        agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
            id: "call_todo".into(),
            name: "todo".into(),
            arguments: serde_json::json!({"items": [
                {"content": "step", "status": "in_progress"}
            ]}),
        }),
        agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
            id: "call_write".into(),
            name: "write".into(),
            arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
        }),
        agentj_core::types::ModelEvent::Done,
    ];
    let mut server = two_run_server(script, |tools| {
        tools.register(Arc::new(TodoMock));
        tools.register(Arc::new(WriteMock));
    });
    server.trajectory_store = Some(wired_store(&dir));
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run1 = run_and_run_id(&server, &sid, "first-turn", 2).await;
    std::fs::remove_file(&target).ok();
    let run2 = run_and_run_id(&server, &sid, "second-turn", 3).await;
    assert_ne!(run1, run2, "each run gets a fresh UUID");

    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","limit":500}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "r1 filter accepted: {resp}");
    let events = v["result"]["events"].as_array().unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|e| e["value"]["kind"].as_str())
        .collect();
    for e in events {
        let kind = e["value"]["kind"].as_str().unwrap();
        if kind == "todo" || kind == "file_change" {
            assert_eq!(
                e["value"]["run_id"].as_str(),
                Some(run1.as_str()),
                "artifacts stay r1-scoped: {e}"
            );
        }
    }
    assert_eq!(kinds.iter().filter(|k| **k == "todo").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "file_change").count(), 1);
    assert_eq!(
        kinds.iter().filter(|k| **k == "user").count(),
        2,
        "non-artifact kinds from both runs pass through"
    );
    assert!(kinds.contains(&"tool_call"));
    assert!(v["result"].get("next_cursor").is_none(), "single page");

    // Filter first, then paginate: a cursor at r2's Todo must skip that
    // node instead of returning it as the first page item.
    let all = read_traj_events(&dir, &sid);
    let r2_todo_seq = all
        .iter()
        .find(|e| {
            matches!(
                &e.value,
                agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. }
                    if run_id.as_deref() == Some(run2.as_str())
            )
        })
        .expect("r2 todo recorded")
        .seq;
    let expected = all
        .iter()
        .find(|e| e.seq >= r2_todo_seq && event_kept_for_run(&e.value, &run1))
        .expect("a later kept event exists");
    assert_ne!(expected.seq, r2_todo_seq, "the r2 todo is filtered out");
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","cursor":{r2_todo_seq},"limit":1}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let page = v["result"]["events"].as_array().unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0]["seq"], expected.seq);
    assert_eq!(v["result"]["next_cursor"], expected.seq + 1);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// T5 AC: `session/history` rejects a malformed `run_id` as `-32602` with
/// `data.reason == "invalid_run_id"`; a valid UUID is accepted.
#[tokio::test]
async fn history_invalid_run_id_rejects() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let long = "a".repeat(129);
    for bad in ["not-a-uuid", long.as_str()] {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"session/history","params":{{"session_id":"s1","run_id":"{bad}"}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "rejected: {resp}");
        assert_eq!(
            v["error"]["data"]["reason"], "invalid_run_id",
            "reason tag: {resp}"
        );
    }
    let valid = Uuid::new_v4().to_string();
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"session/history","params":{{"session_id":"s1","run_id":"{valid}"}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "valid UUID accepted: {resp}");
    assert_eq!(v["result"]["events"], serde_json::json!([]));
}

#[tokio::test]
async fn session_history_unknown_sid_and_no_store_return_empty() {
    let dir = temp_dir_traj("none");
    let server = stub_bus_server_with_usage();
    let raw =
        r#"{"jsonrpc":"2.0","id":8,"method":"session/history","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "unknown sid is not an error: {resp}");
    assert_eq!(v["result"]["events"], serde_json::json!([]));
    assert!(v["result"].get("next_cursor").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_list_mirrors_store_list_order() {
    let dir = temp_dir_traj("order");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let session: Arc<dyn SessionStore> = store.clone();
    let server = IpcServer::new(Kernel::new(), session);
    let a = store.create().unwrap();
    store
        .append(
            &a,
            ChatMessage {
                role: Role::User,
                content: "first".into(),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    let b = store.create().unwrap();
    store
        .append(
            &b,
            ChatMessage {
                role: Role::User,
                content: "second".into(),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    store.archive(&b).unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":9,"method":"session/list"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let rows = v["result"]["sessions"].as_array().expect("sessions array");
    assert_eq!(rows.len(), 2, "both sessions listed: {resp}");
    let entries = store.list().unwrap();
    for (i, e) in entries.iter().enumerate() {
        assert_eq!(rows[i]["session_id"].as_str(), Some(e.session_id.as_str()));
        let want_state = format!("{:?}", e.status).to_lowercase();
        assert_eq!(rows[i]["state"].as_str(), Some(want_state.as_str()));
        assert_eq!(rows[i]["updated_at"].as_u64(), Some(e.updated_at));
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_history_clamps_limit_into_range() {
    let dir = temp_dir_traj("clamp");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/history","params":{"session_id":"s1","limit":0}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(v["result"]["events"][0]["seq"], 1);
    assert_eq!(
        v["result"]["next_cursor"], 2,
        "window full -> cursor advances"
    );
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":11,"method":"session/history","params":{"session_id":"s1","limit":9999}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["events"].as_array().unwrap().len(), 4);
    assert!(
        v["result"].get("next_cursor").is_none(),
        "nothing left behind"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Session handlers (new / list / history / status / archive / delete and
/// the root + status helpers they own) live in `handlers/session.rs`.
#[test]
fn session_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_session_new",
        "resolve_project_root",
        "dispatch_session_list",
        "dispatch_session_history",
        "dispatch_session_status",
        "dispatch_session_archive",
        "dispatch_session_delete",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/session.rs").contains(&needle),
            "{name} missing from session.rs"
        );
    }
}
