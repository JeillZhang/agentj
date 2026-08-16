use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use std::sync::Mutex;

pub struct MockModelProvider {
    script: Mutex<Vec<Result<ModelEvent, ModelError>>>,
}

impl MockModelProvider {
    pub fn new(script: Vec<Result<ModelEvent, ModelError>>) -> Self {
        Self {
            script: Mutex::new(script),
        }
    }
}

#[async_trait]
impl ModelProvider for MockModelProvider {
    async fn chat_stream(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
        _options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let mut guard = self
            .script
            .lock()
            .map_err(|_| ModelError::Stream("mock script mutex poisoned".into()))?;
        let events = std::mem::take(&mut *guard);
        Ok(Box::pin(stream::iter(events)))
    }

    async fn chat(
        &self,
        _messages: Vec<ChatMessage>,
        _options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        Ok(ChatResponse {
            content: "mock response".into(),
            tool_calls: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn mock_provider_returns_scripted_events() {
        let events = vec![
            Ok(ModelEvent::Delta("Hello".into())),
            Ok(ModelEvent::Delta(" world".into())),
            Ok(ModelEvent::Done),
        ];
        let provider = MockModelProvider::new(events);
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions::default(),
            )
            .await
            .unwrap();

        let mut chunks: Vec<String> = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(text) => chunks.push(text),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(chunks.join(""), "Hello world");
    }
}
