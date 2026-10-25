use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;

use crate::anthropic::AnthropicProvider;
use crate::catalog::{ModelCatalog, WireFormat};
use crate::catalog_source::bare_id;
use crate::openai_compat::OpenAICompatibleProvider;
use crate::responses::ResponsesProvider;
#[cfg(test)]
use agentj_core::sync::lock_or_recover;

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

    /// Resolve the catalog entry and the transport that speaks its wire format, returning the
    /// transport plus the **bare** upstream id (the agentj `opencode-go/` namespace stripped).
    fn route(&self, model: &str) -> Result<(&dyn ModelProvider, String), ModelError> {
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
        Ok((transport.as_ref(), bare_id(model).to_string()))
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
        let (transport, model) = self.route(&options.model)?;
        transport
            .chat_stream(messages, tools, ModelOptions { model, ..options })
            .await
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ModelOptions,
    ) -> Result<ChatResponse, ModelError> {
        let (transport, model) = self.route(&options.model)?;
        transport
            .chat(messages, ModelOptions { model, ..options })
            .await
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

    /// Records which transport label was invoked (route probe) and the model id it received.
    struct RecordingTransport {
        label: WireFormat,
        hits: Arc<Mutex<Vec<WireFormat>>>,
        models: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait]
    impl ModelProvider for RecordingTransport {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            lock_or_recover(&self.hits).push(self.label);
            lock_or_recover(&self.models).push(o.model);
            Ok(Box::pin(stream::iter(vec![Ok(ModelEvent::Done)])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            lock_or_recover(&self.hits).push(self.label);
            lock_or_recover(&self.models).push(o.model);
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    fn recording_transports(
        hits: &Arc<Mutex<Vec<WireFormat>>>,
        models: &Arc<Mutex<Vec<String>>>,
    ) -> HashMap<WireFormat, Box<dyn ModelProvider>> {
        let mut transports: HashMap<WireFormat, Box<dyn ModelProvider>> = HashMap::new();
        for label in [
            WireFormat::OpenAiCompat,
            WireFormat::Responses,
            WireFormat::Anthropic,
        ] {
            transports.insert(
                label,
                Box::new(RecordingTransport {
                    label,
                    hits: hits.clone(),
                    models: models.clone(),
                }),
            );
        }
        transports
    }

    fn test_catalog() -> ModelCatalog {
        let mut entries = HashMap::new();
        entries.insert(
            "opencode-go/kimi-k3".into(),
            CatalogEntry {
                id: "opencode-go/kimi-k3".into(),
                format: WireFormat::OpenAiCompat,
                verified: true,
                context_window: None,
            },
        );
        entries.insert(
            "opencode-go/grok-4.5".into(),
            CatalogEntry {
                id: "opencode-go/grok-4.5".into(),
                format: WireFormat::Responses,
                verified: true,
                context_window: None,
            },
        );
        entries.insert(
            "opencode-go/minimax-m3".into(),
            CatalogEntry {
                id: "opencode-go/minimax-m3".into(),
                format: WireFormat::Anthropic,
                verified: true,
                context_window: None,
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
        let models = Arc::new(Mutex::new(Vec::new()));
        let provider = OpenCodeGoProvider::with_catalog_and_transports(
            test_catalog(),
            recording_transports(&hits, &models),
        );

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

        let recorded = lock_or_recover(&hits).clone();
        assert_eq!(
            recorded,
            vec![
                WireFormat::OpenAiCompat,
                WireFormat::Responses,
                WireFormat::Anthropic,
            ]
        );
    }

    /// The upstream gateway keys on the bare id; the `opencode-go/` namespace is an
    /// agentj-internal routing prefix and must never leak onto the wire (0.29.4).
    #[tokio::test]
    async fn facade_forwards_bare_model_ids() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let models = Arc::new(Mutex::new(Vec::new()));
        let provider = OpenCodeGoProvider::with_catalog_and_transports(
            test_catalog(),
            recording_transports(&hits, &models),
        );
        for id in [
            "opencode-go/kimi-k3",
            "opencode-go/grok-4.5",
            "opencode-go/minimax-m3",
        ] {
            let _ = provider
                .chat_stream(
                    vec![],
                    vec![],
                    ModelOptions {
                        model: id.into(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            lock_or_recover(&models).clone(),
            vec!["kimi-k3", "grok-4.5", "minimax-m3"]
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
        {"id":"opencode-go/x","format":"anthropic","verified":true}]}"#;

    #[test]
    fn with_catalog_exposes_models_and_keeps_routing() {
        let catalog = ModelCatalog::from_sources(OVERLAY_X, None);
        let provider = OpenCodeGoProvider::with_catalog("k".into(), catalog);
        assert_eq!(provider.models(), vec!["opencode-go/x"]);
    }
}
