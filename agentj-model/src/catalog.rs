use std::collections::HashMap;

use serde::Deserialize;

/// Wire format a model speaks. Used by the OpenCodeGoProvider facade to route to the
/// correct transport (design 4.1/4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireFormat {
    OpenAiCompat,
    Responses,
    Anthropic,
}

impl WireFormat {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai-compat" => Some(Self::OpenAiCompat),
            "responses" => Some(Self::Responses),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub id: String,
    pub format: WireFormat,
    pub endpoint: String,
}

/// Committed opencode-go model catalog (design 4.1): model id → wire format/endpoint.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    pub base_url: String,
    pub entries: HashMap<String, CatalogEntry>,
}

impl ModelCatalog {
    /// Load the committed artifact at compile time (drift gate: any manual edit that
    /// breaks the format fails the T22 validation tests).
    pub fn builtin() -> Self {
        Self::from_json(include_str!("../../models/opencode-go.json"))
    }

    pub fn from_json(json: &str) -> Self {
        #[derive(Deserialize)]
        struct RawEntry {
            id: String,
            format: String,
            endpoint: String,
        }
        #[derive(Deserialize)]
        struct RawCatalog {
            base_url: String,
            models: Vec<RawEntry>,
        }
        let raw: RawCatalog = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("invalid catalog JSON: {e}"));
        let entries = raw
            .models
            .into_iter()
            .filter_map(|m| {
                WireFormat::from_str(&m.format).map(|format| {
                    (
                        m.id.clone(),
                        CatalogEntry {
                            id: m.id,
                            format,
                            endpoint: m.endpoint,
                        },
                    )
                })
            })
            .collect();
        Self {
            base_url: raw.base_url,
            entries,
        }
    }

    pub fn lookup(&self, model: &str) -> Option<&CatalogEntry> {
        self.entries.get(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_builtin_loads() {
        let catalog = ModelCatalog::builtin();
        assert!(!catalog.entries.is_empty(), "catalog must not be empty");
        for entry in catalog.entries.values() {
            assert!(
                matches!(entry.format, WireFormat::OpenAiCompat | WireFormat::Responses | WireFormat::Anthropic),
                "unknown format in catalog"
            );
        }
    }

    #[test]
    fn catalog_entries_unique_and_valid() {
        let catalog = ModelCatalog::builtin();
        let mut ids = std::collections::HashSet::new();
        for entry in catalog.entries.values() {
            assert!(ids.insert(entry.id.clone()), "duplicate model id: {}", entry.id);
            assert!(!entry.endpoint.is_empty(), "empty endpoint for {}", entry.id);
            assert!(entry.id.starts_with("opencode-go/"), "id must be namespaced: {}", entry.id);
        }
    }

    #[test]
    fn catalog_lookup_routes_formats() {
        let catalog = ModelCatalog::builtin();
        assert_eq!(catalog.lookup("opencode-go/kimi-k3").unwrap().format, WireFormat::OpenAiCompat);
        assert_eq!(catalog.lookup("opencode-go/grok-4.5").unwrap().format, WireFormat::Responses);
        assert_eq!(catalog.lookup("opencode-go/minimax-m3").unwrap().format, WireFormat::Anthropic);
        assert!(catalog.lookup("opencode-go/no-such-model").is_none());
    }
}
