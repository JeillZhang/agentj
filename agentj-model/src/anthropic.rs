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
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server).await;

        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("hi")],
            vec![],
            ModelOptions { model: "opencode-go/qwen3.8-max".into(), ..Default::default() },
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
            .mount(&server).await;

        let provider = AnthropicProvider::new(server.uri(), "sk-test".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("list files")],
            vec![],
            ModelOptions { model: "opencode-go/qwen3.8-max".into(), ..Default::default() },
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
        assert_eq!(tool_calls[0].id, "toolu_1");
        assert_eq!(tool_calls[0].name, "bash");
        assert_eq!(tool_calls[0].arguments["cmd"], "ls");
    }
}
