//! Pure overlay builder for `bin/gen_opencode_catalog` (design D3).
//!
//! Resolution order per live id: explicit `overrides` > models.dev npm hint >
//! `openai-compat` + `verified = false`. Never invents a verified route.

use std::collections::BTreeMap;

use crate::catalog::{CatalogEntry, WireFormat};

/// Build catalog entries from live ids plus metadata sources.
pub fn build_overlay(
    live_ids: &[String],
    models_dev: &serde_json::Value,
    overrides: &BTreeMap<String, WireFormat>,
) -> Vec<CatalogEntry> {
    let mut ids: Vec<String> = live_ids.to_vec();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let from_dev = dev_format(models_dev, &id);
            let (format, verified) = match (overrides.get(&id), from_dev) {
                (Some(f), _) => (*f, true),
                (None, Some(f)) => (f, true),
                (None, None) => (WireFormat::OpenAiCompat, false),
            };
            CatalogEntry {
                endpoint: endpoint_for(format).into(),
                id,
                format,
                verified,
            }
        })
        .collect()
}

/// models.dev npm package → wire format (design D3 metadata source).
fn dev_format(dev: &serde_json::Value, id: &str) -> Option<WireFormat> {
    let npm = dev["opencode-go"]["models"][id.strip_prefix("opencode-go/")?]["npm"].as_str()?;
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
            serde_json::json!({
                "id": e.id,
                "format": format_name(e.format),
                "endpoint": e.endpoint,
                "verified": e.verified,
            })
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

/// Endpoint path for a wire format.
pub fn endpoint_for(format: WireFormat) -> &'static str {
    match format {
        WireFormat::OpenAiCompat => "/v1/chat/completions",
        WireFormat::Responses => "/v1/responses",
        WireFormat::Anthropic => "/v1/messages",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn overrides_win_and_unknown_defaults_to_unverified_openai_compat() {
        let mut overrides = BTreeMap::new();
        overrides.insert("opencode-go/b".to_string(), WireFormat::Anthropic);
        let entries = build_overlay(
            &["opencode-go/a".into(), "opencode-go/b".into()],
            &serde_json::json!({}),
            &overrides,
        );
        let a = entries.iter().find(|e| e.id == "opencode-go/a").unwrap();
        let b = entries.iter().find(|e| e.id == "opencode-go/b").unwrap();
        assert_eq!((a.format, a.verified), (WireFormat::OpenAiCompat, false));
        assert_eq!((b.format, b.verified), (WireFormat::Anthropic, true));
    }

    #[test]
    fn models_dev_npm_maps_wire_format() {
        let dev =
            serde_json::json!({"opencode-go": {"models": {"c": {"npm": "@ai-sdk/anthropic"}}}});
        let entries = build_overlay(&["opencode-go/c".into()], &dev, &BTreeMap::new());
        assert_eq!(entries[0].format, WireFormat::Anthropic);
        assert!(entries[0].verified);
    }

    #[test]
    fn duplicates_collapse() {
        let entries = build_overlay(
            &["opencode-go/a".into(), "opencode-go/a".into()],
            &serde_json::json!({}),
            &BTreeMap::new(),
        );
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn overlay_json_roundtrips_through_from_json() {
        let entries = vec![CatalogEntry {
            id: "opencode-go/x".into(),
            format: WireFormat::Anthropic,
            endpoint: "/v1/messages".into(),
            verified: true,
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
