//! Pure overlay builder for `bin/gen_opencode_catalog` (design D3).
//!
//! Wire format comes from models.dev, the machine-readable mirror of the official endpoint
//! table (`https://opencode.ai/docs/go/#api-endpoints`): a model-level `provider.npm`
//! override wins, otherwise the provider-level `npm` applies (opencode-go's default is
//! `@ai-sdk/openai-compatible`).
//!
//! Resolution order per live id: models.dev npm → `openai-compat` + `verified = false`.
//! An id models.dev does not list is never guessed (never invent a verified route).
//! Endpoint paths are owned by the transports (`WireFormat`), not by the catalog.

use crate::catalog::{CatalogEntry, WireFormat};

/// models.dev provider key holding the opencode-go catalog.
const DEV_PROVIDER: &str = "opencode-go";

/// Build catalog entries from the live id list plus models.dev metadata.
pub fn build_overlay(live_ids: &[String], models_dev: &serde_json::Value) -> Vec<CatalogEntry> {
    let mut ids: Vec<String> = live_ids.to_vec();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let (format, verified) = match dev_format(models_dev, &id) {
                Some(format) => (format, true),
                None => (WireFormat::OpenAiCompat, false),
            };
            let context_window = dev_context_window(models_dev, &id);
            CatalogEntry {
                id,
                format,
                verified,
                context_window,
            }
        })
        .collect()
}

/// models.dev wire format for a namespaced id: the model-level `provider.npm` override,
/// else the provider-level `npm`. `None` when models.dev does not list the id at all.
fn dev_format(dev: &serde_json::Value, id: &str) -> Option<WireFormat> {
    let provider = &dev[DEV_PROVIDER];
    let bare = id.strip_prefix("opencode-go/")?;
    let model = provider["models"].get(bare)?;
    let npm = model["provider"]["npm"]
        .as_str()
        .or_else(|| provider["npm"].as_str())?;
    npm_wire_format(npm)
}

/// models.dev `limit.context` for a namespaced id. `None` when models.dev does not list the
/// id or publishes no positive window — an unknown window stays unknown (design D2).
fn dev_context_window(dev: &serde_json::Value, id: &str) -> Option<u32> {
    let bare = id.strip_prefix("opencode-go/")?;
    let context = dev[DEV_PROVIDER]["models"].get(bare)?["limit"]["context"].as_u64()?;
    u32::try_from(context).ok().filter(|window| *window > 0)
}

/// AI SDK package → wire format; the package names are what the endpoint table publishes.
fn npm_wire_format(npm: &str) -> Option<WireFormat> {
    match npm {
        "@ai-sdk/anthropic" => Some(WireFormat::Anthropic),
        "@ai-sdk/openai" => Some(WireFormat::Responses),
        "@ai-sdk/openai-compatible" => Some(WireFormat::OpenAiCompat),
        _ => None,
    }
}

/// Serialize entries as the committed overlay JSON document.
pub fn overlay_json(base_url: &str, entries: &[CatalogEntry]) -> String {
    let models: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            // Unknown windows stay absent from the committed overlay: a guessed default would
            // silently shrink the request budget for models we know nothing about (design D2).
            let mut obj = serde_json::json!({
                "id": e.id,
                "format": format_name(e.format),
                "verified": e.verified,
            });
            if let Some(window) = e.context_window {
                obj["context_window"] = serde_json::json!(window);
            }
            obj
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "base_url": base_url,
        "models": models,
    }))
    .expect("overlay json")
}

/// Wire-format name used in the committed overlay (matches `WireFormat::from_str`).
pub fn format_name(format: WireFormat) -> &'static str {
    match format {
        WireFormat::OpenAiCompat => "openai-compat",
        WireFormat::Responses => "responses",
        WireFormat::Anthropic => "anthropic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev() -> serde_json::Value {
        serde_json::json!({"opencode-go": {
            "npm": "@ai-sdk/openai-compatible",
            "models": {
                "plain": {},
                "anthropic-x": {"provider": {"npm": "@ai-sdk/anthropic"}},
                "openai-x": {"provider": {"npm": "@ai-sdk/openai"}},
            }
        }})
    }

    #[test]
    fn model_override_wins_and_provider_default_is_inherited() {
        let entries = build_overlay(
            &[
                "opencode-go/plain".into(),
                "opencode-go/anthropic-x".into(),
                "opencode-go/openai-x".into(),
            ],
            &dev(),
        );
        let get = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            (
                get("opencode-go/plain").format,
                get("opencode-go/plain").verified
            ),
            (WireFormat::OpenAiCompat, true)
        );
        assert_eq!(
            (
                get("opencode-go/anthropic-x").format,
                get("opencode-go/anthropic-x").verified
            ),
            (WireFormat::Anthropic, true)
        );
        assert_eq!(
            (
                get("opencode-go/openai-x").format,
                get("opencode-go/openai-x").verified
            ),
            (WireFormat::Responses, true)
        );
    }

    #[test]
    fn id_absent_from_models_dev_is_never_guessed() {
        let entries = build_overlay(&["opencode-go/unknown".into()], &dev());
        assert_eq!(entries[0].format, WireFormat::OpenAiCompat);
        assert!(!entries[0].verified);
    }

    #[test]
    fn build_overlay_copies_the_models_dev_context_window() {
        let dev = serde_json::json!({"opencode-go": {"npm": "@ai-sdk/openai-compatible", "models": {
            "big": {"provider": {"npm": "@ai-sdk/openai-compatible"}, "limit": {"context": 1048576, "output": 65536}},
            "no-limit": {"provider": {"npm": "@ai-sdk/openai-compatible"}},
            "zero": {"provider": {"npm": "@ai-sdk/openai-compatible"}, "limit": {"context": 0}}
        }}});
        let ids: Vec<String> = ["big", "no-limit", "zero"]
            .iter()
            .map(|n| format!("opencode-go/{n}"))
            .collect();
        let entries = build_overlay(&ids, &dev);
        let window = |n: &str| {
            entries
                .iter()
                .find(|e| e.id == format!("opencode-go/{n}"))
                .unwrap()
                .context_window
        };
        assert_eq!(window("big"), Some(1_048_576));
        assert_eq!(window("no-limit"), None);
        assert_eq!(window("zero"), None);
    }

    #[test]
    fn unknown_npm_falls_back_to_unverified() {
        let dev =
            serde_json::json!({"opencode-go": {"npm": "@ai-sdk/whatever", "models": {"x": {}}}});
        let entries = build_overlay(&["opencode-go/x".into()], &dev);
        assert_eq!(entries[0].format, WireFormat::OpenAiCompat);
        assert!(!entries[0].verified);
    }

    #[test]
    fn duplicates_collapse() {
        let entries = build_overlay(
            &["opencode-go/plain".into(), "opencode-go/plain".into()],
            &dev(),
        );
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn overlay_json_roundtrips_through_from_json() {
        let entries = vec![CatalogEntry {
            id: "opencode-go/x".into(),
            format: WireFormat::Anthropic,
            verified: true,
            context_window: None,
        }];
        let json = overlay_json("https://example.test", &entries);
        let parsed = crate::catalog::ModelCatalog::from_json(&json);
        assert_eq!(parsed.base_url, "https://example.test");
        assert_eq!(
            parsed.lookup("opencode-go/x").unwrap().format,
            WireFormat::Anthropic
        );
        assert!(parsed.lookup("opencode-go/x").unwrap().verified);
    }
}
