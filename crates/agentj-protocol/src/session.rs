//! Session method types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base::ProgressToken;
use crate::trajectory::TrajectoryEvent;
use crate::types::{Attachment, SessionId, SessionStatus, ToolCall};

/// `session/new` params. `project_root` is an absolute directory; when omitted
/// the session defaults to the process CWD. @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionNewParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionNewResult {
    pub session_id: SessionId,
    /// Effective canonicalized session root (cwd fallback). @since 0.14.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRunParams {
    pub session_id: SessionId,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<ProgressToken>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRunResult {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionIdParams {
    pub session_id: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionStatusResult {
    pub status: SessionStatus,
}

/// `session/subscribe` params — subscribe to a session's running progress
/// (design 决策 #4: broadcast subscription). `$/progress` broadcasts to all subscribers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionSubscribeParams {
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<ProgressToken>,
}

/// One row of `session/list`. @since 0.12.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub updated_at: u64,
    pub state: SessionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// @since 0.12.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionListParams {}

/// @since 0.12.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionListResult {
    pub sessions: Vec<SessionSummary>,
}

/// Page of a session trajectory. @since 0.12.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionHistoryParams {
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// @since 0.12.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionHistoryResult {
    pub events: Vec<TrajectoryEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ProgressToken;
    use crate::trajectory::{TrajectoryEvent, TrajectoryValue};
    use crate::types::SessionStatus;

    fn summary(sid: &str, preview: Option<String>) -> SessionSummary {
        SessionSummary {
            session_id: sid.into(),
            updated_at: 1_700_000_000,
            state: SessionStatus::Active,
            preview,
        }
    }

    #[test]
    fn session_list_result_shape() {
        let r = SessionListResult {
            sessions: vec![summary("s1", None), summary("s2", Some("hi".into()))],
        };
        let json = serde_json::to_value(&r).unwrap();
        let s0 = &json["sessions"][0];
        assert_eq!(s0["session_id"], "s1");
        assert_eq!(s0["state"], "active");
        assert_eq!(s0["updated_at"], 1_700_000_000);
        assert!(s0.get("preview").is_none(), "None preview must be omitted");
        assert_eq!(json["sessions"][1]["preview"], "hi");
        let back: SessionListResult = serde_json::from_value(json).unwrap();
        assert_eq!(back.sessions.len(), 2);
    }

    #[test]
    fn session_history_params_optional_cursor_limit() {
        let p = SessionHistoryParams {
            session_id: "s1".into(),
            cursor: None,
            limit: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("cursor").is_none() && json.get("limit").is_none());
        let back: SessionHistoryParams = serde_json::from_str(r#"{"session_id":"s1"}"#).unwrap();
        assert!(back.cursor.is_none() && back.limit.is_none());
        let with = SessionHistoryParams {
            session_id: "s1".into(),
            cursor: Some(4),
            limit: Some(50),
        };
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["cursor"], 4);
        assert_eq!(json["limit"], 50);
    }

    #[test]
    fn session_history_result_events_and_next_cursor() {
        let ev = TrajectoryEvent {
            session_id: "s1".into(),
            seq: 0,
            ts: 1,
            value: TrajectoryValue::User {
                content: "hi".into(),
            },
        };
        let done = SessionHistoryResult {
            events: vec![ev],
            next_cursor: None,
        };
        let json = serde_json::to_value(&done).unwrap();
        assert_eq!(json["events"][0]["session_id"], "s1");
        assert_eq!(json["events"][0]["value"]["kind"], "user");
        assert_eq!(json["events"][0]["seq"], 0);
        assert!(json.get("next_cursor").is_none());
        let more = SessionHistoryResult {
            events: vec![],
            next_cursor: Some(9),
        };
        let json = serde_json::to_value(&more).unwrap();
        assert_eq!(json["next_cursor"], 9);
    }

    #[test]
    fn session_new_params_project_root_roundtrip() {
        let p = SessionNewParams {
            project_root: Some("/tmp/proj".into()),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["project_root"], "/tmp/proj");
        let back: SessionNewParams = serde_json::from_value(json).unwrap();
        assert_eq!(back.project_root.as_deref(), Some("/tmp/proj"));

        let none = SessionNewParams { project_root: None };
        let json = serde_json::to_value(&none).unwrap();
        assert!(
            json.get("project_root").is_none(),
            "None project_root must be omitted"
        );
        let empty: SessionNewParams =
            serde_json::from_str(r#"{}"#).expect("empty params must deserialize");
        assert!(empty.project_root.is_none());
    }

    #[test]
    fn session_new_result_roundtrip() {
        let r = SessionNewResult {
            session_id: "s1".into(),
            project_root: None,
        };
        assert_eq!(serde_json::to_value(&r).unwrap()["session_id"], "s1");
    }

    #[test]
    fn session_new_result_project_root_is_additive() {
        let none = SessionNewResult {
            session_id: "s1".into(),
            project_root: None,
        };
        assert!(
            serde_json::to_value(&none)
                .unwrap()
                .get("project_root")
                .is_none()
        );
        let some = SessionNewResult {
            session_id: "s1".into(),
            project_root: Some("/tmp/proj".into()),
        };
        assert_eq!(
            serde_json::to_value(some).unwrap()["project_root"],
            "/tmp/proj"
        );
    }

    #[test]
    fn session_run_params_serialize() {
        let p = SessionRunParams {
            session_id: "s1".into(),
            content: "hi".into(),
            attachments: None,
            progress_token: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["session_id"], "s1");
        assert!(json.get("attachments").is_none());
        assert!(json.get("progress_token").is_none());
    }

    #[test]
    fn session_run_params_with_progress_token() {
        let p = SessionRunParams {
            session_id: "s1".into(),
            content: "hi".into(),
            attachments: Some(vec![crate::types::Attachment {
                path: Some("a.txt".into()),
                content: None,
                mime: None,
            }]),
            progress_token: Some(ProgressToken::Str("tok".into())),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["progress_token"], "tok");
        assert_eq!(json["attachments"][0]["path"], "a.txt");
    }

    #[test]
    fn session_status_result_serialize() {
        let r = SessionStatusResult {
            status: SessionStatus::Archived,
        };
        assert_eq!(serde_json::to_value(&r).unwrap()["status"], "archived");
    }

    #[test]
    fn session_id_params_roundtrip() {
        let p = SessionIdParams {
            session_id: "s9".into(),
        };
        let back: SessionIdParams =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.session_id, "s9");
    }

    #[test]
    fn session_subscribe_params_serialize() {
        let p = SessionSubscribeParams {
            session_id: "s1".into(),
            progress_token: Some(ProgressToken::Str("tok".into())),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["session_id"], "s1");
        assert_eq!(json["progress_token"], "tok");
    }

    #[test]
    fn session_subscribe_params_progress_token_optional() {
        let p = SessionSubscribeParams {
            session_id: "s1".into(),
            progress_token: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("progress_token").is_none());
    }
}
