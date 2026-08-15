use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use reqwest::Client;
use serde_json::Value;

/// OpenAI Responses API provider (design 4.1) — wire format for opencode-go's
/// `/v1/responses` models (Grok, GPT-5.6 Luna). Same ModelProvider trait.
pub struct ResponsesProvider {
    base_url: String,
    api_key: String,
    client: Client,
}

impl ResponsesProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string(), api_key, client: Client::new() }
    }

    fn request_body(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
        stream: bool,
    ) -> Value {
        serde_json::json!({
            "model": options.model,
            "input": map_input(messages),
            "tools": tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema,
            })).collect::<Vec<_>>(),
            "stream": stream,
            "temperature": options.temperature.unwrap_or(0.7),
            "max_output_tokens": options.max_tokens.unwrap_or(4096),
        })
    }
}

/// Map agentj ChatMessage history to the Responses API `input` items array.
/// System → developer role; tool results → function_call_output items; assistant
/// tool_calls → assistant output items (design 4.1 wire mapping).
fn map_input(messages: &[ChatMessage]) -> Value {
    let mut input = Vec::new();
    for m in messages {
        match m.role {
            Role::Tool => {
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": m.tool_call_id.clone().unwrap_or_default(),
                    "output": m.content,
                }));
            }
            Role::Assistant if !m.tool_calls.is_empty() => {
                input.push(serde_json::json!({
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": m.content }],
                    "output": m.tool_calls.iter().map(|tc| serde_json::json!({
                        "type": "function_call",
                        "call_id": tc.id,
                        "name": tc.name,
                        "arguments": tc.arguments,
                    })).collect::<Vec<_>>(),
                }));
            }
            _ => {
                let role = match m.role { Role::System => "developer", Role::Assistant => "assistant", _ => "user" };
                input.push(serde_json::json!({
                    "role": role,
                    "content": [{ "type": "input_text", "text": m.content }],
                }));
            }
        }
    }
    serde_json::json!(input)
}

/// Accumulates a streaming function call by item_id (design 4.2).
struct PendingCall { call_id: String, name: String, args: String }

/// Stateful parser for Responses API SSE (design 4.2: tool-call parsing in v0.1).
struct ResponsesChunkParser {
    pending: HashMap<String, PendingCall>,
    done_emitted: bool,
}

impl ResponsesChunkParser {
    fn new() -> Self { Self { pending: HashMap::new(), done_emitted: false } }

    fn push_line(&mut self, line: &str, out: &mut Vec<Result<ModelEvent, ModelError>>) {
        let line = line.trim();
        if !line.starts_with("data:") { return; }
        let data = line[5..].trim();
        if data == "[DONE]" {
            if !self.done_emitted { out.push(Ok(ModelEvent::Done)); self.done_emitted = true; }
            return;
        }
        if data.is_empty() { return; }
        let json: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => { out.push(Err(ModelError::Stream(e.to_string()))); return; }
        };
        match json["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                // clippy collapsible_if: nested if → let-chain (edition 2024)
                if let Some(text) = json["delta"]["text"].as_str()
                    && !text.is_empty()
                {
                    out.push(Ok(ModelEvent::Delta(text.to_string())));
                }
            }
            "response.output_item.added" => {
                if json["item"]["type"].as_str() == Some("function_call") {
                    let item_id = json["item"]["id"].as_str().unwrap_or("").to_string();
                    let call_id = json["item"]["call_id"].as_str().unwrap_or("").to_string();
                    let name = json["item"]["name"].as_str().unwrap_or("").to_string();
                    self.pending.insert(item_id, PendingCall { call_id, name, args: String::new() });
                }
            }
            "response.function_call_arguments.delta" => {
                // clippy collapsible_if: nested if-let → let-chain (edition 2024)
                let item_id = json["item_id"].as_str().unwrap_or("");
                if let Some(pc) = self.pending.get_mut(item_id)
                    && let Some(args) = json["delta"]["arguments"].as_str()
                {
                    pc.args.push_str(args);
                }
            }
            "response.output_item.done" => {
                if json["item"]["type"].as_str() == Some("function_call") {
                    let item_id = json["item"]["id"].as_str().unwrap_or("").to_string();
                    if let Some(pc) = self.pending.remove(&item_id) {
                        let arguments = serde_json::from_str(&pc.args).unwrap_or(Value::Null);
                        out.push(Ok(ModelEvent::ToolCall(ToolCall { id: pc.call_id, name: pc.name, arguments })));
                    }
                }
            }
            // clippy collapsible_match: arm-body if → match guard (done_emitted true
            // falls through to `_` below, same semantics as the spec's nested if)
            "response.completed" if !self.done_emitted => {
                out.push(Ok(ModelEvent::Done));
                self.done_emitted = true;
            }
            _ => {} // response.created etc — ignorable protocol events
        }
    }
}

#[async_trait]
impl ModelProvider for ResponsesProvider {
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
        let url = format!("{}/v1/responses", self.base_url);
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .json(&self.request_body(&messages, &[], &options, false))
            .send().await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if status != 200 {
            let msg = response.text().await.unwrap_or_default();
            return Err(ModelError::Api { status, message: msg });
        }
        let json: Value = response.json().await.map_err(|e| ModelError::Network(e.to_string()))?;
        let output_text: Vec<&str> = json["output"].as_array().map(|arr| arr.iter()
            .filter(|item| item["type"] == "message")
            .filter_map(|item| item["content"][0]["text"].as_str())
            .collect()).unwrap_or_default();
        Ok(ChatResponse { content: output_text.join(""), tool_calls: vec![] })
    }
}

impl ResponsesProvider {
    /// Open a streaming connection. 429 maps to RateLimited (parsing Retry-After so
    /// the caller can honour the server's backoff hint, design error 模型层).
    async fn open_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let url = format!("{}/v1/responses", self.base_url);
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .json(&self.request_body(messages, tools, options, true))
            .send().await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if status == 429 {
            let retry_after = response.headers().get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(ModelError::RateLimited { retry_after });
        }
        if status != 200 {
            let msg = response.text().await.unwrap_or_default();
            return Err(ModelError::Api { status, message: msg });
        }
        let byte_stream = response.bytes_stream();
        let parser = Arc::new(Mutex::new(ResponsesChunkParser::new()));
        let stream = byte_stream.flat_map(move |result| {
            let bytes = match result {
                Ok(b) => b,
                Err(e) => return stream::iter(vec![Err(ModelError::Network(e.to_string()))]),
            };
            let mut events = Vec::new();
            {
                let mut parser = parser.lock().unwrap();
                for line in String::from_utf8_lossy(&bytes).lines() {
                    parser.push_line(line, &mut events);
                }
            }
            stream::iter(events)
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
    async fn responses_provider_streams_text_events() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":{\"text\":\"Hello\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":{\"text\":\" world\"}}\n\n",
            "data: {\"type\":\"response.completed\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server).await;

        let provider = ResponsesProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("hi")],
            vec![],
            ModelOptions { model: "opencode-go/grok-4.5".into(), ..Default::default() },
        ).await.unwrap();

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
    async fn responses_provider_assembles_function_call() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":1,\"delta\":{\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server).await;

        let provider = ResponsesProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("list files")],
            vec![],
            ModelOptions { model: "opencode-go/grok-4.5".into(), ..Default::default() },
        ).await.unwrap();

        let mut tool_calls = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::ToolCall(tc) => tool_calls.push(tc),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(tool_calls.len(), 1, "expected one assembled tool call");
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "bash");
        assert_eq!(tool_calls[0].arguments["cmd"], "ls");
    }
}
