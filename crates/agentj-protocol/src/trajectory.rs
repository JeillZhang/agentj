//! Trajectory wire types: append-only per-session event log (design D1 轨迹/时间线,
//! spec §后端 4). One `TrajectoryEvent` JSON object per log line / history-page item;
//! the store file and the `session/history` payload share these types byte-for-byte
//! (concretization 3). Envelope-then-tagged-value nesting mirrors `ProgressParams`
//! {token, value} so schemars/TS consumers model it exactly like `$/progress`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::permission::PermissionDecision;
use crate::question::AskOption;
use crate::session::UsageInfo;
use crate::streaming::{FileChange, TodoItem};
use crate::types::{SessionId, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryEvent {
    pub session_id: SessionId,
    /// Monotonic per-session sequence number; page cursor = last seen seq + 1.
    pub seq: u64,
    /// Unix epoch **milliseconds**.
    pub ts: u64,
    pub value: TrajectoryValue,
}

/// Kind payload of a trajectory event, discriminated by `value.kind`.
/// Forward-compat by the standard rule: consumers ignore unknown kinds, and the
/// store replay skips unparseable lines defensively (see T7) — no serde(other)
/// needed on a tagged enum with payloads, matching ProgressValue's policy.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrajectoryValue {
    User {
        content: String,
    },
    /// Merged streamed deltas of one assistant thought span — a single node (concretization 4).
    Thinking {
        content: String,
    },
    ToolCall {
        tool_call: ToolCall,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
    /// Recorded on the explicit permission respond path only (concretization 4).
    Permission {
        request_id: String,
        tool: String,
        decision: PermissionDecision,
    },
    Usage {
        usage: UsageInfo,
    },
    Error {
        message: String,
    },
    /// Full TODO snapshot replay; the highest revision wins on rebuild (决策 #10). @since 0.13.0
    Todo {
        revision: u64,
        items: Vec<TodoItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>, // @since 0.15.0
    },
    /// File change replay, mirroring the live `file_change` progress payload. @since 0.13.0
    FileChange {
        change: FileChange,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>, // @since 0.15.0
    },
    AskQuestion {
        question_id: String,
        question: String,
        header: Option<String>,
        options: Vec<AskOption>,
        allow_multiple: bool,
        allow_other: bool,
    },
    /// Question settlement replay, recorded exactly once per question. @since 0.13.0
    QuestionAnswer {
        question_id: String,
        answers: Vec<String>,
        skipped: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::PermissionDecision;
    use crate::question::AskOption;
    use crate::session::UsageInfo;
    use crate::streaming::{FileChange, FileChangeKind, TodoItem, TodoStatus};
    use crate::types::ToolCall;

    fn event(seq: u64, kind: &str) -> serde_json::Value {
        let value = match kind {
            "user" => TrajectoryValue::User {
                content: "hi".into(),
            },
            "thinking" => TrajectoryValue::Thinking {
                content: "plan".into(),
            },
            "tool_call" => TrajectoryValue::ToolCall {
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "bash".into(),
                    arguments: serde_json::json!({}),
                },
            },
            "tool_result" => TrajectoryValue::ToolResult {
                tool_call_id: "c1".into(),
                content: "ok".into(),
            },
            "permission" => TrajectoryValue::Permission {
                request_id: "r1".into(),
                tool: "bash".into(),
                decision: PermissionDecision::Allow,
            },
            "usage" => TrajectoryValue::Usage {
                usage: UsageInfo {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                },
            },
            "error" => TrajectoryValue::Error {
                message: "boom".into(),
            },
            "todo" => TrajectoryValue::Todo {
                revision: 3,
                items: vec![TodoItem {
                    id: "t1".into(),
                    content: "task".into(),
                    status: TodoStatus::InProgress,
                }],
                run_id: None,
            },
            "file_change" => TrajectoryValue::FileChange {
                change: FileChange {
                    path: "src/lib.rs".into(),
                    op: FileChangeKind::Modify,
                    additions: 1,
                    deletions: 2,
                    tool_call_id: Some("c1".into()),
                    diff: None,
                    diff_truncated: None,
                    run_id: None,
                    seq: None,
                    source: None,
                    total_bytes: None,
                },
                run_id: None,
            },
            "ask_question" => TrajectoryValue::AskQuestion {
                question_id: "q1".into(),
                question: "pick one".into(),
                header: Some("Header".into()),
                options: vec![AskOption {
                    id: "o1".into(),
                    label: "One".into(),
                    hint: None,
                }],
                allow_multiple: false,
                allow_other: true,
            },
            "question_answer" => TrajectoryValue::QuestionAnswer {
                question_id: "q1".into(),
                answers: vec!["o1".into()],
                skipped: false,
            },
            other => panic!("bad kind {other}"),
        };
        serde_json::to_value(TrajectoryEvent {
            session_id: "s-1".into(),
            seq,
            ts: 1_700_000_000_000,
            value,
        })
        .unwrap()
    }

    #[test]
    fn every_kind_serializes_with_envelope_and_tag() {
        for kind in [
            "user",
            "thinking",
            "tool_call",
            "tool_result",
            "permission",
            "usage",
            "error",
            "todo",
            "file_change",
            "ask_question",
            "question_answer",
        ] {
            let json = event(7, kind);
            assert_eq!(json["session_id"], "s-1");
            assert_eq!(json["seq"], 7);
            assert_eq!(json["ts"], 1_700_000_000_000_u64);
            assert_eq!(json["value"]["kind"], kind, "kind tag for {kind}");
        }
    }

    #[test]
    fn trajectory_todo_and_file_change_variants_roundtrip() {
        let json = event(4, "todo");
        assert_eq!(json["value"]["revision"], 3);
        assert_eq!(json["value"]["items"][0]["status"], "in_progress");
        let back: TrajectoryEvent = serde_json::from_value(json).unwrap();
        match back.value {
            TrajectoryValue::Todo {
                revision, items, ..
            } => {
                assert_eq!(revision, 3);
                assert_eq!(items[0].id, "t1");
            }
            other => panic!("expected todo, got {other:?}"),
        }

        let json = event(5, "file_change");
        assert_eq!(json["value"]["change"]["path"], "src/lib.rs");
        assert_eq!(json["value"]["change"]["op"], "modify");
        let back: TrajectoryEvent = serde_json::from_value(json).unwrap();
        match back.value {
            TrajectoryValue::FileChange { change, .. } => {
                assert_eq!(change.path, "src/lib.rs");
                assert_eq!(change.op, FileChangeKind::Modify);
            }
            other => panic!("expected file_change, got {other:?}"),
        }
    }

    #[test]
    fn trajectory_new_variants_roundtrip() {
        let json = event(6, "ask_question");
        assert_eq!(json["value"]["question_id"], "q1");
        assert_eq!(json["value"]["options"][0]["id"], "o1");
        assert_eq!(json["value"]["allow_multiple"], false);
        assert_eq!(json["value"]["allow_other"], true);
        let back: TrajectoryEvent = serde_json::from_value(json).unwrap();
        match back.value {
            TrajectoryValue::AskQuestion {
                question_id,
                options,
                ..
            } => {
                assert_eq!(question_id, "q1");
                assert_eq!(options[0].label, "One");
            }
            other => panic!("expected ask_question, got {other:?}"),
        }

        let json = event(7, "question_answer");
        assert_eq!(json["value"]["answers"][0], "o1");
        assert_eq!(json["value"]["skipped"], false);
        let back: TrajectoryEvent = serde_json::from_value(json).unwrap();
        match back.value {
            TrajectoryValue::QuestionAnswer {
                question_id,
                answers,
                skipped,
            } => {
                assert_eq!(question_id, "q1");
                assert_eq!(answers, vec!["o1".to_string()]);
                assert!(!skipped);
            }
            other => panic!("expected question_answer, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_event_roundtrips_toolcall() {
        let json = event(1, "tool_call");
        assert_eq!(json["value"]["tool_call"]["name"], "bash");
        let back: TrajectoryEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back.session_id, "s-1");
        match back.value {
            TrajectoryValue::ToolCall { tool_call } => assert_eq!(tool_call.id, "c1"),
            other => panic!("expected tool_call, got {other:?}"),
        }
    }

    #[test]
    fn permission_event_decision_roundtrips_and_tolerates_unknown() {
        let json = serde_json::to_value(TrajectoryEvent {
            session_id: "s-1".into(),
            seq: 2,
            ts: 1,
            value: TrajectoryValue::Permission {
                request_id: "r1".into(),
                tool: "bash".into(),
                decision: PermissionDecision::AllowAlways,
            },
        })
        .unwrap();
        assert_eq!(json["value"]["decision"], "allow_always");
        // Forward-compat: unknown future decision strings parse, never fail.
        let unknown = serde_json::json!({"session_id": "s-1", "seq": 2, "ts": 1,
            "value": {"kind": "permission", "request_id": "r1", "tool": "bash",
                      "decision": "future_thing"}});
        let ev: TrajectoryEvent = serde_json::from_value(unknown).unwrap();
        match ev.value {
            TrajectoryValue::Permission {
                decision: PermissionDecision::Unknown,
                ..
            } => {}
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn trajectory_schema_lists_all_kinds() {
        let s = schemars::schema_for!(TrajectoryValue);
        let text = serde_json::to_string(&s).unwrap();
        for kind in [
            "user",
            "thinking",
            "tool_call",
            "tool_result",
            "permission",
            "usage",
            "error",
            "todo",
            "file_change",
            "ask_question",
            "question_answer",
        ] {
            assert!(
                text.contains(&format!("\"{kind}\"")),
                "schema must list kind {kind}"
            );
        }
    }
}
