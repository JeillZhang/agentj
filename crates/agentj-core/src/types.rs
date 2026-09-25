use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::artifacts::{FileChangeArtifact, TodoSnapshot};
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
    Todo {
        run_id: Option<String>,
        revision: u64,
        snapshot: TodoSnapshot,
    },
    FileChange(FileChangeArtifact),
    Done {
        run_id: Option<String>,
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
    /// Session-level path base for tools; absent in legacy persisted data. @since 0.13.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionListEntry {
    pub session_id: SessionId,
    pub updated_at: u64,
    pub status: crate::module::SessionStatus,
    pub preview: Option<String>,
}

pub const PREVIEW_MAX_CHARS: usize = 120;

pub fn preview_of(snapshot: &SessionSnapshot) -> Option<String> {
    let last = snapshot.messages.last()?.content.clone();
    Some(last.chars().take(PREVIEW_MAX_CHARS).collect())
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
    fn todo_and_file_change_events_are_constructible() {
        let snapshot = TodoSnapshot { items: vec![] };
        let todo = AgentEvent::Todo {
            run_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            revision: 3,
            snapshot: snapshot.clone(),
        };
        assert!(matches!(
            todo,
            AgentEvent::Todo {
                revision: 3,
                snapshot: s,
                ..
            } if s == snapshot
        ));
        // legacy None still constructible
        let legacy = AgentEvent::Todo {
            run_id: None,
            revision: 1,
            snapshot: snapshot.clone(),
        };
        assert!(matches!(legacy, AgentEvent::Todo { revision: 1, .. }));
        let change = crate::artifacts::write_change("f.txt", None, "x", Some("c1"));
        let event = AgentEvent::FileChange(change.clone());
        assert!(matches!(event, AgentEvent::FileChange(c) if c == change));
        let done = AgentEvent::Done {
            run_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            final_response: "ok".into(),
            tool_calls: vec![],
            usage: None,
        };
        assert!(matches!(
            done,
            AgentEvent::Done {
                run_id: Some(_),
                ..
            }
        ));
    }

    // ---- T5: SessionSnapshot.project_root (design decision #3) ----

    #[test]
    fn snapshot_without_root_deserializes_none() {
        let raw =
            r#"{"session_id":"s","status":"Active","messages":[],"created_at":1,"updated_at":2}"#;
        let snap: SessionSnapshot = serde_json::from_str(raw).unwrap();
        assert_eq!(snap.project_root, None);
    }

    #[test]
    fn snapshot_with_project_root_serializes_field() {
        let snap = SessionSnapshot {
            session_id: "s".into(),
            status: SessionStatus::Active,
            messages: vec![],
            created_at: 1,
            updated_at: 2,
            project_root: Some("/tmp/p".into()),
        };
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["project_root"], serde_json::json!("/tmp/p"));
    }

    #[test]
    fn snapshot_without_project_root_omits_field() {
        let snap = SessionSnapshot {
            session_id: "s".into(),
            status: SessionStatus::Active,
            messages: vec![],
            created_at: 1,
            updated_at: 2,
            project_root: None,
        };
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.get("project_root").is_none());
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
