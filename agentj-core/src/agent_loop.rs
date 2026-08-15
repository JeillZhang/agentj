//! Agent loop — orchestrates a single turn: chat stream, tool calls, and
//! history persistence. RED state: tests only, AgentLoop not implemented yet.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::*;
    use crate::module::*;
    use crate::types::*;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use std::sync::{Arc, Mutex};

    struct MockProvider { chunks: Vec<String> }
    #[async_trait]
    impl ModelProvider for MockProvider {
        async fn chat_stream(&self, _m: Vec<ChatMessage>, _t: Vec<ToolDefinition>, _o: ModelOptions)
            -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let chunks: Vec<_> = self.chunks.iter().map(|s| Ok(ModelEvent::Delta(s.clone())))
                .chain(std::iter::once(Ok(ModelEvent::Done))).collect();
            Ok(Box::pin(stream::iter(chunks)))
        }
        async fn chat(&self, _m: Vec<ChatMessage>, _o: ModelOptions) -> Result<ChatResponse, ModelError> {
            Ok(ChatResponse { content: "mock".into(), tool_calls: vec![] })
        }
    }

    // design 4.3: records the model string it receives so tests can assert the
    // config-driven default_model reached the provider.
    struct RecordingProvider { models: Arc<Mutex<Vec<String>>> }
    #[async_trait]
    impl ModelProvider for RecordingProvider {
        async fn chat_stream(&self, _m: Vec<ChatMessage>, _t: Vec<ToolDefinition>, o: ModelOptions)
            -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            self.models.lock().unwrap().push(o.model.clone());
            Ok(Box::pin(stream::iter(vec![Ok(ModelEvent::Done)])))
        }
        async fn chat(&self, _m: Vec<ChatMessage>, o: ModelOptions) -> Result<ChatResponse, ModelError> {
            self.models.lock().unwrap().push(o.model.clone());
            Ok(ChatResponse { content: String::new(), tool_calls: vec![] })
        }
    }

    struct MockSession { history: Mutex<Vec<ChatMessage>> }
    impl SessionStore for MockSession {
        fn create(&self) -> Result<SessionId, ModuleError> { Ok("test-session".into()) }
        fn append(&self, _sid: &SessionId, msg: ChatMessage) -> Result<(), ModuleError> {
            self.history.lock().unwrap().push(msg); Ok(())
        }
        fn get(&self, _sid: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(self.history.lock().unwrap().clone())
        }
        fn estimate_tokens(&self, _sid: &SessionId) -> Result<usize, ModuleError> { Ok(0) }
    }

    struct MockConfig;
    impl ConfigProvider for MockConfig {
        fn get_str(&self, _key: &str) -> Option<String> { None }
        fn get_int(&self, _key: &str) -> Option<i64> { None }
        fn providers(&self) -> Vec<(String, String)> { vec![] }
    }

    // design 4.3: config that serves a custom model.default_model.
    struct ModelConfig(&'static str);
    impl ConfigProvider for ModelConfig {
        fn get_str(&self, key: &str) -> Option<String> {
            if key == "model.default_model" { Some(self.0.to_string()) } else { None }
        }
        fn get_int(&self, _key: &str) -> Option<i64> { None }
        fn providers(&self) -> Vec<(String, String)> { vec![] }
    }

    struct MockPipeline;
    impl ContextPipeline for MockPipeline {
        fn assemble(&self, system: &str, history: &[ChatMessage], _tool_results: &[(ToolCall, String)]) -> Vec<ChatMessage> {
            let mut msgs = vec![ChatMessage::system(system)];
            msgs.extend_from_slice(history);
            msgs
        }
        fn reflux(&self, history: &mut Vec<ChatMessage>, tool_call: &ToolCall, result: String) {
            history.push(ChatMessage { role: Role::Assistant, content: String::new(), tool_call_id: None, tool_calls: vec![tool_call.clone()] });
            history.push(ChatMessage::tool_result(&tool_call.id, result));
        }
    }

    #[tokio::test]
    async fn run_turn_stream_yields_deltas() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let session = Arc::new(MockSession { history: Mutex::new(vec![]) });
        let config: Arc<dyn ConfigProvider> = Arc::new(MockConfig);
        let mock = Arc::new(MockProvider { chunks: vec!["Hello".into(), " world".into()] });
        providers.register("mock".into(), mock);

        let ctx = KernelContext { session, tools, providers, config };
        let agent_loop = AgentLoop::new(ctx, Arc::new(MockPipeline), "You are a helpful assistant.".into());
        let sid = agent_loop.create_session().unwrap();

        let mut rx = agent_loop.run_turn_stream(&sid, "hello");
        let mut collected = vec![];
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(text) => collected.push(text),
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("agent error: {e}"),
                _ => {}
            }
        }
        assert_eq!(collected.join(""), "Hello world");
    }

    #[tokio::test]
    async fn run_turn_uses_config_default_model() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let models = Arc::new(Mutex::new(Vec::new()));
        providers.register("mock".into(), Arc::new(RecordingProvider { models: models.clone() }));
        let session = Arc::new(MockSession { history: Mutex::new(vec![]) });
        let config: Arc<dyn ConfigProvider> = Arc::new(ModelConfig("opencode-go/kimi-k3"));

        let ctx = KernelContext { session, tools, providers, config };
        let agent_loop = AgentLoop::new(ctx, Arc::new(MockPipeline), "You are a helpful assistant.".into());
        let sid = agent_loop.create_session().unwrap();

        let mut rx = agent_loop.run_turn_stream(&sid, "hello");
        while let Some(event) = rx.recv().await {
            if let AgentEvent::Done { .. } = event { break; }
        }
        let recorded = models.lock().unwrap().clone();
        assert_eq!(recorded, vec!["opencode-go/kimi-k3".to_string()]);
    }
}
