//! `frames` area tests.

use super::*;
use crate::test_support::stub_bus_server;
use crate::*;
use std::sync::Arc;

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

/// The `$/progress` frame builders, the artifact wire mappers and
/// `attach_git_statuses` live in `frames.rs`.
#[test]
fn frame_builders_moved_out_of_lib() {
    for name in [
        "build_progress_frame",
        "todo_items_into_wire",
        "attach_git_statuses",
        "file_change_into_wire",
        "history_events_filtered",
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
