//! Session method types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base::ProgressToken;
use crate::types::{Attachment, SessionId, SessionStatus, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionNewResult {
    pub session_id: SessionId,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRunResult {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionIdParams {
    pub session_id: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionStatusResult {
    pub status: SessionStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::ProgressToken;
    use crate::types::SessionStatus;

    #[test]
    fn session_new_result_roundtrip() {
        let r = SessionNewResult {
            session_id: "s1".into(),
        };
        assert_eq!(serde_json::to_value(&r).unwrap()["session_id"], "s1");
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
}
