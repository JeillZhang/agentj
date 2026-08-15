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
