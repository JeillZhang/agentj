use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::{
    ChatMessage, ModelEvent, ModelOptions, TokenUsage, ToolCall, ToolDefinition,
};
use async_trait::async_trait;
use futures::stream::{self, BoxStream};

use crate::anthropic::AnthropicProvider;
use crate::openai_compat::OpenAICompatibleProvider;
use crate::opencode_go::OpenCodeGoProvider;
use crate::responses::ResponsesProvider;

struct MockStateful {
    calls: AtomicUsize,
}

impl MockStateful {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for MockStateful {
    async fn chat_stream(
        &self,
        _m: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let events: Vec<Result<ModelEvent, ModelError>> = if n == 0 {
            vec![Ok(ModelEvent::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "curl http://example.com"}),
            }))]
        } else {
            vec![
                Ok(ModelEvent::Delta("done".into())),
                Ok(ModelEvent::Usage(TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                })),
                Ok(ModelEvent::Done),
            ]
        };
        Ok(Box::pin(stream::iter(events)))
    }

    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<agentj_core::types::ChatResponse, ModelError> {
        Ok(agentj_core::types::ChatResponse {
            content: "mock response".into(),
            tool_calls: vec![],
        })
    }
}

/// E2E `mock-workbench` script (T25): call 1 emits a `todo` submission with two
/// items, call 2 a `write` of `e2e-artifact.txt`, call 3 a text delta + Done.
struct MockWorkbench {
    calls: AtomicUsize,
}

impl MockWorkbench {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn script(call: usize) -> Vec<Result<ModelEvent, ModelError>> {
        match call {
            0 => vec![Ok(ModelEvent::ToolCall(ToolCall {
                id: "call_todo".into(),
                name: "todo".into(),
                arguments: serde_json::json!({"items": [
                    {"content": "write e2e-artifact.txt", "status": "in_progress"},
                    {"content": "verify todo/file_change frames", "status": "pending"}
                ]}),
            }))],
            1 => vec![Ok(ModelEvent::ToolCall(ToolCall {
                id: "call_write".into(),
                name: "write".into(),
                arguments: serde_json::json!({
                    "path": "e2e-artifact.txt",
                    "content": "e2e-artifact\n",
                }),
            }))],
            _ => vec![
                Ok(ModelEvent::Delta("workbench done".into())),
                Ok(ModelEvent::Done),
            ],
        }
    }
}

#[async_trait]
impl ModelProvider for MockWorkbench {
    async fn chat_stream(
        &self,
        _m: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter(Self::script(n))))
    }

    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<agentj_core::types::ChatResponse, ModelError> {
        Ok(agentj_core::types::ChatResponse {
            content: "mock response".into(),
            tool_calls: vec![],
        })
    }
}

/// Single instantiation entry for model providers (design 4.3). The config `kind`
/// field ("openai-compat" | "responses" | "anthropic" | "opencode-go") selects the
/// concrete provider; unknown kinds are a config error (no silent fallback).
pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create(
        kind: &str,
        name: &str,
        base_url: &str,
        api_key: String,
    ) -> Result<Arc<dyn ModelProvider>, ModelError> {
        let provider: Arc<dyn ModelProvider> = match kind {
            "openai-compat" => Arc::new(OpenAICompatibleProvider::new(
                base_url.to_string(),
                api_key,
                name.to_string(),
            )),
            "responses" => Arc::new(ResponsesProvider::new(base_url.to_string(), api_key)),
            "anthropic" => Arc::new(AnthropicProvider::new(base_url.to_string(), api_key)),
            "opencode-go" => Arc::new(OpenCodeGoProvider::new(api_key)),
            // "mock" scripts a scripted tool call + reply — used by the E2E
            // permission test (T11). The command falls through to mode=ask: it
            // must not match the fixture's deny rule (deny short-circuits).
            "mock" => Arc::new(MockStateful::new()),
            // "mock-workbench" scripts the T25 E2E scenario: todo → write →
            // delta/done, producing todo + file_change `$/progress` frames.
            "mock-workbench" => Arc::new(MockWorkbench::new()),
            other => {
                return Err(ModelError::Stream(format!(
                    "unknown provider kind: {other}"
                )));
            }
        };
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::types::ModelOptions;
    use futures::StreamExt;

    #[test]
    fn factory_creates_provider_per_kind() {
        let openai = ProviderFactory::create(
            "openai-compat",
            "deepseek-chat",
            "https://api.deepseek.com",
            "k".into(),
        );
        assert!(openai.is_ok());
        let responses = ProviderFactory::create(
            "responses",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(responses.is_ok());
        let anthropic = ProviderFactory::create(
            "anthropic",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(anthropic.is_ok());
        let og = ProviderFactory::create(
            "opencode-go",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        );
        assert!(og.is_ok());
    }

    #[test]
    fn factory_creates_mock_provider() {
        let mock = ProviderFactory::create("mock", "mock", "mock://local", String::new());
        assert!(mock.is_ok());
    }

    #[tokio::test]
    async fn factory_creates_mock_workbench() {
        let provider = ProviderFactory::create(
            "mock-workbench",
            "mock-workbench",
            "mock://local",
            String::new(),
        )
        .unwrap();
        let first = scripted_events(&provider).await;
        match first.as_slice() {
            [ModelEvent::ToolCall(call)] => {
                assert_eq!(call.name, "todo");
                let items = call.arguments["items"].as_array().unwrap();
                assert_eq!(items.len(), 2, "the todo call must script two items");
                assert_eq!(items[0]["status"], "in_progress");
                assert_eq!(items[1]["status"], "pending");
            }
            other => panic!("call 1 must be one todo tool call, got {other:?}"),
        }
        let second = scripted_events(&provider).await;
        match second.as_slice() {
            [ModelEvent::ToolCall(call)] => {
                assert_eq!(call.name, "write");
                assert_eq!(call.arguments["path"], "e2e-artifact.txt");
            }
            other => panic!("call 2 must be one write tool call, got {other:?}"),
        }
        let third = scripted_events(&provider).await;
        assert!(
            matches!(third.as_slice(), [ModelEvent::Delta(_), ModelEvent::Done]),
            "call 3 must be a text delta + Done, got {third:?}"
        );
    }

    async fn scripted_events(provider: &Arc<dyn ModelProvider>) -> Vec<ModelEvent> {
        let mut stream = provider
            .chat_stream(vec![], vec![], ModelOptions::default())
            .await
            .unwrap();
        let mut events = vec![];
        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }
        events
    }

    #[test]
    fn factory_rejects_unknown_kind() {
        let result = ProviderFactory::create("bogus", "x", "http://localhost", "k".into());
        assert!(matches!(result, Err(ModelError::Stream(_))));
    }

    #[tokio::test]
    async fn factory_opencode_go_routes_unknown_model() {
        let provider = ProviderFactory::create(
            "opencode-go",
            "opencode-go",
            "https://opencode.ai/zen/go",
            "k".into(),
        )
        .unwrap();
        let result = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/no-such".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(result, Err(ModelError::UnknownModel { .. })));
    }
}
