#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogEntry, ModelCatalog, WireFormat};
    use futures::stream;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Records which format a transport was invoked for (route test probe).
    struct RecordingTransport {
        hits: Arc<Mutex<Vec<WireFormat>>>,
    }
    #[async_trait]
    impl ModelProvider for RecordingTransport {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            self.hits.lock().unwrap().push(format_of(&o.model));
            Ok(Box::pin(stream::iter(vec![Ok(ModelEvent::Done)])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            self.hits.lock().unwrap().push(format_of(&o.model));
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    fn format_of(model: &str) -> WireFormat {
        match model {
            "opencode-go/grok-4.5" => WireFormat::Responses,
            "opencode-go/minimax-m3" => WireFormat::Anthropic,
            _ => WireFormat::OpenAiCompat,
        }
    }

    fn test_catalog() -> ModelCatalog {
        let mut entries = HashMap::new();
        entries.insert(
            "opencode-go/kimi-k3".into(),
            CatalogEntry {
                id: "opencode-go/kimi-k3".into(),
                format: WireFormat::OpenAiCompat,
                endpoint: "/v1/chat/completions".into(),
            },
        );
        entries.insert(
            "opencode-go/grok-4.5".into(),
            CatalogEntry {
                id: "opencode-go/grok-4.5".into(),
                format: WireFormat::Responses,
                endpoint: "/v1/responses".into(),
            },
        );
        entries.insert(
            "opencode-go/minimax-m3".into(),
            CatalogEntry {
                id: "opencode-go/minimax-m3".into(),
                format: WireFormat::Anthropic,
                endpoint: "/v1/messages".into(),
            },
        );
        ModelCatalog {
            base_url: "https://opencode.ai/zen/go".into(),
            entries,
        }
    }

    #[tokio::test]
    async fn facade_routes_by_catalog() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let mut transports = HashMap::new();
        transports.insert(
            WireFormat::OpenAiCompat,
            Box::new(RecordingTransport { hits: hits.clone() }) as Box<dyn ModelProvider>,
        );
        transports.insert(WireFormat::Responses, Box::new(RecordingTransport { hits: hits.clone() }));
        transports.insert(WireFormat::Anthropic, Box::new(RecordingTransport { hits: hits.clone() }));
        let provider = OpenCodeGoProvider::with_catalog_and_transports(test_catalog(), transports);

        let _ = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/kimi-k3".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let _ = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/grok-4.5".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let _ = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/minimax-m3".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let recorded = hits.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![
                WireFormat::OpenAiCompat,
                WireFormat::Responses,
                WireFormat::Anthropic,
            ]
        );
    }

    #[tokio::test]
    async fn facade_unknown_model_returns_error() {
        let provider = OpenCodeGoProvider::with_catalog_and_transports(test_catalog(), HashMap::new());
        let result = provider
            .chat_stream(
                vec![],
                vec![],
                ModelOptions {
                    model: "opencode-go/no-such".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(
            result,
            Err(ModelError::UnknownModel { model, .. }) if model == "opencode-go/no-such"
        ));
    }
}
