//! Streaming + kernel method types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base::Id;
use crate::permission::PermissionDecision;
use crate::session::UsageInfo;
use crate::types::{ModuleState, SessionId, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgressParams {
    pub token: crate::base::ProgressToken,
    pub value: ProgressValue,
}

/// `$/progress` payload, discriminated by `kind` (design 决策 #3).
/// An unknown `kind` is ignorable by consumers (per `$/`-prefix convention,
/// design 决策 #5) — consumer-side behavior documented in the spec, deferred to v0.1.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressValue {
    Text {
        delta: String,
    },
    ToolCall {
        tool_call: ToolCall,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
    /// Token-usage settlement emitted once per run at Done (design D1 结算行). @since 0.12.0
    Usage {
        usage: UsageInfo,
    },
    /// Decision settlement for a pending $/permissionRequest (design D1 轨迹/卡片结算).
    /// session_id is optional-additive so multi-session clients route the settle.
    Permission {
        request_id: String,
        tool: String,
        decision: PermissionDecision,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<SessionId>, // @since 0.12.0
    },
    /// Full TODO snapshot (design 决策 #2); revision is monotonic per session,
    /// clients discard snapshots older than the highest seen. @since 0.13.0
    Todo {
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>, // @since 0.15.0
        revision: u64,
        items: Vec<TodoItem>,
    },
    /// Workspace file change produced by write/bash artifacts (design 决策 #3). @since 0.13.0
    FileChange {
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>, // @since 0.15.0
        change: FileChange,
    },
}

/// One entry of a full TODO snapshot. @since 0.13.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TodoItem {
    /// Kernel-computed stable id (content hash; duplicates disambiguated by occurrence).
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
}

/// Lifecycle status of a TODO item. `Unknown` tolerates forward values
/// (serde(other)); schema lists only the known values. @since 0.13.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// A workspace file change. @since 0.13.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FileChange {
    /// Path relative to the project root.
    pub path: String,
    pub op: FileChangeKind,
    pub additions: u64,
    pub deletions: u64,
    /// Tool call card this change belongs to; omitted when unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Unified diff text; omitted when unavailable (untracked / over limit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// True only when the diff was truncated; omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_truncated: Option<bool>,
    /// Run this change belongs to. @since 0.15.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Per-run monotonic sequence for ordering inside a run. @since 0.15.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// Source of the change: write (LCS diff) or bash (numstat only). @since 0.15.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<FileChangeSource>,
    /// Original diff byte length when truncated. @since 0.15.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

