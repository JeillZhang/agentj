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
                ResponseTemplate::new(200).set_body_string(sse_body)
                    .insert_header("content-type", "application/x-ndjson"),
            )
            .mount(&server).await;

        let provider = OllamaProvider::new(server.uri(), "llama3".into());
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("hi")], vec![],
            ModelOptions { model: "llama3".into(), ..Default::default() },
        ).await.unwrap();
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
    async fn api_error_maps_to_model_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server).await;
        let provider = OllamaProvider::new(server.uri(), "m".into());
        let result = provider.chat_stream(vec![], vec![], ModelOptions::default()).await;
        assert!(matches!(result, Err(ModelError::Api { status: 404, .. })));
    }
}
