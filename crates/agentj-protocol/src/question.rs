//! `$/askQuestion` + `question/respond` wire types (design 决策 #5 ask tool). @since 0.13.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::SessionId;

/// Default for `allow_other` — omitted on the wire means true. @since 0.13.0
fn default_true() -> bool {
    true
}

/// `$/askQuestion` params (server → owner client, 2–8 options). @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AskQuestionParams {
    pub question_id: String,
    pub session_id: SessionId,
    pub question: String,
    /// Optional short header for the question card. @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub options: Vec<AskOption>,
    /// Always serialized; omitted on the wire means false. @since 0.13.0
    #[serde(default)]
    pub allow_multiple: bool,
    /// Always serialized; omitted on the wire means true. @since 0.13.0
    #[serde(default = "default_true")]
    pub allow_other: bool,
    /// Timeout in seconds; `None` = no timeout (timeout settles as skipped).
    /// @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
}

/// One selectable answer option. @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AskOption {
    pub id: String,
    pub label: String,
    /// Optional hint rendered with the option. @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `question/respond` params (`answers` are option ids or `other:`-prefixed
/// free text). @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionRespondParams {
    pub question_id: String,
    pub answers: Vec<String>,
    /// Omitted on the wire means false. @since 0.13.0
    #[serde(default)]
    pub skipped: bool,
}

/// `question/respond` result — `false` for duplicate/unknown/settled ids.
/// @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuestionRespondResult {
    pub accepted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_defaults_present_and_optional_fields_omitted() {
        let p = AskQuestionParams {
            question_id: "q1".into(),
            session_id: "s1".into(),
            question: "Pick a color".into(),
            header: None,
            options: vec![AskOption {
                id: "red".into(),
                label: "Red".into(),
                hint: None,
            }],
            allow_multiple: false,
            allow_other: true,
            timeout_s: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["question_id"], "q1");
        assert_eq!(v["session_id"], "s1");
        assert_eq!(v["allow_multiple"], false);
        assert_eq!(v["allow_other"], true);
        assert!(v.get("header").is_none(), "None header must be omitted");
        assert!(
            v.get("timeout_s").is_none(),
            "None timeout_s must be omitted"
        );
        assert!(
            v["options"][0].get("hint").is_none(),
            "None hint must be omitted"
        );
        assert!(v["allow_multiple"].is_boolean() && v["allow_other"].is_boolean());
    }

    #[test]
    fn question_wire_defaults_applied_when_fields_absent() {
        let p: AskQuestionParams = serde_json::from_str(
            r#"{"question_id":"q1","session_id":"s1","question":"Pick","options":[{"id":"a","label":"A"}]}"#,
        )
        .unwrap();
        assert!(!p.allow_multiple);
        assert!(p.allow_other);
        assert!(p.header.is_none() && p.timeout_s.is_none());
    }

    #[test]
    fn question_respond_skipped_defaults_false() {
        let p: QuestionRespondParams =
            serde_json::from_str(r#"{"question_id":"q1","answers":["red"]}"#).unwrap();
        assert!(!p.skipped);
        assert_eq!(p.answers, vec!["red".to_string()]);
        let skipped: QuestionRespondParams =
            serde_json::from_str(r#"{"question_id":"q1","answers":[],"skipped":true}"#).unwrap();
        assert!(skipped.skipped);
    }

    #[test]
    fn question_ask_payload_with_all_optionals_roundtrips() {
        let p = AskQuestionParams {
            question_id: "q2".into(),
            session_id: "s1".into(),
            question: "Pick".into(),
            header: Some("Header".into()),
            options: vec![AskOption {
                id: "a".into(),
                label: "A".into(),
                hint: Some("hint-a".into()),
            }],
            allow_multiple: true,
            allow_other: false,
            timeout_s: Some(30),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["header"], "Header");
        assert_eq!(v["options"][0]["hint"], "hint-a");
        assert_eq!(v["allow_multiple"], true);
        assert_eq!(v["allow_other"], false);
        assert_eq!(v["timeout_s"], 30);
        let back: AskQuestionParams = serde_json::from_value(v).unwrap();
        assert_eq!(back.question_id, "q2");
        assert_eq!(back.timeout_s, Some(30));
        assert_eq!(back.options.len(), 1);
    }

    #[test]
    fn question_respond_result_accepted_is_boolean() {
        let r = QuestionRespondResult { accepted: true };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["accepted"], true);
        let back: QuestionRespondResult = serde_json::from_str(r#"{"accepted":false}"#).unwrap();
        assert!(!back.accepted);
    }

    #[test]
    fn question_types_generate_json_schema() {
        let params = serde_json::to_value(schemars::schema_for!(AskQuestionParams)).unwrap();
        assert_eq!(params["type"], "object");
        for key in ["question_id", "session_id", "question", "options"] {
            assert!(params["properties"].get(key).is_some(), "missing {key}");
        }
        let option = serde_json::to_value(schemars::schema_for!(AskOption)).unwrap();
        assert!(option["properties"].get("hint").is_some());
        let respond = serde_json::to_value(schemars::schema_for!(QuestionRespondParams)).unwrap();
        assert!(respond["properties"].get("skipped").is_some());
    }
}
