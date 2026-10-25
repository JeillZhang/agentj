//! Context pipeline — pure message-builder for the agent loop.

use crate::module::ContextPipeline as ContextPipelineTrait;
use crate::types::{ChatMessage, Role, ToolCall, ToolDefinition};

/// Pure message-builder pipeline. The system prompt is supplied by the caller
/// (AgentLoop) at assemble time, so this struct carries no state.
pub struct DefaultContextPipeline;

impl DefaultContextPipeline {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultContextPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextPipelineTrait for DefaultContextPipeline {
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

    /// Charges the definition reservation against the **total**, not against a body that
    /// `fit` then re-floors at 1k tokens — otherwise a big tool catalogue on a small window
    /// silently overflows and the overflow retry replays a byte-identical request.
    fn fit_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tool_definitions: &[ToolDefinition],
        budget_tokens: usize,
    ) -> Vec<ChatMessage> {
        crate::budget::fit_messages_with(
            messages,
            tool_definitions,
            budget_tokens,
            &crate::budget::HeuristicTokenizer,
        )
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

    #[test]
    fn assemble_builds_system_prompt_and_history() {
        let pipeline = DefaultContextPipeline::new();
        let history = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
        ];
        let result = pipeline.assemble("You are a helpful assistant.", &history, &[]);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "You are a helpful assistant.");
    }

    /// A toy tool whose definition costs roughly `tokens` (a long description is the
    /// cheapest way to make the reservation dominate a small window).
    fn fat_tool(name: &str, tokens: usize) -> crate::types::ToolDefinition {
        crate::types::ToolDefinition {
            name: name.to_string(),
            description: "d".repeat(tokens * 4),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    #[test]
    fn fit_with_tools_charges_definitions_against_the_total_budget() {
        let pipeline = DefaultContextPipeline::new();
        let defs = vec![fat_tool("fat", 1_500)];
        let reserved = crate::budget::definitions_tokens(&defs);
        assert!(reserved >= 1_500, "fixture must reserve a big chunk: {reserved}");
        // Ten ~104-token turns: with the reservation charged the body may keep about four of
        // them, while the generic `fit` floor (1k) would let nine through and overflow.
        let mut messages = vec![ChatMessage::system("sys")];
        messages.extend((0..10).map(|_| ChatMessage::user("u".repeat(400))));
        let fitted = pipeline.fit_with_tools(messages, &defs, 2_000);
        let body = crate::budget::request_tokens(&fitted);
        // The generic `fit` clamps the *body* to 1k, which would put the total at 2.5k+;
        // the reservation has to be charged against the total instead.
        assert!(
            body + reserved <= 2_000,
            "body {body} + definitions {reserved} must stay within 2000"
        );
    }

    #[test]
    fn reflux_appends_tool_result() {
        let pipeline = DefaultContextPipeline::new();
        let mut history = vec![ChatMessage::user("hi")];
        let tc = ToolCall {
            id: "call-1".into(),
            name: "read".into(),
            arguments: serde_json::json!({}),
        };
        pipeline.reflux(&mut history, &tc, "file content".into());
        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].tool_calls.len(), 1);
        assert_eq!(history[2].role, Role::Tool);
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call-1"));
    }
}