/// Change operation of a `FileChange`. `Unknown` tolerates forward values
/// (serde(other)); schema lists only the known values. @since 0.13.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Add,
    Modify,
    Delete,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Source of a FileChange: write produces LCS diff, bash produces counts only. @since 0.15.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeSource {
    Write,
    Bash,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CancelParams {
    pub id: Id,
    /// Target session for the cancel (design G2). Additive: absent on legacy
    /// clients — the server falls back to a single-inflight session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>, // @since 0.6.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderInfo {
    pub id: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KernelStatusResult {
    pub version: String,
    pub modules: Vec<ModuleInfo>,
    /// Config model.default_model, e.g. "deepseek-chat". Absent when no config. @since 0.12.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Provider list from config (name → base_url); models reserved, empty. @since 0.12.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<ProviderInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleInfo {
    pub id: String,
    pub state: ModuleState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::Id;
    use crate::permission::PermissionDecision;
    use crate::session::UsageInfo;
    use crate::types::{ModuleState, ToolCall};

    #[test]
    fn progress_value_text_kind() {
        let v = ProgressValue::Text {
            delta: "hello".into(),
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["kind"], "text");
        assert_eq!(json["delta"], "hello");
    }

    #[test]
    fn progress_value_tool_call_kind() {
        let v = ProgressValue::ToolCall {
            tool_call: ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({}),
            },
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["kind"], "tool_call");
        assert_eq!(json["tool_call"]["name"], "bash");
    }

    #[test]
    fn progress_value_tool_result_kind() {
        let v = ProgressValue::ToolResult {
            tool_call_id: "c1".into(),
            content: "ok".into(),
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["kind"], "tool_result");
        assert_eq!(json["tool_call_id"], "c1");
    }

    #[test]
    fn cancel_params_with_session_id_roundtrip() {
        let p = CancelParams {
            id: Id::Number(5),
            session_id: Some("s-1".into()),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["session_id"], "s-1");
        let back: CancelParams = serde_json::from_value(json).unwrap();
        assert_eq!(back.session_id.as_deref(), Some("s-1"));
    }

    #[test]
    fn cancel_params_without_session_id_omits_field_and_accepts_legacy_wire() {
        let p = CancelParams {
            id: Id::Number(5),
            session_id: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("session_id").is_none(), "None must not serialize");
        let back: CancelParams = serde_json::from_str(r#"{"id":5}"#).unwrap();
        assert_eq!(back.id, Id::Number(5));
        assert!(back.session_id.is_none());
    }

    #[test]
    fn kernel_status_result_serialize() {
        let r = KernelStatusResult {
            version: "0.1.0".into(),
            modules: vec![ModuleInfo {
                id: "m1".into(),
                state: ModuleState::Ready,
            }],
            default_model: None,
            providers: None,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["version"], "0.1.0");
        assert_eq!(json["modules"][0]["state"], "ready");
    }

    #[test]
    fn streaming_types_generate_json_schema() {
        let s = schemars::schema_for!(ProgressValue);
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("anyOf").is_some() || json.get("oneOf").is_some());
    }

    #[test]
    fn progress_value_usage_kind() {
        let v = ProgressValue::Usage {
            usage: UsageInfo {
                prompt_tokens: 10,
                completion_tokens: 5,
            },
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["kind"], "usage");
        assert_eq!(json["usage"]["prompt_tokens"], 10);
        assert_eq!(json["usage"]["completion_tokens"], 5);
        let back: ProgressValue = serde_json::from_value(json).unwrap();
        assert!(matches!(back, ProgressValue::Usage { usage } if usage.prompt_tokens == 10));
    }

    #[test]
    fn progress_value_permission_kind_with_session() {
        let v = ProgressValue::Permission {
            request_id: "r1".into(),
            tool: "bash".into(),
            decision: PermissionDecision::AllowAlways,
            session_id: Some("s-1".into()),
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["kind"], "permission");
        assert_eq!(json["request_id"], "r1");
        assert_eq!(json["tool"], "bash");
        assert_eq!(json["decision"], "allow_always");
        assert_eq!(json["session_id"], "s-1");
    }

    #[test]
    fn progress_value_permission_kind_omits_session_when_none() {
        let v = ProgressValue::Permission {
            request_id: "r1".into(),
            tool: "bash".into(),
            decision: PermissionDecision::Deny,
            session_id: None,
        };
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("session_id").is_none());
    }

    #[test]
    fn progress_value_permission_unknown_decision_tolerated() {
        // Forward-compat: a future decision string must not break parsing.
        let json = serde_json::json!({"kind": "permission", "request_id": "r1", "tool": "bash",
                                      "decision": "maybe_later"});
        let v: ProgressValue = serde_json::from_value(json).unwrap();
        match v {
            ProgressValue::Permission {
                decision: PermissionDecision::Unknown,
                ..
            } => {}
            other => panic!("expected Unknown decision, got {other:?}"),
        }
    }

    #[test]
    fn progress_todo_kind_wire_shape() {
        let v = ProgressValue::Todo {
            session_id: "s1".into(),
            run_id: None,
            revision: 3,
            items: vec![TodoItem {
                id: "t1".into(),
                content: "task".into(),
                status: TodoStatus::InProgress,
            }],
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["kind"], "todo");
        assert_eq!(j["revision"], 3);
        assert_eq!(j["items"][0]["status"], "in_progress");
    }

    #[test]
    fn todo_status_unknown_tolerated() {
        // Forward-compat: a future status string must not break parsing.
        let status: TodoStatus = serde_json::from_str("\"blocked\"").unwrap();
        assert_eq!(status, TodoStatus::Unknown);
        let known: TodoStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(known, TodoStatus::Completed);
    }

    #[test]
    fn file_change_kind_unknown_tolerated() {
        // Forward-compat: a future op string must not break parsing.
        let op: FileChangeKind = serde_json::from_str("\"rename\"").unwrap();
        assert_eq!(op, FileChangeKind::Unknown);
    }

    #[test]
    fn file_change_omits_none_diff_fields() {
        let v = ProgressValue::FileChange {
            session_id: "s1".into(),
            run_id: None,
            change: FileChange {
                path: "src/lib.rs".into(),
                op: FileChangeKind::Modify,
                additions: 2,
                deletions: 1,
                tool_call_id: None,
                diff: None,
                diff_truncated: None,
                run_id: None,
                seq: None,
                source: None,
                total_bytes: None,
            },
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["kind"], "file_change");
        assert_eq!(j["change"]["path"], "src/lib.rs");
        assert_eq!(j["change"]["op"], "modify");
        assert_eq!(j["change"]["additions"], 2);
        assert_eq!(j["change"]["deletions"], 1);
        assert!(j["change"].get("tool_call_id").is_none());
        assert!(
            j["change"].get("diff").is_none(),
            "None diff must not serialize"
        );
        assert!(
            j["change"].get("diff_truncated").is_none(),
            "None diff_truncated must not serialize"
        );
    }

    #[test]
    fn kernel_status_result_optional_model_fields() {
        let with = KernelStatusResult {
            version: "0.11.1".into(),
            modules: vec![],
            default_model: Some("deepseek-chat".into()),
            providers: Some(vec![ProviderInfo {
                id: "deepseek-chat".into(),
                models: vec![],
            }]),
        };
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["default_model"], "deepseek-chat");
        assert_eq!(json["providers"][0]["id"], "deepseek-chat");
        assert_eq!(json["providers"][0]["models"].as_array().unwrap().len(), 0);

        let without = KernelStatusResult {
            version: "0.11.1".into(),
            modules: vec![],
            default_model: None,
            providers: None,
        };
        let json = serde_json::to_value(&without).unwrap();
        assert!(json.get("default_model").is_none());
        assert!(json.get("providers").is_none());
        // Legacy wire payload (no new keys) parses with None.
        let legacy: KernelStatusResult =
            serde_json::from_str(r#"{"version":"0.11.1","modules":[]}"#).unwrap();
        assert!(legacy.default_model.is_none() && legacy.providers.is_none());
    }

    #[test]
    fn file_change_source_unknown_tolerated() {
        // Forward-compat: a future source string must not break parsing.
        let fc: FileChange = serde_json::from_str(
            r#"{"path":"a.txt","op":"modify","additions":1,"deletions":0,"source":"edit"}"#,
        )
        .unwrap();
        assert_eq!(fc.source, Some(FileChangeSource::Unknown));
    }

    #[test]
    fn file_change_new_fields_roundtrip() {
        let fc = FileChange {
            path: "src/main.rs".into(),
            op: FileChangeKind::Modify,
            additions: 10,
            deletions: 2,
            tool_call_id: Some("call_1".into()),
            diff: Some("@@ -1 +1 @@\n-hi\n+hello".into()),
            diff_truncated: Some(true),
            run_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            seq: Some(5),
            source: Some(FileChangeSource::Write),
            total_bytes: Some(70000),
        };
        let v = serde_json::to_value(&fc).unwrap();
        assert_eq!(v["source"], "write");
        assert_eq!(v["seq"], 5);
        assert_eq!(v["total_bytes"], 70000);
        assert_eq!(v["run_id"], "550e8400-e29b-41d4-a716-446655440000");
        let back: FileChange = serde_json::from_value(v).unwrap();
        assert_eq!(back, fc);

        let bare = FileChange {
            path: "a.txt".into(),
            op: FileChangeKind::Add,
            additions: 0,
            deletions: 0,
            tool_call_id: None,
            diff: None,
            diff_truncated: None,
            run_id: None,
            seq: None,
            source: None,
            total_bytes: None,
        };
        let j = serde_json::to_value(&bare).unwrap();
        assert!(j.get("run_id").is_none(), "None run_id must be omitted");
        assert!(j.get("seq").is_none(), "None seq must be omitted");
        assert!(j.get("source").is_none(), "None source must be omitted");
        assert!(
            j.get("total_bytes").is_none(),
            "None total_bytes must be omitted"
        );
    }

    #[test]
    fn progress_todo_run_id_additive() {
        let pv = ProgressValue::Todo {
            session_id: "s1".into(),
            run_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            revision: 3,
            items: vec![],
        };
        let j = serde_json::to_value(&pv).unwrap();
        assert_eq!(j["kind"], "todo");
        assert_eq!(j["run_id"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(j["revision"], 3);

        // Legacy wire without run_id still deserializes (additive field).
        let legacy: ProgressValue = serde_json::from_value(
            serde_json::json!({"kind":"todo","session_id":"s1","revision":3,"items":[]}),
        )
        .unwrap();
        match legacy {
            ProgressValue::Todo { run_id, .. } => assert!(run_id.is_none()),
            other => panic!("expected Todo, got {other:?}"),
        }
    }
}
