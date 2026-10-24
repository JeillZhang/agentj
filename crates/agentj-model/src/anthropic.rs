use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Anthropic Messages API provider (design 4.1) — wire format for opencode-go's
/// `/v1/messages` models (MiniMax, Qwen). Same ModelProvider trait.
pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    client: Client,
    session_header: Option<String>,
}

impl AnthropicProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client: crate::http::client(),
            session_header: None,
        }
    }

    /// Same as [`Self::new`] but emits `header: <session_id>` when the turn carries one
    /// (opencode-go compliance header; design D4).
    pub fn with_session_header(base_url: String, api_key: String, header: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client: crate::http::client(),
            session_header: Some(header),
        }
    }

    /// Attach the auth headers for `/v1/messages`: `x-api-key` (Anthropic convention —
    /// the opencode-go gateway answers a bare `Bearer` with 401 `Missing API key`) plus
    /// `Authorization: Bearer` for proxies that only accept bearer tokens.
    fn auth_headers(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("x-api-key", &self.api_key)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("anthropic-version", "2023-06-01")
    }

    /// Attach the configured session header when both the header name and the turn's
    /// session id are present.
    fn with_session<'a>(
        &'a self,
        rb: reqwest::RequestBuilder,
        options: &'a ModelOptions,
    ) -> reqwest::RequestBuilder {
        let name = self.session_header.as_deref().unwrap_or("");
        match crate::http::session_header(name, options) {
            Some((name, value)) => rb.header(name, value),
            None => rb,
        }
    }

    fn request_body(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
        stream: bool,
    ) -> Value {
        // Anthropic: system is a top-level field, NOT a message role (design 4.1 wire mapping)
        let system: String = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let msgs: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| match m.role {
                Role::Tool => serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content,
                    }],
                }),
                Role::Assistant if !m.tool_calls.is_empty() => serde_json::json!({
                    "role": "assistant",
                    "content": m.tool_calls.iter().map(|tc| serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    })).collect::<Vec<_>>(),
                }),
                Role::Assistant => serde_json::json!({ "role": "assistant", "content": m.content }),
                _ => serde_json::json!({ "role": "user", "content": m.content }),
            })
            .collect();
        serde_json::json!({
            "model": options.model,
            "system": system,
            "messages": msgs,
            "tools": tools.iter().map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })).collect::<Vec<_>>(),
            "max_tokens": options.max_tokens.unwrap_or(4096),
            "stream": stream,
            "temperature": options.temperature.unwrap_or(0.7),
        })
    }
}

/// Accumulates a streaming tool_use block by content-block index (design 4.2).
struct PendingToolUse {
    id: String,
    name: String,
    args: String,
}

/// Stateful parser for Anthropic Messages SSE (design 4.2: tool-call parsing in v0.1).
struct AnthropicChunkParser {
    tool_blocks: HashMap<usize, PendingToolUse>,
    done_emitted: bool,
}

impl AnthropicChunkParser {
    fn new() -> Self {
        Self {
            tool_blocks: HashMap::new(),
            done_emitted: false,
        }
    }

    fn push_line(&mut self, line: &str, out: &mut Vec<Result<ModelEvent, ModelError>>) {
        crate::sse::push_line(self, line, out);
    }
}

