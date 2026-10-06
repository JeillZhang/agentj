use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use reqwest::Client;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

/// Ollama local-model provider (design v0.3+ 模型扩展). Talks NDJSON over POST /api/chat.
pub struct OllamaProvider {
    base_url: String,
    /// Reserved for ProviderFactory wiring parity; options.model is authoritative at call time.
    #[allow(dead_code)]
    model: String,
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            client: crate::http::client(),
        }
    }

    async fn open_stream(
        &self,
        messages: &[ChatMessage],
        options: &ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let url = format!("{}/api/chat", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "model": options.model,
                "messages": messages,
                "stream": true,
            }))
            .send()
            .await
            .map_err(|e| ModelError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if status != 200 {
            let msg = response.text().await.unwrap_or_default();
            return Err(ModelError::Api {
                status,
                message: msg,
            });
        }
        // bytes_stream() is a Stream, not AsyncRead — StreamReader bridges it to the
        // line reader (tokio-util); each NDJSON line becomes one ModelEvent.
        let byte_stream = response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other));
        let reader = BufReader::new(StreamReader::new(byte_stream));
        // State: (reader, done_seen, pending_usage, pending_done, terminated).
        // - done_seen: a done:true line was emitted (or a truncation error was).
        // - pending_usage: a done:true line carried eval counts; emit Usage next.
        // - pending_done: a done:true line carried content/usage; emit Done next.
        // - terminated: a truncation error was emitted; end the stream.
        let stream = stream::unfold(
            (reader, false, None, false, false),
            |(mut reader, mut done_seen, mut pending_usage, pending_done, mut terminated)| async move {
                if terminated {
                    return None;
                }
                if let Some(usage) = pending_usage.take() {
                    return Some((
                        Ok(ModelEvent::Usage(usage)),
                        (reader, done_seen, None, true, false),
                    ));
                }
                if pending_done {
                    return Some((Ok(ModelEvent::Done), (reader, true, None, false, false)));
                }
                let mut line = String::new();
                loop {
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            // EOF before done:true → truncated stream, never silent.
                            if !done_seen {
                                terminated = true;
                                return Some((
                                    Err(ModelError::Stream("stream ended before done:true".into())),
                                    (reader, done_seen, None, false, terminated),
                                ));
                            }
                            return None; // normal end after done
                        }
                        Ok(_) => {}
                        Err(e) => {
                            // Transport error before done:true → truncated stream.
                            if !done_seen {
                                terminated = true;
                                return Some((
                                    Err(ModelError::Stream(format!(
                                        "stream error before done:true: {e}"
                                    ))),
                                    (reader, done_seen, None, false, terminated),
                                ));
                            }
                            return None;
                        }
                    }
                    let trimmed = line.trim().to_string();
                    line.clear();
                    let json: Value = match serde_json::from_str(&trimmed) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let content = json["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let done = json.get("done").and_then(|d| d.as_bool()).unwrap_or(false);
                    if done {
                        done_seen = true;
                        let prompt = json.get("prompt_eval_count").and_then(|v| v.as_u64());
                        let eval = json.get("eval_count").and_then(|v| v.as_u64());
                        let usage_opt = match (prompt, eval) {
                            (None, None) => None,
                            _ => Some(TokenUsage {
                                prompt_tokens: prompt.unwrap_or(0),
                                completion_tokens: eval.unwrap_or(0),
                            }),
                        };
                        if !content.is_empty() {
                            // BUG-010: emit delta before done for done-line content.
                            // If usage present: Delta -> Usage -> Done;
                            // else: Delta -> Done.
                            if let Some(u) = usage_opt {
                                return Some((
                                    Ok(ModelEvent::Delta(content)),
                                    (reader, done_seen, Some(u), false, false),
                                ));
                            }
                            return Some((
                                Ok(ModelEvent::Delta(content)),
                                (reader, done_seen, None, true, false),
                            ));
                        }
                        if let Some(u) = usage_opt {
                            return Some((
                                Ok(ModelEvent::Usage(u)),
                                (reader, done_seen, None, true, false),
                            ));
                        }
                        return Some((
                            Ok(ModelEvent::Done),
                            (reader, done_seen, None, false, false),
                        ));
                    }
                    if !content.is_empty() {
                        return Some((
                            Ok(ModelEvent::Delta(content)),
                            (reader, done_seen, None, false, false),
                        ));
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        self.open_stream(&messages, &options).await
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        let url = format!("{}/api/chat", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "model": options.model,
                "messages": messages,
                "stream": false,
            }))
            .send()
            .await
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
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(ChatResponse {
            content,
            tool_calls: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn ollama_provider_streams_sse() {
        let server = MockServer::start().await;
        let sse_body = concat!(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n",
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\" world\"},\"done\":false}\n",
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n"
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "application/x-ndjson"),
            )
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri(), "llama3".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "llama3".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut chunks = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(t) => chunks.push(t),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(chunks.join(""), "Hello world");
    }

    #[tokio::test]
    async fn done_line_with_content_emits_delta_before_done() {
        let server = MockServer::start().await;
        let body = concat!(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n",
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\" world\"},\"done\":true}\n"
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .insert_header("content-type", "application/x-ndjson"),
            )
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri(), "llama3".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "llama3".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut chunks = vec![];
        let mut saw_done = false;
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(t) => chunks.push(t),
                ModelEvent::Done => {
                    saw_done = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_done, "done:true line must yield a Done event");
        assert_eq!(chunks.join(""), "Hello world");
    }

    #[tokio::test]
    async fn truncated_stream_without_done_emits_error() {
        let server = MockServer::start().await;
        // Body ends without a done:true line — simulates a mid-stream transport
        // interruption (EOF before completion).
        let body = "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n";
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .insert_header("content-type", "application/x-ndjson"),
            )
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri(), "llama3".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "llama3".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut chunks = vec![];
        let mut saw_error = false;
        while let Some(result) = stream.next().await {
            match result {
                Ok(ModelEvent::Delta(t)) => chunks.push(t),
                Ok(ModelEvent::Done) => break,
                Err(e) => {
                    saw_error = true;
                    assert!(
                        matches!(e, ModelError::Stream(_)),
                        "truncation must surface as a stream error, got: {e}"
                    );
                    break;
                }
                _ => {}
            }
        }
        assert!(
            saw_error,
            "truncated stream must emit an error, not end silently"
        );
        assert_eq!(chunks.join(""), "Hello");
    }

    #[tokio::test]
    async fn api_error_maps_to_model_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri(), "m".into());
        let result = provider
            .chat_stream(vec![], vec![], ModelOptions::default())
            .await;
        assert!(matches!(result, Err(ModelError::Api { status: 404, .. })));
    }

    #[tokio::test]
    async fn ollama_parses_usage_from_done_line() {
        let server = MockServer::start().await;
        let body = concat!(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n",
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\" world\"},\"done\":true,\"prompt_eval_count\":50,\"eval_count\":11}\n"
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(body)
                    .insert_header("content-type", "application/x-ndjson"),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri(), "llama3".into());
        let mut stream = provider
            .chat_stream(
                vec![ChatMessage::user("hi")],
                vec![],
                ModelOptions {
                    model: "llama3".into(),
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
                prompt_tokens: 50,
                completion_tokens: 11
            }
        );
        let usage_idx = events
            .iter()
            .position(|e| matches!(e, ModelEvent::Usage(_)))
            .unwrap();
        let done_idx = events
            .iter()
            .position(|e| matches!(e, ModelEvent::Done))
            .unwrap();
        assert!(usage_idx < done_idx);
    }
}
