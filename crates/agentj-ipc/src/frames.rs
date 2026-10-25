//! `$/progress` frame builders and artifact wire-shape mappers.
//!
//! Verbatim move out of `lib.rs`: the JSON frame layout, the TODO enum mapping
//! and the git-status attachment (including its all-null degradation) are
//! unchanged; only visibility widened to `pub(crate)` for the handler modules.

use crate::file_list;
use crate::{sanitize_run_id, trajectory};
use agentj_core::artifacts::{
    FileChangeArtifact, FileChangeOp, TodoItemArtifact, TodoStatusArtifact,
};
use agentj_protocol::file::{FileKind, FileListResult};
use serde_json::Value;

pub(crate) fn build_progress_frame(
    token: &Value,
    value: &agentj_protocol::streaming::ProgressValue,
) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": { "token": token.clone(), "value": serde_json::to_value(value).expect("serializable") },
    })
    .to_string()
}

/// Map core TODO items to wire items (design contract item 2), enums 1:1.
pub(crate) fn todo_items_into_wire(
    items: Vec<TodoItemArtifact>,
) -> Vec<agentj_protocol::streaming::TodoItem> {
    items
        .into_iter()
        .map(|item| agentj_protocol::streaming::TodoItem {
            id: item.id,
            content: item.content,
            status: todo_status_into_wire(item.status),
        })
        .collect()
}

/// 1:1 TODO status mapping — the wire `Unknown` tolerance is never produced
/// by the kernel (artifacts only exist for known statuses).
fn todo_status_into_wire(status: TodoStatusArtifact) -> agentj_protocol::streaming::TodoStatus {
    match status {
        TodoStatusArtifact::Pending => agentj_protocol::streaming::TodoStatus::Pending,
        TodoStatusArtifact::InProgress => agentj_protocol::streaming::TodoStatus::InProgress,
        TodoStatusArtifact::Completed => agentj_protocol::streaming::TodoStatus::Completed,
        TodoStatusArtifact::Cancelled => agentj_protocol::streaming::TodoStatus::Cancelled,
    }
}

/// Attach `git_status` to file entries from one snapshot of `root`; a non-git
/// root or git failure degrades to all-null (design error handling).
pub(crate) async fn attach_git_statuses(
    result: FileListResult,
    root: &std::path::Path,
) -> FileListResult {
    let snapshot = agentj_core::git_sample::sample(root).await;
    if snapshot.is_none() {
        tracing::debug!(root = %root.display(), "file/list: git sample unavailable");
    }
    let entries = result
        .entries
        .into_iter()
        .map(|mut entry| {
            match entry.kind {
                FileKind::File => {
                    entry.git_status = snapshot
                        .as_ref()
                        .and_then(|s| file_list::git_statuses(s, &entry.path))
                        .and_then(|(x, y)| file_list::map_git_status(x, y));
                }
                FileKind::Dir => {
                    entry.git_status = snapshot
                        .as_ref()
                        .and_then(|s| file_list::aggregate_dir_status(&entry.path, &s.statuses));
                }
                FileKind::Unknown => {}
            }
            entry
        })
        .collect();
    FileListResult {
        entries,
        truncated: result.truncated,
        total: result.total,
    }
}

/// Map a core file-change artifact to the wire shape (design contract item 3).
/// `diff_truncated` is `Some(true)` only when truncated; `false` stays omitted
/// per the field's `skip_serializing_if`.
pub(crate) fn file_change_into_wire(
    change: FileChangeArtifact,
) -> agentj_protocol::streaming::FileChange {
    agentj_protocol::streaming::FileChange {
        path: change.path,
        op: file_change_op_into_wire(change.op),
        additions: change.additions,
        deletions: change.deletions,
        tool_call_id: change.tool_call_id,
        diff: change.diff,
        diff_truncated: change.diff_truncated.then_some(true),
        run_id: sanitize_run_id(change.run_id),
        seq: change.seq,
        source: change.source,
        total_bytes: change.total_bytes,
    }
}

/// 1:1 file-change op mapping (artifact ops are a closed set).
fn file_change_op_into_wire(op: FileChangeOp) -> agentj_protocol::streaming::FileChangeKind {
    match op {
        FileChangeOp::Add => agentj_protocol::streaming::FileChangeKind::Add,
        FileChangeOp::Modify => agentj_protocol::streaming::FileChangeKind::Modify,
        FileChangeOp::Delete => agentj_protocol::streaming::FileChangeKind::Delete,
    }
}

pub(crate) fn history_events_filtered(
    store: &trajectory::TrajectoryStore,
    sid: &str,
    run_id: Option<&str>,
    cursor: Option<u64>,
    limit: usize,
) -> std::io::Result<(
    Vec<agentj_protocol::trajectory::TrajectoryEvent>,
    Option<u64>,
)> {
    let (all, _) = store.read_paged(&sid.to_string(), None, usize::MAX)?;
    let filtered: Vec<_> = if let Some(rid) = run_id {
        all.into_iter()
            .filter(|ev| match &ev.value {
                agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. }
                | agentj_protocol::trajectory::TrajectoryValue::FileChange { run_id, .. } => {
                    run_id.as_deref() == Some(rid)
                }
                _ => true,
            })
            .collect()
    } else {
        all
    };
    let mut out = Vec::new();
    let mut seen: Option<u64> = None;
    for ev in filtered {
        if ev.seq < cursor.unwrap_or(0) {
            continue;
        }
        seen = Some(ev.seq);
        out.push(ev);
        if out.len() == limit {
            break;
        }
    }
    let nxt = if out.len() == limit {
        seen.map(|s| s + 1)
    } else {
        None
    };
    Ok((out, nxt))
}
