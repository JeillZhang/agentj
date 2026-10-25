use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use reqwest::Client;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

/// Ollama local-model provider (design v0.3+ 模型扩展). Talks NDJSON over POST /api/chat.
pub struct OllamaProvider {
    base_url: String,
    /// Reserved for ProviderFactory wiring parity; options.model is authoritative at call time.
    #[allow(dead_code)]
    model: String,
    client: Client,
}

/// One decoded NDJSON frame of the `/api/chat` streaming vocabulary.
struct NdjsonFrame {
    content: String,
    done: bool,
    usage: Option<TokenUsage>,
}

impl NdjsonFrame {
    /// Decode one line; `None` for blank/unparsable frames (the reader skips them).
    fn parse(line: &str) -> Option<Self> {
        let json: Value = serde_json::from_str(line.trim()).ok()?;
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let done = json.get("done").and_then(|d| d.as_bool()).unwrap_or(false);
        let prompt = json.get("prompt_eval_count").and_then(|v| v.as_u64());
        let eval = json.get("eval_count").and_then(|v| v.as_u64());
        let usage = match (prompt, eval) {
            (None, None) => None,
            _ => Some(TokenUsage {
                prompt_tokens: prompt.unwrap_or(0),
                completion_tokens: eval.unwrap_or(0),
            }),
        };
        Some(Self {
            content,
            done,
            usage,
        })
    }
}

/// NDJSON stream state machine for one `POST /api/chat` response.
///
/// - `done_seen`: a `done:true` frame (or a truncation error) was consumed.
/// - `pending_usage`: a `done:true` frame carried eval counts; emit `Usage` next.
/// - `pending_done`: a `done:true` frame carried content/usage; emit `Done` next.
/// - `terminated`: a truncation error was emitted; the stream ends here.
struct NdjsonStream<R> {
    reader: BufReader<R>,
    done_seen: bool,
    pending_usage: Option<TokenUsage>,
    pending_done: bool,
    terminated: bool,
}

impl<R: AsyncBufRead + Unpin> NdjsonStream<R> {
    fn new(reader: BufReader<R>) -> Self {
        Self {
            reader,
            done_seen: false,
            pending_usage: None,
            pending_done: false,
            terminated: false,
        }
    }

    /// One `stream::unfold` step.
    async fn next_event(&mut self) -> Option<Result<ModelEvent, ModelError>> {
        if self.terminated {
            return None;
        }
        if let Some(pending) = self.take_pending() {
            return Some(pending);
        }
        self.read_event().await
    }

    /// Drain the events buffered behind a `done:true` frame (Usage before Done).
    fn take_pending(&mut self) -> Option<Result<ModelEvent, ModelError>> {
        if let Some(usage) = self.pending_usage.take() {
            self.pending_done = true;
            return Some(Ok(ModelEvent::Usage(usage)));
        }
        if self.pending_done {
            self.pending_done = false;
            self.done_seen = true;
            return Some(Ok(ModelEvent::Done));
        }
        None
    }

    /// Read frames until one of them yields an event (or the stream ends).
    async fn read_event(&mut self) -> Option<Result<ModelEvent, ModelError>> {
        loop {
            let line = match self.read_line().await {
                Ok(Some(line)) => line,
                Ok(None) => return None,
                Err(e) => return Some(Err(e)),
            };
            let Some(frame) = NdjsonFrame::parse(&line) else {
                continue;
            };
            if frame.done {
                self.done_seen = true;
                return Some(self.done_event(frame));
            }
            if !frame.content.is_empty() {
                return Some(Ok(ModelEvent::Delta(frame.content)));
            }
        }
    }

    /// Read one line; EOF/transport failures before `done:true` become truncation errors.
    async fn read_line(&mut self) -> Result<Option<String>, ModelError> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) if !self.done_seen => {
                self.terminated = true;
                Err(ModelError::Stream("stream ended before done:true".into()))
            }
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(line)),
            Err(e) if !self.done_seen => {
                self.terminated = true;
                Err(ModelError::Stream(format!(
                    "stream error before done:true: {e}"
                )))
            }
            Err(_) => Ok(None),
        }
    }

    /// Event a `done:true` frame stands for (BUG-010: delta before done).
    fn done_event(&mut self, frame: NdjsonFrame) -> Result<ModelEvent, ModelError> {
        let has_usage = frame.usage.is_some();
        if !frame.content.is_empty() {
            self.pending_usage = frame.usage;
            self.pending_done = !has_usage;
            return Ok(ModelEvent::Delta(frame.content));
        }
        match frame.usage {
            Some(usage) => {
                self.pending_done = true;
                Ok(ModelEvent::Usage(usage))
            }
            None => Ok(ModelEvent::Done),
        }
    }
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
        let response = self.send_chat_request(messages, options).await?;
        // bytes_stream() is a Stream, not AsyncRead — StreamReader bridges it to the
        // line reader (tokio-util); each NDJSON line becomes one ModelEvent.
        let byte_stream = response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other));
        let reader = BufReader::new(StreamReader::new(byte_stream));
        let stream = stream::unfold(NdjsonStream::new(reader), |mut state| async move {
            let event = state.next_event().await?;
            Some((event, state))
        });
        Ok(Box::pin(stream))
    }

    /// POST `/api/chat` in streaming mode and reject non-200 responses verbatim.
    async fn send_chat_request(
        &self,
        messages: &[ChatMessage],
        options: &ModelOptions,
    ) -> Result<reqwest::Response, ModelError> {
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
        Ok(response)
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
