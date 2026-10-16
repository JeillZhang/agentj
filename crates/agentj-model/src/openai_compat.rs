use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

pub struct OpenAICompatibleProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: Client,
    session_header: Option<String>,
}

impl OpenAICompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            client: crate::http::client(),
            session_header: None,
        }
    }

    /// Same as [`Self::new`] but emits `header: <session_id>` when the turn carries one
    /// (opencode-go compliance header; design D4).
    pub fn with_session_header(
        base_url: String,
        api_key: String,
        model: String,
        header: String,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            client: crate::http::client(),
            session_header: Some(header),
        }
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
    ) -> Value {
        let model = if options.model.is_empty() {
            &self.model
        } else {
            &options.model
        };
        serde_json::json!({
            "model": model,
            "messages": map_messages(messages),
            "tools": tools.iter().map(|t| serde_json::json!({
                "type": "function",
                "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
            })).collect::<Vec<_>>(),
            "stream": true,
            "stream_options": { "include_usage": true },
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
        let body = self.request_body(messages, tools, options);
        let response = crate::retry::send(&body, |b| {
            self.with_session(
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(b),
                options,
            )
        })
        .await?;

        let byte_stream = response.bytes_stream();
        let mut parser = OpenAiChunkParser::new();
        let stream = crate::sse::sse_event_stream(byte_stream, move |line, out| {
            parser.push_line(line, out);
        });
        Ok(Box::pin(stream))
    }
}

fn map_messages(messages: &[ChatMessage]) -> Value {
    let mapped: Vec<Value> = messages
        .iter()
        .map(|m| match m.role {
            Role::Tool => serde_json::json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content
            }),
            Role::Assistant if !m.tool_calls.is_empty() => {
                let content = if m.content.is_empty() {
                    Value::Null
                } else {
                    Value::String(m.content.clone())
                };
                serde_json::json!({
                    "role": "assistant",
                    "content": content,
                    "tool_calls": m.tool_calls.iter().map(|tc| {
                        let args_str = match &tc.arguments {
                            Value::String(s) => s.clone(),
                            _ => serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        };
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": args_str
                            }
                        })
                    }).collect::<Vec<_>>()
                })
            }
            _ => {
                let role_str = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                serde_json::json!({
                    "role": role_str,
                    "content": m.content
                })
            }
        })
        .collect();
    Value::Array(mapped)
}

/// Stateful parser for OpenAI Chat Completions SSE (design 4.2: tool-call delta
/// assembly pulled into v0.1). Accumulates `delta.tool_calls` fragments keyed by
/// index; emits a complete ToolCall when a fragment carries both id and name and a
/// later fragment with the same index supplies arguments (name+id arrive first).
struct OpenAiChunkParser {
    pending: HashMap<usize, ToolCall>,
    saw_tool_finish: bool,
    done_emitted: bool,
}

impl OpenAiChunkParser {
    fn new() -> Self {
        Self {
            pending: HashMap::new(),
            saw_tool_finish: false,
            done_emitted: false,
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
            if !self.done_emitted {
                out.push(Ok(ModelEvent::Done));
                self.done_emitted = true;
            }
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
        if let Some(u) = json.get("usage").and_then(|u| u.as_object())
            && let (Some(p), Some(c)) = (
                u.get("prompt_tokens").and_then(|v| v.as_u64()),
                u.get("completion_tokens").and_then(|v| v.as_u64()),
            )
        {
            out.push(Ok(ModelEvent::Usage(TokenUsage {
                prompt_tokens: p,
                completion_tokens: c,
            })));
        }
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
            if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str())
                && finish == "tool_calls"
            {
                self.saw_tool_finish = true;
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
        let body = serde_json::json!({ "model": options.model, "messages": map_messages(&messages), "stream": false });
        let response = crate::retry::send(&body, |b| {
            self.with_session(
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(b),
                &options,
            )
        })
        .await?;
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

    #[tokio::test]
    async fn openai_compat_parses_streaming_usage_frame() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"index\":0}]}\n\n",
            "data: {\"id\":\"2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"3\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":7,\"total_tokens\":16}}\n\n",
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
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "test-model".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut events: Vec<ModelEvent> = vec![];
        while let Some(result) = stream.next().await {
            events.push(result.unwrap());
        }
        assert!(events.iter().any(|e| matches!(
            e,
            ModelEvent::Usage(u) if u.prompt_tokens == 9 && u.completion_tokens == 7
        )));
        let done_idx = events
            .iter()
            .position(|e| matches!(e, ModelEvent::Done))
            .unwrap();
        let usage_idx = events
            .iter()
            .position(|e| matches!(e, ModelEvent::Usage(_)))
            .unwrap();
        assert!(usage_idx < done_idx, "usage must be emitted before Done");
    }

    #[tokio::test]
    async fn openai_request_includes_stream_options_include_usage() {
        let provider = OpenAICompatibleProvider::new("http://x".into(), "k".into(), "m".into());
        let body = provider.request_body(
            &[ChatMessage::user("hi")],
            &[],
            &ModelOptions {
                model: "m".into(),
                ..Default::default()
            },
        );
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[tokio::test]
    async fn session_header_is_sent_only_when_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("data: [DONE]\n\n"))
            .mount(&server)
            .await;
        let opts = ModelOptions {
            model: "m".into(),
            session_id: Some("s1".into()),
            ..Default::default()
        };
        let with = OpenAICompatibleProvider::with_session_header(
            server.uri(),
            "k".into(),
            String::new(),
            "x-opencode-session".into(),
        );
        let _ = with.chat_stream(vec![], vec![], opts.clone()).await;
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs[0].headers.get("x-opencode-session").unwrap(), "s1");
        assert!(
            reqs[0]
                .headers
                .get("user-agent")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("agentj/")
        );

        let plain = OpenAICompatibleProvider::new(server.uri(), "k".into(), String::new());
        let _ = plain.chat_stream(vec![], vec![], opts).await;
        let reqs = server.received_requests().await.unwrap();
        assert!(reqs[1].headers.get("x-opencode-session").is_none());
    }

    #[tokio::test]
    async fn retry_after_header_is_parsed_for_openai_compat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "7"))
            .mount(&server)
            .await;
        let p = OpenAICompatibleProvider::new(server.uri(), "k".into(), String::new());
        let err = match p
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "m".into(),
                    ..Default::default()
                },
            )
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("expected a rate-limit error"),
        };
        assert!(
            matches!(err, ModelError::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(7))
        );
    }

    #[tokio::test]
    async fn auth_error_is_parsed_with_relogin_hint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string(
                r#"{"type":"error","error":{"type":"AuthError","message":"missing API key"}}"#,
            ))
            .mount(&server)
            .await;
        let p = OpenAICompatibleProvider::new(server.uri(), "k".into(), String::new());
        let err = match p
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "m".into(),
                    ..Default::default()
                },
            )
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("expected an auth error"),
        };
        let msg = err.to_string();
        assert!(msg.contains("AuthError: missing API key"), "{msg}");
        assert!(msg.contains("agentj auth login"), "{msg}");
    }
}
