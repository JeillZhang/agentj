//! Shared payload types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
    // Forward-compat: unknown roles are preserved as `Unknown`, never a parse error
    // (design 决策 #2 / spec §9). Excluded from the schema — schema lists known values.
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Attachment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModuleState {
    Unloaded,
    Initializing,
    Ready,
    Stopping,
    Stopped,
    Failed,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Archived,
    Deleted,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_lowercase() {
        assert_eq!(serde_json::to_value(Role::System).unwrap(), "system");
        assert_eq!(serde_json::to_value(Role::Assistant).unwrap(), "assistant");
        let r: Role = serde_json::from_value(serde_json::json!("tool")).unwrap();
        assert_eq!(r, Role::Tool);
    }

    #[test]
    fn unknown_enum_values_are_tolerated_not_fatal() {
        let r: Role = serde_json::from_value(serde_json::json!("weird-role")).unwrap();
        assert_eq!(r, Role::Unknown);
        let s: ModuleState = serde_json::from_value(serde_json::json!("future-state")).unwrap();
        assert_eq!(s, ModuleState::Unknown);
        let st: SessionStatus = serde_json::from_value(serde_json::json!("pending")).unwrap();
        assert_eq!(st, SessionStatus::Unknown);
    }

    #[test]
    fn chatmessage_roundtrip() {
        let m = ChatMessage {
            role: Role::User,
            content: "hello".into(),
            tool_call_id: None,
            tool_calls: vec![],
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["role"], "user");
        assert!(json.get("tool_call_id").is_none());
        assert!(json.get("tool_calls").is_none());
        let back: ChatMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back.content, "hello");
    }

    #[test]
    fn module_state_and_session_status_serialize_lowercase() {
        assert_eq!(serde_json::to_value(ModuleState::Ready).unwrap(), "ready");
        assert_eq!(
            serde_json::to_value(SessionStatus::Active).unwrap(),
            "active"
        );
    }

    #[test]
    fn tool_call_roundtrip() {
        let tc = ToolCall {
            id: "call-1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"cmd":"ls"}),
        };
        let back: ToolCall = serde_json::from_str(&serde_json::to_string(&tc).unwrap()).unwrap();
        assert_eq!(back.name, "bash");
        assert_eq!(back.arguments["cmd"], "ls");
    }

    #[test]
    fn shared_types_generate_json_schema() {
        let s = schemars::schema_for!(ChatMessage);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["type"], "object");
        assert!(json["properties"].get("role").is_some());
    }
}
