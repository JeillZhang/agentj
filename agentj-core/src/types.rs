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
}
