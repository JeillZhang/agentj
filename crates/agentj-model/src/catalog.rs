use std::collections::HashMap;

use serde::Deserialize;

/// Canonical opencode-go API base — the origin the endpoint table
/// (`https://opencode.ai/docs/go/#api-endpoints`) publishes.
pub const OPENCODE_GO_BASE: &str = "https://opencode.ai/zen/go";

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
    /// False for live-discovered ids whose wire format was synthesized (design D3).
    pub verified: bool,
}

/// Committed opencode-go model catalog (design 4.1): model id → wire format/endpoint.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    pub base_url: String,
    pub entries: HashMap<String, CatalogEntry>,
}

impl ModelCatalog {
    /// Load the generated artifact at compile time. Regenerate with
    /// `cargo run -p agentj-model --bin gen_opencode_catalog` (design D3); never hand-edit —
    /// the file is a snapshot of `GET /v1/models` ∩ the models.dev wire-format metadata, and
    /// the catalog tests fail if the routes below drift.
    pub fn builtin() -> Self {
        Self::from_json(include_str!("../data/opencode-go.json"))
    }

    pub fn from_json(json: &str) -> Self {
        #[derive(Deserialize)]
        struct RawEntry {
            id: String,
            format: String,
            #[serde(default = "default_true")]
            verified: bool,
        }
        #[derive(Deserialize)]
        struct RawCatalog {
            base_url: String,
            models: Vec<RawEntry>,
        }
        let raw: RawCatalog =
            serde_json::from_str(json).unwrap_or_else(|e| panic!("invalid catalog JSON: {e}"));
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
                            verified: m.verified,
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

    /// All model ids, sorted.
    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }

    /// Ids whose wire format could not be confirmed (synthesized as openai-compat).
    pub fn unverified_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .entries
            .values()
            .filter(|e| !e.verified)
            .map(|e| e.id.clone())
            .collect();
        v.sort();
        v
    }

    /// Merge a committed overlay with live-discovered ids (design D3).
    pub fn from_sources(overlay_json: &str, live_ids: Option<Vec<String>>) -> Self {
        Self::from_overlay_and_live(Self::from_json(overlay_json), live_ids)
    }

    /// Single embed site for callers that already hold a catalog (`ModelCatalog::builtin()`).
    ///
    /// A successful-but-empty live result is treated as unavailable and falls back to the
    /// overlay (never "all models removed").
    pub fn from_overlay_and_live(overlay: ModelCatalog, live_ids: Option<Vec<String>>) -> Self {
        let Some(mut live) = live_ids.filter(|ids| !ids.is_empty()) else {
            return overlay;
        };
        live.sort();
        live.dedup();
        let mut entries = HashMap::new();
        for id in live {
            let entry = overlay.lookup(&id).cloned().unwrap_or(CatalogEntry {
                id: id.clone(),
                format: WireFormat::OpenAiCompat,
                verified: false,
            });
            entries.insert(id, entry);
        }
        Self {
            base_url: overlay.base_url,
            entries,
        }
    }
}

fn default_true() -> bool {
    true
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
                matches!(
                    entry.format,
                    WireFormat::OpenAiCompat | WireFormat::Responses | WireFormat::Anthropic
                ),
                "unknown format in catalog"
            );
        }
    }

    #[test]
    fn catalog_entries_unique_and_valid() {
        let catalog = ModelCatalog::builtin();
        let mut ids = std::collections::HashSet::new();
        for entry in catalog.entries.values() {
            assert!(
                ids.insert(entry.id.clone()),
                "duplicate model id: {}",
                entry.id
            );
            assert!(
                entry.id.starts_with("opencode-go/"),
                "id must be namespaced: {}",
                entry.id
            );
        }
    }

    #[test]
    fn catalog_lookup_routes_formats() {
        let catalog = ModelCatalog::builtin();
        // Routes mirror the official endpoint table; a regeneration that loses one of these
        // would silently send the model to the wrong endpoint.
        for (id, format) in [
            ("opencode-go/kimi-k3", WireFormat::OpenAiCompat),
            ("opencode-go/longcat-2.0", WireFormat::OpenAiCompat),
            ("opencode-go/grok-4.5", WireFormat::Responses),
            ("opencode-go/grok-4.7", WireFormat::Responses),
            ("opencode-go/gpt-6-luna", WireFormat::Responses),
            (
                "opencode-go/muse-spark-1.2-contributor",
                WireFormat::Responses,
            ),
            ("opencode-go/minimax-m3", WireFormat::Anthropic),
            ("opencode-go/claude-haiku-5-5", WireFormat::Anthropic),
            ("opencode-go/qwen3.8-flash", WireFormat::Anthropic),
        ] {
            let entry = catalog.lookup(id).expect(id);
            assert_eq!(entry.format, format, "{id}");
            assert!(entry.verified, "{id} must be resolved from models.dev");
        }
        assert!(catalog.lookup("opencode-go/no-such-model").is_none());
    }

    #[test]
    fn catalog_builtin_marks_unresolved_ids_unverified() {
        let catalog = ModelCatalog::builtin();
        // Ids `GET /v1/models` still advertises but models.dev does not cover.
        for id in ["opencode-go/glm-5", "opencode-go/omen-alpha"] {
            let entry = catalog.lookup(id).expect(id);
            assert_eq!(entry.format, WireFormat::OpenAiCompat);
            assert!(!entry.verified, "{id} must stay unverified");
        }
    }
}

#[cfg(test)]
mod from_sources_tests {
    use super::*;

    const OVERLAY: &str = r#"{"base_url":"https://opencode.ai/zen/go","models":[
        {"id":"opencode-go/b","format":"anthropic","verified":true}]}"#;

    #[test]
    fn unknown_live_id_is_synthesized_as_unverified_openai_compat() {
        let cat = ModelCatalog::from_sources(
            OVERLAY,
            Some(vec!["opencode-go/b".into(), "opencode-go/a".into()]),
        );
        assert_eq!(cat.ids(), vec!["opencode-go/a", "opencode-go/b"]);
        let a = cat.lookup("opencode-go/a").unwrap();
        assert_eq!(a.format, WireFormat::OpenAiCompat);
        assert!(!a.verified);
        assert_eq!(cat.unverified_ids(), vec!["opencode-go/a"]);
        assert!(cat.lookup("opencode-go/b").unwrap().verified);
    }

    #[test]
    fn live_none_falls_back_to_overlay_only() {
        let cat = ModelCatalog::from_sources(OVERLAY, None);
        assert_eq!(cat.ids(), vec!["opencode-go/b"]);
        assert!(cat.unverified_ids().is_empty());
    }

    #[test]
    fn overlay_only_entries_are_dropped_when_live_succeeds() {
        let cat = ModelCatalog::from_sources(OVERLAY, Some(vec!["opencode-go/c".into()]));
        assert_eq!(cat.ids(), vec!["opencode-go/c"]);
    }

    #[test]
    fn empty_live_result_falls_back_to_overlay() {
        let cat = ModelCatalog::from_sources(OVERLAY, Some(vec![]));
        assert_eq!(cat.ids(), vec!["opencode-go/b"]);
    }
}
