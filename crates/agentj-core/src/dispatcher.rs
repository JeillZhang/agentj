//! Sub-Agent Dispatcher — spawns parallel AgentLoop instances on cloned
//! KernelContexts, one fresh session each.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::*;
    use crate::module::*;
    use crate::types::*;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        chunks: Vec<String>,
    }
    #[async_trait]
    impl ModelProvider for MockProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let chunks: Vec<_> = self
                .chunks
                .iter()
                .map(|s| Ok(ModelEvent::Delta(s.clone())))
                .chain(std::iter::once(Ok(ModelEvent::Done)))
                .collect();
            Ok(Box::pin(stream::iter(chunks)))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            Ok(ChatResponse {
                content: "mock".into(),
                tool_calls: vec![],
            })
        }
    }

    struct MockSession {
        history: Mutex<Vec<ChatMessage>>,
    }
    impl SessionStore for MockSession {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok(uuid_like())
        }
        fn append(&self, _s: &SessionId, m: ChatMessage) -> Result<(), ModuleError> {
            self.history.lock().unwrap().push(m);
            Ok(())
        }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(self.history.lock().unwrap().clone())
        }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> {
            Ok(0)
        }
    }
    // In tests, SessionId = String — a counter keeps ids distinct across dispatches.
    static SESSION_SEQ: AtomicU64 = AtomicU64::new(0);
    fn uuid_like() -> String {
        format!("s{}", SESSION_SEQ.fetch_add(1, Ordering::SeqCst))
    }

    struct MockConfig;
    impl ConfigProvider for MockConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    struct MockPipeline;
    impl ContextPipeline for MockPipeline {
        fn assemble(
            &self,
            system: &str,
            history: &[ChatMessage],
            _t: &[(ToolCall, String)],
        ) -> Vec<ChatMessage> {
            let mut msgs = vec![ChatMessage::system(system)];
            msgs.extend_from_slice(history);
            msgs
        }
        fn reflux(&self, history: &mut Vec<ChatMessage>, tc: &ToolCall, result: String) {
            history.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![tc.clone()],
            });
            history.push(ChatMessage::tool_result(&tc.id, result));
        }
    }

    #[tokio::test]
    async fn dispatcher_runs_parallel_sub_agents() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockProvider {
                chunks: vec!["Hello".into(), " world".into()],
            }),
        );
        let ctx = KernelContext {
            session: Arc::new(MockSession {
                history: Mutex::new(vec![]),
            }),
            tools,
            providers,
            config: Arc::new(MockConfig),
        };
        let dispatcher = Dispatcher::new(ctx, Arc::new(MockPipeline), "You are helpful.".into());

        let a = dispatcher.dispatch(TaskSpec {
            prompt: "task a".into(),
            system_prompt: None,
        });
        let b = dispatcher.dispatch(TaskSpec {
            prompt: "task b".into(),
            system_prompt: None,
        });
        let (ra, rb) = tokio::join!(a, b);
        let ra = ra.unwrap().unwrap();
        let rb = rb.unwrap().unwrap();
        assert_eq!(ra.final_response, "Hello world");
        assert_eq!(rb.final_response, "Hello world");
        assert_ne!(ra.session_id, rb.session_id, "each sub-agent gets its own session");
    }
}