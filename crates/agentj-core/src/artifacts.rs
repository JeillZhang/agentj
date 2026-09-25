//! Turn artifacts produced by tools: file changes (with unified diff) and todo snapshots.
//!
//! These are core-native types (not wire types); the IPC layer maps them to protocol
//! payloads. The diff implementation is hand-rolled with no third-party dependency:
//! a line-level LCS DP over a bounded work area, degrading to a multiset line
//! difference when the inputs are too large.

use std::collections::HashMap;

pub use agentj_protocol::streaming::FileChangeSource;

use serde_json::Value;

/// Maximum size in bytes of an embedded unified diff.
///
/// Larger diffs are dropped (`diff = None`) and flagged via `diff_truncated`.
pub const DIFF_MAX_BYTES: usize = 64 * 1024;

/// Maximum line product (`before_lines * after_lines`) allowed for the LCS DP table.
const LCS_MAX_LINE_PRODUCT: usize = 4 * 1024 * 1024;

/// Artifact emitted while a turn executes.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnArtifact {
    /// A file was created, modified, or deleted.
    FileChange(FileChangeArtifact),
    /// Full todo list snapshot submitted by the model.
    Todo(TodoSnapshot),
}

/// One file change with line counts and, when small enough, a unified diff.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChangeArtifact {
    /// Path relative to the session workspace root.
    pub path: String,
    /// Kind of change (`write` only produces `Add` / `Modify`).
    pub op: FileChangeOp,
    /// Number of added lines.
    pub additions: u64,
    /// Number of deleted lines.
    pub deletions: u64,
    /// Tool call that produced this change, when known.
    pub tool_call_id: Option<String>,
    /// Unified diff text, or `None` when unavailable or truncated.
    pub diff: Option<String>,
    /// True when the diff was dropped because inputs exceeded a cap.
    pub diff_truncated: bool,
    /// Run this change belongs to. @since 0.15.0
    pub run_id: Option<String>,
    /// Per-run monotonic sequence for ordering inside a run. @since 0.15.0
    pub seq: Option<u64>,
    /// Source of the change: write (LCS diff) or bash (numstat only). @since 0.15.0
    pub source: Option<FileChangeSource>,
    /// Original diff byte length when truncated. @since 0.15.0
    pub total_bytes: Option<u64>,
}

/// Kind of file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeOp {
    /// File created.
    Add,
    /// File content changed.
    Modify,
    /// File removed.
    Delete,
}

/// Full todo list snapshot (always a whole-list replacement).
#[derive(Debug, Clone, PartialEq)]
pub struct TodoSnapshot {
    /// Items in submission order.
    pub items: Vec<TodoItemArtifact>,
}

/// One todo entry with a stable, content-derived id.
#[derive(Debug, Clone, PartialEq)]
pub struct TodoItemArtifact {
    /// Stable id (`t<hash>`, duplicated content gets a `#n` suffix).
    pub id: String,
    /// Human-readable task text.
    pub content: String,
    /// Current lifecycle status.
    pub status: TodoStatusArtifact,
}

/// Todo lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatusArtifact {
    /// Not started.
    Pending,
    /// Currently being worked on.
    InProgress,
    /// Finished.
    Completed,
    /// Abandoned.
    Cancelled,
}

impl TodoStatusArtifact {
    /// Wire string for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse a wire status string; unknown values are an error.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("invalid todo status `{other}`")),
        }
    }
}

/// Compute a unified diff for `before`/`after` (line-level LCS).
///
/// Returns an empty string when the inputs are identical or when the LCS work
/// area is exceeded (callers that need counts use [`write_change`]).
pub fn unified_diff(before: &str, after: &str) -> String {
    diff_outcome(before, after).text.unwrap_or_default()
}

