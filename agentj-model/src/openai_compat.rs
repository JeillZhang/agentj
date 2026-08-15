use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

pub struct OpenAICompatibleProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAICompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            client: Client::new(),
        }
    }

    fn request_body(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
    ) -> Value {
        let model = if options.model.is_empty() {
            &self.model
        } else {
            &options.model
        };
        serde_json::json!({
            "model": model,
            "messages": messages,
            "tools": tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
            })).collect::<Vec<_>>(),
            "stream": true,
            "temperature": options.temperature.unwrap_or(0.7),
        })
    }

    async fn open_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .header("Content-Type", "application/json")
            .json(&self.request_body(messages, tools, options))
            .send()
            .await
            .map_err(|e| ModelError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        if status == 429 {
            return Err(ModelError::RateLimited { retry_after: None });
        }
        if status != 200 {
            let msg = response.text().await.unwrap_or_default();
            return Err(ModelError::Api {
                status,
                message: msg,
            });
        }

        let byte_stream = response.bytes_stream();
        let parser = Arc::new(Mutex::new(OpenAiChunkParser::new()));
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

/// Stateful parser for OpenAI Chat Completions SSE (design 4.2: tool-call delta
/// assembly pulled into v0.1). Accumulates `delta.tool_calls` fragments keyed by
/// index; emits a complete ToolCall when a fragment carries both id and name and a
/// later fragment with the same index supplies arguments (name+id arrive first).
struct OpenAiChunkParser {
    pending: HashMap<usize, ToolCall>,
    saw_tool_finish: bool,
}

impl OpenAiChunkParser {
    fn new() -> Self {
        Self {
            pending: HashMap::new(),
            saw_tool_finish: false,
        }
    }

    /// Parse one SSE line and push resulting ModelEvents into `out`.
    /// The whole response body may arrive as ONE bytes chunk (wiremock), so this
    /// must handle multiple `data:` lines per call and multi-chunk splits.
    fn push_line(&mut self, line: &str, out: &mut Vec<Result<ModelEvent, ModelError>>) {
        let line = line.trim();
        if !line.starts_with("data:") {
            return;
        }
        let data = line[5..].trim();
        if data == "[DONE]" {
            out.push(Ok(ModelEvent::Done));
            return;
        }
        if data.is_empty() {
            return;
        }
        let json: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                out.push(Err(ModelError::Stream(e.to_string())));
                return;
            }
        };
        let Some(choices) = json.get("choices").and_then(|c| c.as_array()) else {
            return;
        };
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(content) = delta.get("content").and_then(|v| v.as_str())
                    && !content.is_empty()
                {
                    out.push(Ok(ModelEvent::Delta(content.to_string())));
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let entry = self.pending.entry(idx).or_insert_with(|| ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: Value::Null,
                        });
                        if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                            entry.id = id.to_string();
                        }
                        if let Some(f) = tc.get("function") {
                            if let Some(name) = f.get("name").and_then(|v| v.as_str()) {
                                entry.name = name.to_string();
                            }
                            if let Some(args) = f.get("arguments").and_then(|v| v.as_str()) {
                                let mut accumulated = match &entry.arguments {
                                    Value::String(s) => s.clone(),
                                    _ => String::new(),
                                };
                                accumulated.push_str(args);
                                entry.arguments = match serde_json::from_str(&accumulated) {
                                    Ok(v) => v,
                                    Err(_) => Value::String(accumulated), // still partial JSON
                                };
                            }
                        }
                    }
                }
            }
            if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                if finish == "tool_calls" {
                    self.saw_tool_finish = true;
                } else if finish == "stop" || finish == "length" {
                    out.push(Ok(ModelEvent::Done));
                }
            }
        }
        // Emit assembled tool calls once the finish_reason=tool_calls marker is seen
        // (arguments for an index arrive in subsequent chunks after id+name).
        if self.saw_tool_finish && !self.pending.is_empty() {
            let mut idxs: Vec<usize> = self.pending.keys().copied().collect();
            idxs.sort_unstable();
            for idx in idxs {
                if let Some(tc) = self.pending.remove(&idx)
                    && !tc.id.is_empty()
                    && !tc.name.is_empty()
                {
                    out.push(Ok(ModelEvent::ToolCall(tc)));
                }
            }
            self.pending.clear();
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAICompatibleProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        const MAX_RETRIES: u32 = 3;
        let mut last_err = None;
        for attempt in 0..MAX_RETRIES {
            match self.open_stream(&messages, &tools, &options).await {
                Ok(stream) => return Ok(stream),
                Err(e @ ModelError::RateLimited { .. }) => {
                    sleep(Duration::from_secs(2u64.pow(attempt))).await;
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
        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .json(&serde_json::json!({ "model": options.model, "messages": messages, "stream": false }))
            .send().await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if status != 200 {
            let msg = response.text().await.unwrap_or_default();
            return Err(ModelError::Api {
                status,
                message: msg,
            });
        }
        let json: Value = response
            .json()
            .await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let message = &json["choices"][0]["message"];
        let content = message["content"].as_str().unwrap_or("").to_string();
        // design 4.2: non-streaming tool_calls parsed too (message.tool_calls → ToolCall)
        let tool_calls = message
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|tc| ToolCall {
                        id: tc["id"].as_str().unwrap_or("").to_string(),
                        name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                        arguments: tc["function"]["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(Value::Null),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(ChatResponse {
            content,
            tool_calls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn openai_compat_streams_sse() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"index\":0}]}\n\n",
            "data: {\"id\":\"2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\" world\"},\"index\":0}]}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider =
            OpenAICompatibleProvider::new(server.uri(), "sk-test".into(), "test-model".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "test-model".into(),
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
    async fn api_error_maps_to_model_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let provider = OpenAICompatibleProvider::new(server.uri(), "bad".into(), "m".into());
        let result = provider
            .chat_stream(vec![], vec![], ModelOptions::default())
            .await;
        assert!(matches!(result, Err(ModelError::Api { status: 401, .. })));
    }

    #[tokio::test]
    async fn openai_compat_assembles_tool_calls_from_deltas() {
        // design 4.2: tool-call delta assembly — id+name arrive first, arguments
        // stream in later fragments keyed by index; a complete ToolCall is emitted
        // once finish_reason=tool_calls is seen.
        let server = MockServer::start().await;
        let sse_body = concat!(
            // fragment 1: id + name (arguments empty)
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}}]},\"index\":0}]}\n\n",
            // fragment 2: arguments part 1
            "data: {\"id\":\"2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}]},\"index\":0}]}\n\n",
            // finish_reason=tool_calls
            "data: {\"id\":\"3\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider =
            OpenAICompatibleProvider::new(server.uri(), "sk-test".into(), "test-model".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("list files")],
                vec![],
                ModelOptions {
                    model: "test-model".into(),
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
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "bash");
        assert_eq!(tool_calls[0].arguments["cmd"], "ls");
    }

    #[tokio::test]
    async fn openai_compat_chat_parses_non_streaming_tool_calls() {
        // design 4.2: non-streaming chat() must also surface tool_calls
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "id": "1", "object": "chat.completion",
            "choices": [{ "index": 0, "message": {
                "role": "assistant", "content": null,
                "tool_calls": [{
                    "id": "call_9", "type": "function",
                    "function": { "name": "bash", "arguments": "{\"cmd\":\"pwd\"}" }
                }]
            }, "finish_reason": "tool_calls" }]
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(body)
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let provider =
            OpenAICompatibleProvider::new(server.uri(), "sk-test".into(), "test-model".into());
        let resp = provider
            .chat(
                vec![ChatMessage::user("list files")],
                ModelOptions {
                    model: "test-model".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_9");
        assert_eq!(resp.tool_calls[0].name, "bash");
        assert_eq!(resp.tool_calls[0].arguments["cmd"], "pwd");
    }
}
