//! Context pipeline crate — pure message-builder for the agent loop.

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
