//! Streaming + kernel method types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base::Id;
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
pub struct KernelStatusResult {
    pub version: String,
    pub modules: Vec<ModuleInfo>,
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
}
