#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ModelError;
    use agentj_core::module::ModelProvider;
    use agentj_core::types::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn mock_provider_returns_scripted_events() {
        let events = vec![
            Ok(ModelEvent::Delta("Hello".into())),
            Ok(ModelEvent::Delta(" world".into())),
            Ok(ModelEvent::Done),
        ];
        let provider = MockModelProvider::new(events);
        let mut stream = provider.chat_stream(
            vec![ChatMessage::user("hi")], vec![], ModelOptions::default(),
        ).await.unwrap();

        let mut chunks: Vec<String> = vec![];
        while let Some(result) = stream.next().await {
            match result.unwrap() {
                ModelEvent::Delta(text) => chunks.push(text),
                ModelEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(chunks.join(""), "Hello world");
    }
}
