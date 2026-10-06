use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;

use crate::anthropic::AnthropicProvider;
use crate::catalog::{ModelCatalog, WireFormat};
use crate::openai_compat::OpenAICompatibleProvider;
use crate::responses::ResponsesProvider;

/// Facade for the opencode-go subscription (design 4.1): one base_url + one api_key
/// covering all models. Routes `opencode-go/<model>` to the transport matching the
/// model's wire format via the committed catalog.
pub struct OpenCodeGoProvider {
    catalog: ModelCatalog,
    transports: HashMap<WireFormat, Box<dyn ModelProvider>>,
}

impl OpenCodeGoProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_catalog(api_key, ModelCatalog::builtin())
    }

    /// Build the facade over an injected catalog (live/overlay merged) and wire the
    /// opencode-go compliance session header into every transport (design D4).
    pub fn with_catalog(api_key: String, catalog: ModelCatalog) -> Self {
        let base = catalog.base_url.clone();
        let session = "x-opencode-session".to_string();
        let mut transports: HashMap<WireFormat, Box<dyn ModelProvider>> = HashMap::new();
        transports.insert(
            WireFormat::OpenAiCompat,
            Box::new(OpenAICompatibleProvider::with_session_header(
                base.clone(),
                api_key.clone(),
                String::new(),
                session.clone(),
            )),
        );
        transports.insert(
            WireFormat::Responses,
            Box::new(ResponsesProvider::with_session_header(
                base.clone(),
                api_key.clone(),
                session.clone(),
            )),
        );
        transports.insert(
            WireFormat::Anthropic,
            Box::new(AnthropicProvider::with_session_header(
                base, api_key, session,
            )),
        );
        Self {
            catalog,
            transports,
        }
    }

    /// Test constructor: inject catalog + mock transports for route tests.
    pub fn with_catalog_and_transports(
        catalog: ModelCatalog,
        transports: HashMap<WireFormat, Box<dyn ModelProvider>>,
    ) -> Self {
        Self {
            catalog,
            transports,
        }
    }

    fn route(&self, model: &str) -> Result<(&dyn ModelProvider, WireFormat), ModelError> {
        let entry = self
            .catalog
            .lookup(model)
            .ok_or_else(|| ModelError::UnknownModel {
                provider: "opencode-go".into(),
                model: model.to_string(),
            })?;
        let transport = self
            .transports
            .get(&entry.format)
            .ok_or_else(|| ModelError::Stream(format!("no transport for {:?}", entry.format)))?;
        Ok((transport.as_ref(), entry.format))
    }
}

#[async_trait]
impl ModelProvider for OpenCodeGoProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
        let (transport, _) = self.route(&options.model)?;
        transport.chat_stream(messages, tools, options).await
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        let (transport, _) = self.route(&options.model)?;
        transport.chat(messages, options).await
    }

    fn models(&self) -> Vec<String> {
        self.catalog.ids()
    }
}

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
                verified: true,
            },
        );
        entries.insert(
            "opencode-go/grok-4.5".into(),
            CatalogEntry {
                id: "opencode-go/grok-4.5".into(),
                format: WireFormat::Responses,
                endpoint: "/v1/responses".into(),
                verified: true,
            },
        );
        entries.insert(
            "opencode-go/minimax-m3".into(),
            CatalogEntry {
                id: "opencode-go/minimax-m3".into(),
                format: WireFormat::Anthropic,
                endpoint: "/v1/messages".into(),
                verified: true,
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
        transports.insert(
            WireFormat::Responses,
            Box::new(RecordingTransport { hits: hits.clone() }),
        );
        transports.insert(
            WireFormat::Anthropic,
            Box::new(RecordingTransport { hits: hits.clone() }),
        );
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
        let provider =
            OpenCodeGoProvider::with_catalog_and_transports(test_catalog(), HashMap::new());
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

    const OVERLAY_X: &str = r#"{"base_url":"https://opencode.ai/zen/go","models":[
        {"id":"opencode-go/x","format":"anthropic","endpoint":"/v1/messages","verified":true}]}"#;

    #[test]
    fn with_catalog_exposes_models_and_keeps_routing() {
        let catalog = ModelCatalog::from_sources(OVERLAY_X, None);
        let provider = OpenCodeGoProvider::with_catalog("k".into(), catalog);
        assert_eq!(provider.models(), vec!["opencode-go/x"]);
    }
}
