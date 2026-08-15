use agentj_core::error::ModelError;
use agentj_core::types::*;

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
            .mount(&server).await;

        let provider = OpenAICompatibleProvider::new(
            server.uri(), "sk-test".into(), "test-model".into(),
        );
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("hi")],
            vec![],
            ModelOptions { model: "test-model".into(), ..Default::default() },
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
    async fn api_error_maps_to_model_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server).await;

        let provider = OpenAICompatibleProvider::new(server.uri(), "bad".into(), "m".into());
        let result = provider.chat_stream(vec![], vec![], ModelOptions::default()).await;
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
            .mount(&server).await;

        let provider = OpenAICompatibleProvider::new(server.uri(), "sk-test".into(), "test-model".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("list files")],
            vec![],
            ModelOptions { model: "test-model".into(), ..Default::default() },
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
            .mount(&server).await;

        let provider = OpenAICompatibleProvider::new(server.uri(), "sk-test".into(), "test-model".into());
        let resp = provider.chat(
            vec![ChatMessage::user("list files")],
            ModelOptions { model: "test-model".into(), ..Default::default() },
        ).await.unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_9");
        assert_eq!(resp.tool_calls[0].name, "bash");
        assert_eq!(resp.tool_calls[0].arguments["cmd"], "pwd");
    }
}