/// Build a [`FileChangeArtifact`] for a `write` tool call.
///
/// `before == None` means the file did not exist (op `Add`). Counts fall back to
/// a bounded approximation when the LCS work area is exceeded, and the diff is
/// dropped when it exceeds [`DIFF_MAX_BYTES`].
pub fn write_change(
    path: &str,
    before: Option<&str>,
    after: &str,
    tool_call_id: Option<&str>,
) -> FileChangeArtifact {
    let outcome = diff_outcome(before.unwrap_or(""), after);
    let op = if before.is_some() {
        FileChangeOp::Modify
    } else {
        FileChangeOp::Add
    };
    let tid = tool_call_id.map(str::to_string);
    if outcome.truncated {
        return FileChangeArtifact {
            path: path.to_string(),
            op,
            additions: outcome.additions,
            deletions: outcome.deletions,
            tool_call_id: tid,
            diff: None,
            diff_truncated: true,
            run_id: None,
            seq: None,
            source: Some(FileChangeSource::Write),
            total_bytes: None,
        };
    }
    let text = outcome.text.unwrap_or_default();
    if text.len() > DIFF_MAX_BYTES {
        let total = text.len() as u64;
        FileChangeArtifact {
            path: path.to_string(),
            op,
            additions: outcome.additions,
            deletions: outcome.deletions,
            tool_call_id: tid,
            diff: Some(text[..DIFF_MAX_BYTES].to_string()),
            diff_truncated: true,
            run_id: None,
            seq: None,
            source: Some(FileChangeSource::Write),
            total_bytes: Some(total),
        }
    } else {
        FileChangeArtifact {
            path: path.to_string(),
            op,
            additions: outcome.additions,
            deletions: outcome.deletions,
            tool_call_id: tid,
            diff: Some(text),
            diff_truncated: false,
            run_id: None,
            seq: None,
            source: Some(FileChangeSource::Write),
            total_bytes: None,
        }
    }
}

/// Build bash-sourced file changes from `(path, additions, deletions)` entries.
///
/// Each artifact has `source=Bash`, `diff=None`, `diff_truncated=false`.
pub fn bash_file_changes(
    entries: Vec<(&str, u64, u64)>,
    tool_call_id: Option<&str>,
) -> Vec<FileChangeArtifact> {
    entries
        .into_iter()
        .map(|(path, additions, deletions)| FileChangeArtifact {
            path: path.to_string(),
            op: FileChangeOp::Modify,
            additions,
            deletions,
            tool_call_id: tool_call_id.map(str::to_string),
            diff: None,
            diff_truncated: false,
            run_id: None,
            seq: None,
            source: Some(FileChangeSource::Bash),
            total_bytes: None,
        })
        .collect()
}

/// Parse and validate a `todo` tool call's arguments into a snapshot.
///
/// `args.items` must be an array of objects with non-empty string `content` and
/// an optional valid `status` (defaults to `pending`). An empty array is a legal
/// "clear" snapshot. Ids are stable FNV-1a hashes of the content with an `#n`
/// suffix for repeated content.
pub fn todo_snapshot_from_args(args: &Value) -> Result<TodoSnapshot, String> {
    let items = args
        .get("items")
        .ok_or_else(|| "todo args missing `items` array".to_string())?
        .as_array()
        .ok_or_else(|| "todo args `items` must be an array".to_string())?;
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut snapshot = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("todo item {index} must be an object"))?;
        let content = object
            .get("content")
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| format!("todo item {index} `content` must be a non-empty string"))?;
        let status = match object.get("status") {
            None | Some(Value::Null) => TodoStatusArtifact::Pending,
            Some(Value::String(raw)) => {
                TodoStatusArtifact::parse(raw).map_err(|err| format!("todo item {index}: {err}"))?
            }
            Some(other) => {
                return Err(format!(
                    "todo item {index} `status` must be a string, got {other}"
                ));
            }
        };
        let occurrence = seen.entry(content).or_insert(0);
        *occurrence += 1;
        snapshot.push(TodoItemArtifact {
            id: todo_id(content, *occurrence),
            content: content.to_string(),
            status,
        });
    }
    Ok(TodoSnapshot { items: snapshot })
}