impl crate::sse::SsePayload for AnthropicChunkParser {
    fn push_payload(&mut self, json: Value, out: &mut Vec<Result<ModelEvent, ModelError>>) {
        match json["type"].as_str().unwrap_or("") {
            "content_block_start" => {
                if json["content_block"]["type"].as_str() == Some("tool_use") {
                    let idx = json["index"].as_u64().unwrap_or(0) as usize;
                    let id = json["content_block"]["id"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let name = json["content_block"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    self.tool_blocks.insert(
                        idx,
                        PendingToolUse {
                            id,
                            name,
                            args: String::new(),
                        },
                    );
                }
            }
            "content_block_delta" => match json["delta"]["type"].as_str().unwrap_or("") {
                "text_delta" => {
                    // clippy collapsible_if: nested if → let-chain (edition 2024)
                    if let Some(text) = json["delta"]["text"].as_str()
                        && !text.is_empty()
                    {
                        out.push(Ok(ModelEvent::Delta(text.to_string())));
                    }
                }
                "input_json_delta" => {
                    let idx = json["index"].as_u64().unwrap_or(0) as usize;
                    // clippy collapsible_if: nested if-let → let-chain (edition 2024)
                    if let Some(tb) = self.tool_blocks.get_mut(&idx)
                        && let Some(args) = json["delta"]["partial_json"].as_str()
                    {
                        tb.args.push_str(args);
                    }
                }
                _ => {}
            },
            "content_block_stop" => {
                let idx = json["index"].as_u64().unwrap_or(0) as usize;
                if let Some(tb) = self.tool_blocks.remove(&idx) {
                    let arguments = serde_json::from_str(&tb.args).unwrap_or(Value::Null);
                    out.push(Ok(ModelEvent::ToolCall(ToolCall {
                        id: tb.id,
                        name: tb.name,
                        arguments,
                    })));
                }
            }
            "message_delta" => {
                if let Some(u) = json.get("usage").and_then(|u| u.as_object())
                    && let (Some(i), Some(o)) = (
                        u.get("input_tokens").and_then(|v| v.as_u64()),
                        u.get("output_tokens").and_then(|v| v.as_u64()),
                    )
                {
                    out.push(Ok(ModelEvent::Usage(TokenUsage {
                        prompt_tokens: i,
                        completion_tokens: o,
                    })));
                }
            }
            // clippy collapsible_match: arm-body if → match guard (done_emitted true
            // falls through to `_` below, same semantics as the spec's nested if)
            "message_stop" if !self.done_emitted => {
                out.push(Ok(ModelEvent::Done));
                self.done_emitted = true;
            }
            _ => {} // message_start / ping etc — ignorable protocol events
        }
    }

    fn done_emitted_mut(&mut self) -> &mut bool {
        &mut self.done_emitted
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        // design error 模型层: 429 (opencode-go usage quota) → RateLimited with
        // retry_after + exponential backoff, same policy as T8.
        const MAX_RETRIES: u32 = 3;
        let mut last_err = None;
        for attempt in 0..MAX_RETRIES {
            match self.open_stream(&messages, &tools, &options).await {
                Ok(stream) => return Ok(stream),
                Err(e @ ModelError::RateLimited { .. }) => {
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| ModelError::Stream("max retries exceeded".into())))
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = self.request_body(&messages, &[], &options, false);
        let response = crate::retry::send(&body, |b| {
            self.with_session(self.auth_headers(self.client.post(&url)).json(b), &options)
        })
        .await?;
        let json: Value = response
            .json()
            .await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let content: String = json["content"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|b| b["text"].as_str()).collect())
            .unwrap_or_default();
        Ok(ChatResponse {
            content,
            tool_calls: vec![],
        })
    }
}

impl AnthropicProvider {
    /// Open a streaming connection. 429 maps to RateLimited (parsing Retry-After so
    /// the caller can honour the server's backoff hint, design error 模型层).
    async fn open_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = self.request_body(messages, tools, options, true);
        let response = crate::retry::send(&body, |b| {
            self.with_session(self.auth_headers(self.client.post(&url)).json(b), options)
        })
        .await?;
        let byte_stream = response.bytes_stream();
        let mut parser = AnthropicChunkParser::new();
        let stream = crate::sse::sse_event_stream(byte_stream, move |line, out| {
            parser.push_line(line, out);
        });
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn anthropic_provider_streams_text_events() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-test"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "opencode-go/qwen3.8-max".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut chunks = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(text) => chunks.push(text),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(chunks.join(""), "Hello world");
    }

    #[tokio::test]
    async fn anthropic_provider_assembles_tool_use() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"bash\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("list files")],
                vec![],
                ModelOptions {
                    model: "opencode-go/qwen3.8-max".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut tool_calls = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::ToolCall(tc) => tool_calls.push(tc),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(tool_calls.len(), 1, "expected one assembled tool call");
        assert_eq!(tool_calls[0].id, "toolu_1");
        assert_eq!(tool_calls[0].name, "bash");
        assert_eq!(tool_calls[0].arguments["cmd"], "ls");
    }

    #[tokio::test]
    async fn anthropic_parses_usage_from_message_delta() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":30,\"output_tokens\":4}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "m".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut events: Vec<ModelEvent> = vec![];
        while let Some(result) = stream.next().await {
            events.push(result.unwrap());
        }
        let usage = events
            .iter()
            .find_map(|e| match e {
                ModelEvent::Usage(u) => Some(*u),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            usage,
            TokenUsage {
                prompt_tokens: 30,
                completion_tokens: 4
            }
        );
    }

    /// Models that deprecated `temperature` (e.g. `claude-haiku-5-5`) answer 400; the
    /// transport must retry once without the field instead of failing the turn (0.29.4).
    #[tokio::test]
    async fn anthropic_retries_without_temperature_when_rejected() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"type":"error","error":{"type":"invalid_request_error","message":"`temperature` is deprecated for this model."}}"#,
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "opencode-go/claude-haiku-5-5".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut chunks = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(text) => chunks.push(text),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(chunks.join(""), "Hi");

        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.len(), 2, "exactly one retry");
        let first = String::from_utf8_lossy(&reqs[0].body);
        let second = String::from_utf8_lossy(&reqs[1].body);
        assert!(first.contains("temperature"), "default request keeps it");
        assert!(!second.contains("temperature"), "retry drops it");
    }
}
