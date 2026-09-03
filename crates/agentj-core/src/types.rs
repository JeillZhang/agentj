use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::module::SessionStatus;

pub type ModuleId = String;
pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<ToolCall>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl TokenUsage {
    pub fn accumulate(&mut self, other: TokenUsage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
    }
}

#[derive(Debug, Clone)]
pub enum ModelEvent {
    Delta(String),
    ToolCall(ToolCall),
    Usage(TokenUsage),
    Done,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

/// Incremental event emitted during agent loop execution.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta(String),
    ToolCall(ToolCall),
    ToolResult {
        call_id: String,
        result: String,
    },
    Done {
        final_response: String,
        tool_calls: Vec<ToolCall>,
        usage: Option<TokenUsage>,
    },
    Error(String),
}

#[allow(clippy::type_complexity)]
pub type OnTurnComplete = Arc<dyn Fn(&SessionId, Option<TokenUsage>) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ModelOptions {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

impl Default for ModelOptions {
    fn default() -> Self {
        Self {
            model: "deepseek-chat".into(),
            temperature: None,
            max_tokens: None,
            stream: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub path: Option<String>,
    pub content: Option<String>,
    pub mime: Option<String>,
}

/// Serializable session snapshot — the atomic unit of persistence/recovery
/// (design §会话持久化与恢复, decision #16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub status: SessionStatus,
    pub messages: Vec<ChatMessage>,
    pub created_at: u64, // epoch seconds (InMemory: best-effort current time)
    pub updated_at: u64,
}

/// Current UNIX epoch seconds (shared by trait default + InMemoryStore).
pub fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatmessage_system_constructor() {
        let msg = ChatMessage::system("hello");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "hello");
        assert!(msg.tool_calls.is_empty());
    }

    #[test]
    fn chatmessage_assistant_and_tool_constructors() {
        let assistant = ChatMessage::assistant("hi");
        assert_eq!(assistant.role, Role::Assistant);
        let tool = ChatMessage::tool_result("call-1", "result");
        assert_eq!(tool.role, Role::Tool);
        assert_eq!(tool.tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn chatmessage_serialization_roundtrip() {
        let json = serde_json::to_string(&ChatMessage::user("hi")).unwrap();
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, Role::User);
    }

    #[test]
    fn model_options_default_is_streaming_deepseek() {
        let opts = ModelOptions::default();
        assert_eq!(opts.model, "deepseek-chat");
        assert!(opts.stream);
    }

    #[test]
    fn relocated_agent_event_is_constructible_and_cloneable() {
        let e = AgentEvent::Delta("x".into());
        let e2 = e.clone();
        assert!(matches!(e2, AgentEvent::Delta(t) if t == "x"));
    }

    #[test]
    fn token_usage_accumulate_sums_and_saturates() {
        let mut u = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
        };
        u.accumulate(TokenUsage {
            prompt_tokens: 90,
            completion_tokens: 15,
        });
        assert_eq!(
            u,
            TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20
            }
        );
        u.prompt_tokens = u64::MAX;
        u.accumulate(TokenUsage {
            prompt_tokens: 1,
            completion_tokens: 0,
        });
        assert_eq!(u.prompt_tokens, u64::MAX);
    }
}