struct DiffOutcome {
    additions: u64,
    deletions: u64,
    text: Option<String>,
    truncated: bool,
}

/// Diff two texts, degrading to multiset counts when the LCS cap is exceeded.
fn diff_outcome(before: &str, after: &str) -> DiffOutcome {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    if before_lines.len().saturating_mul(after_lines.len()) > LCS_MAX_LINE_PRODUCT {
        let (additions, deletions) = approximate_counts(&before_lines, &after_lines);
        return DiffOutcome {
            additions,
            deletions,
            text: None,
            truncated: true,
        };
    }
    let (text, additions, deletions) = lcs_diff(&before_lines, &after_lines);
    DiffOutcome {
        additions,
        deletions,
        text: Some(text),
        truncated: false,
    }
}

/// Line-level LCS diff with a whole-file hunk header; returns (text, +lines, -lines).
fn lcs_diff(before: &[&str], after: &[&str]) -> (String, u64, u64) {
    let width = after.len() + 1;
    let mut dp = vec![0u32; (before.len() + 1) * width];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            dp[i * width + j] = if before[i] == after[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }
    let mut body = String::new();
    let (mut i, mut j) = (0, 0);
    let (mut additions, mut deletions) = (0u64, 0u64);
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            push_line(&mut body, ' ', before[i]);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            push_line(&mut body, '-', before[i]);
            i += 1;
            deletions += 1;
        } else {
            push_line(&mut body, '+', after[j]);
            j += 1;
            additions += 1;
        }
    }
    while i < before.len() {
        push_line(&mut body, '-', before[i]);
        i += 1;
        deletions += 1;
    }
    while j < after.len() {
        push_line(&mut body, '+', after[j]);
        j += 1;
        additions += 1;
    }
    if additions == 0 && deletions == 0 {
        return (String::new(), 0, 0);
    }
    (
        format!("{}{body}", hunk_header(before.len(), after.len())),
        additions,
        deletions,
    )
}

fn push_line(out: &mut String, prefix: char, line: &str) {
    out.push(prefix);
    out.push_str(line);
    out.push('\n');
}

/// Whole-file hunk header; `0,0` on an empty side per unified diff convention.
fn hunk_header(before_len: usize, after_len: usize) -> String {
    let before_start = if before_len == 0 { 0 } else { 1 };
    let after_start = if after_len == 0 { 0 } else { 1 };
    format!("@@ -{before_start},{before_len} +{after_start},{after_len} @@\n")
}

/// Multiset line difference: lines in `after` not matched in `before` count as
/// additions, leftover `before` lines as deletions.
fn approximate_counts(before: &[&str], after: &[&str]) -> (u64, u64) {
    let mut pool: HashMap<&str, i64> = HashMap::new();
    for line in before {
        *pool.entry(line).or_insert(0) += 1;
    }
    let mut additions = 0u64;
    for line in after {
        let slot = pool.entry(line).or_insert(0);
        if *slot > 0 {
            *slot -= 1;
        } else {
            additions += 1;
        }
    }
    let deletions = pool
        .values()
        .filter(|count| **count > 0)
        .map(|count| *count as u64)
        .sum();
    (additions, deletions)
}

