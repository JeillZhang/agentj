//! Context pipeline crate — pure message-builder for the agent loop.

use agentj_core::module::ContextPipeline as ContextPipelineTrait;
use agentj_core::types::{ChatMessage, Role, ToolCall};

/// Pure message-builder pipeline. The system prompt is supplied by the caller
/// (AgentLoop) at assemble time, so this struct carries no state.
pub struct ContextPipeline;

impl ContextPipeline {
    pub fn new() -> Self { Self }
}

impl Default for ContextPipeline {
    fn default() -> Self { Self::new() }
}

impl ContextPipelineTrait for ContextPipeline {
    fn assemble(
        &self,
        system: &str,
        history: &[ChatMessage],
        tool_results: &[(ToolCall, String)],
    ) -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::system(system)];
        messages.extend_from_slice(history);
        for (tc, result) in tool_results {
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![tc.clone()],
            });
            messages.push(ChatMessage::tool_result(&tc.id, result));
        }
        messages
    }

    fn reflux(&self, history: &mut Vec<ChatMessage>, tool_call: &ToolCall, result: String) {
        history.push(ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            tool_call_id: None,
            tool_calls: vec![tool_call.clone()],
        });
        history.push(ChatMessage::tool_result(&tool_call.id, result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::module::ContextPipeline as _;
    use agentj_core::types::*;

    #[test]
    fn assemble_builds_system_prompt_and_history() {
        let pipeline = ContextPipeline::new();
        let history = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
        ];
        let result = pipeline.assemble("You are a helpful assistant.", &history, &[]);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "You are a helpful assistant.");
    }

    #[test]
    fn reflux_appends_tool_result() {
        let pipeline = ContextPipeline::new();
        let mut history = vec![ChatMessage::user("hi")];
        let tc = ToolCall { id: "call-1".into(), name: "read".into(), arguments: serde_json::json!({}) };
        pipeline.reflux(&mut history, &tc, "file content".into());
        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].tool_calls.len(), 1);
        assert_eq!(history[2].role, Role::Tool);
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call-1"));
    }
}