fn todo_id(content: &str, occurrence: u32) -> String {
    let base = format!("t{:016x}", fnv1a_64(content.as_bytes()));
    if occurrence <= 1 {
        base
    } else {
        format!("{base}#{occurrence}")
    }
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_change_add_counts_lines_and_diff() {
        let change = write_change("src/new.rs", None, "a\nb", Some("call-1"));
        assert_eq!(change.path, "src/new.rs");
        assert_eq!(change.op, FileChangeOp::Add);
        assert_eq!(change.additions, 2);
        assert_eq!(change.deletions, 0);
        assert_eq!(change.tool_call_id.as_deref(), Some("call-1"));
        assert!(!change.diff_truncated);
        let diff = change.diff.expect("small add keeps its diff");
        assert!(diff.contains("@@ -0,0 +1,2 @@"), "hunk header: {diff}");
        let content: Vec<&str> = diff
            .lines()
            .filter(|line| !line.starts_with("@@"))
            .collect();
        assert_eq!(content, vec!["+a", "+b"]);
    }

    #[test]
    fn write_change_modify_counts_lines_and_diff() {
        let change = write_change("f.txt", Some("a\nb\nc"), "a\nx\nc", None);
        assert_eq!(change.op, FileChangeOp::Modify);
        assert_eq!(change.additions, 1);
        assert_eq!(change.deletions, 1);
        assert_eq!(change.tool_call_id, None);
        assert!(!change.diff_truncated);
        let diff = change.diff.expect("small modify keeps its diff");
        assert!(diff.contains("-b"), "diff: {diff}");
        assert!(diff.contains("+x"), "diff: {diff}");
        assert!(diff.contains("@@ -1,3 +1,3 @@"), "diff: {diff}");
    }

    #[test]
    fn write_change_diff_truncates_over_cap() {
        let after: String = (0..4096)
            .map(|i| format!("line-{i:06}-xxxxxxxxxxxxxxxx\n"))
            .collect();
        assert!(after.len() > DIFF_MAX_BYTES, "fixture must exceed the cap");
        let change = write_change("big.txt", None, &after, Some("call-9"));
        assert_eq!(change.op, FileChangeOp::Add);
        assert_eq!(change.additions, 4096);
        assert_eq!(change.deletions, 0);
        assert!(change.diff_truncated, "over-cap must be flagged truncated");
        assert_eq!(change.source, Some(FileChangeSource::Write));
        // New semantics (0.15.0): over-cap keeps a 64 KiB prefix + total_bytes disclosure,
        // rather than dropping the diff entirely.
        let diff = change.diff.expect("over-cap must keep truncated prefix");
        assert!(diff.len() <= DIFF_MAX_BYTES, "prefix must fit cap");
        assert!(change.total_bytes.unwrap() > DIFF_MAX_BYTES as u64);
    }

    #[test]
    fn write_change_degrades_over_lcs_cap() {
        let before: String = (0..2100).map(|i| format!("l{i:05}\n")).collect();
        let after: String = (0..2100)
            .map(|i| {
                if i == 7 {
                    "changed\n".to_string()
                } else {
                    format!("l{i:05}\n")
                }
            })
            .collect();
        let change = write_change("huge.txt", Some(&before), &after, None);
        assert_eq!(change.op, FileChangeOp::Modify);
        assert_eq!(change.additions, 1);
        assert_eq!(change.deletions, 1);
        assert!(change.diff.is_none(), "degraded diff must be dropped");
        assert!(change.diff_truncated);
    }

    #[test]
    fn todo_snapshot_derives_stable_ids_and_dedup() {
        let args = json!({
            "items": [
                {"content": "a", "status": "pending"},
                {"content": "a", "status": "in_progress"},
                {"content": "b", "status": "completed"}
            ]
        });
        let snapshot = todo_snapshot_from_args(&args).expect("valid todo args");
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.items[0].content, "a");
        assert_eq!(snapshot.items[0].status, TodoStatusArtifact::Pending);
        assert_eq!(snapshot.items[1].status, TodoStatusArtifact::InProgress);
        assert_eq!(snapshot.items[2].status, TodoStatusArtifact::Completed);
        assert!(snapshot.items[0].id.starts_with('t'));
        assert_eq!(snapshot.items[1].id, format!("{}#2", snapshot.items[0].id));
        assert_ne!(snapshot.items[2].id, snapshot.items[0].id);
        let again = todo_snapshot_from_args(&args).expect("valid todo args");
        assert_eq!(snapshot.items, again.items);
        assert_eq!(snapshot.items[0].id, again.items[0].id);
    }

    #[test]
    fn todo_snapshot_rejects_invalid_status() {
        let args = json!({"items": [{"content": "a", "status": "doing"}]});
        let err = todo_snapshot_from_args(&args).expect_err("invalid status must fail");
        assert!(err.contains("doing"), "error: {err}");
    }

    #[test]
    fn todo_snapshot_rejects_bad_items_shape() {
        assert!(todo_snapshot_from_args(&json!({})).is_err());
        assert!(todo_snapshot_from_args(&json!({"items": "nope"})).is_err());
        assert!(todo_snapshot_from_args(&json!({"items": [{"content": "  "}]})).is_err());
        assert!(todo_snapshot_from_args(&json!({"items": [{"content": 3}]})).is_err());
    }

    #[test]
    fn todo_snapshot_empty_items_is_legal_clear() {
        let snapshot = todo_snapshot_from_args(&json!({"items": []})).expect("empty clear");
        assert!(snapshot.items.is_empty());
    }

    #[test]
    fn todo_snapshot_defaults_missing_status_to_pending() {
        let args = json!({"items": [{"content": "a"}]});
        let snapshot = todo_snapshot_from_args(&args).expect("status is optional");
        assert_eq!(snapshot.items[0].status, TodoStatusArtifact::Pending);
        assert_eq!(snapshot.items[0].status.as_str(), "pending");
    }

    #[test]
    fn todo_status_round_trips_wire_strings() {
        for status in [
            TodoStatusArtifact::Pending,
            TodoStatusArtifact::InProgress,
            TodoStatusArtifact::Completed,
            TodoStatusArtifact::Cancelled,
        ] {
            assert_eq!(TodoStatusArtifact::parse(status.as_str()), Ok(status));
        }
        assert!(TodoStatusArtifact::parse("nope").is_err());
    }

    #[test]
    fn unified_diff_identical_is_empty_and_change_has_prefixes() {
        assert_eq!(unified_diff("a\nb", "a\nb"), "");
        let diff = unified_diff("a", "b");
        assert!(diff.contains("-a"), "diff: {diff}");
        assert!(diff.contains("+b"), "diff: {diff}");
    }

    #[test]
    fn turn_artifact_wraps_both_variants() {
        let file = TurnArtifact::FileChange(write_change("f", None, "x", None));
        let todo = TurnArtifact::Todo(todo_snapshot_from_args(&json!({"items": []})).unwrap());
        match (file, todo) {
            (TurnArtifact::FileChange(change), TurnArtifact::Todo(snapshot)) => {
                assert_eq!(change.op, FileChangeOp::Add);
                assert!(snapshot.items.is_empty());
            }
            _ => panic!("unexpected TurnArtifact variants"),
        }
    }

    #[test]
    fn write_change_truncation_sets_total_bytes() {
        let big_after = "a\n".repeat(70 * 1024);
        let art = write_change("big.txt", None, &big_after, Some("call1"));
        assert_eq!(art.source, Some(FileChangeSource::Write));
        assert!(art.diff_truncated);
        assert!(art.total_bytes.unwrap() > 64 * 1024);
        assert!(art.diff.as_ref().unwrap().len() <= 64 * 1024);
    }

    #[test]
    fn bash_changes_tagged_bash() {
        let changes = bash_file_changes(vec![("a.txt", 5, 1), ("b.txt", 2, 0)], Some("call2"));
        assert_eq!(changes.len(), 2);
        for c in &changes {
            assert_eq!(c.source, Some(FileChangeSource::Bash));
            assert!(c.diff.is_none());
            assert!(!c.diff_truncated);
            assert!(c.total_bytes.is_none());
        }
        assert_eq!(changes[0].additions, 5);
        assert_eq!(changes[0].deletions, 1);
    }
}
