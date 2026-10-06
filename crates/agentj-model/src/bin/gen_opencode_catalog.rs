//! Regenerate `models/opencode-go.json` from the live `/zen/go/v1/models` list plus
//! models.dev metadata (design D3). Network-dependent — run manually or from CI refresh;
//! `--check` asserts the committed overlay is not stale.
//!
//! Usage: `cargo run -p agentj-model --bin gen_opencode_catalog [-- --check]`

use agentj_model::catalog::WireFormat;

const BASE: &str = "https://opencode.ai/zen/go";

/// Committed overlay path resolved from the crate root, so the bin can run from any cwd.
fn overlay_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/opencode-go.json")
}

#[tokio::main]
async fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let live = agentj_model::catalog_source::fetch_live_models(BASE, None)
        .await
        .expect("live /models must be reachable for generation");
    let dev: serde_json::Value = reqwest::get("https://models.dev/api.json")
        .await
        .expect("models.dev api")
        .json()
        .await
        .expect("models.dev json");
    let entries = agentj_model::catalog_gen::build_overlay(&live, &dev, &overrides());
    let rendered = agentj_model::catalog_gen::overlay_json(BASE, &entries);
    if check {
        assert_eq!(
            rendered.trim(),
            std::fs::read_to_string(overlay_path())
                .expect("read committed overlay")
                .trim(),
            "committed overlay is stale; re-run without --check"
        );
        return;
    }
    let path = overlay_path();
    std::fs::write(&path, format!("{rendered}\n")).expect("write overlay");
    println!("wrote {} ({} models)", path.display(), entries.len());
}

/// Seeded from the pre-task committed `models/opencode-go.json` — the curated
/// `id -> format` pairs validated against the Go endpoints table. New live ids fall
/// through to models.dev, then to openai-compat + verified=false.
/// Never invent a verified route: an unverified entry is honest, a wrong verified one is not.
fn overrides() -> std::collections::BTreeMap<String, WireFormat> {
    use WireFormat::{Anthropic, OpenAiCompat, Responses};
    let mut m = std::collections::BTreeMap::new();
    // responses
    m.insert("opencode-go/grok-4.5".into(), Responses);
    m.insert("opencode-go/gpt-5.6-luna".into(), Responses);
    // openai-compat
    m.insert("opencode-go/glm-5.3".into(), OpenAiCompat);
    m.insert("opencode-go/glm-5.2".into(), OpenAiCompat);
    m.insert("opencode-go/glm-5.1".into(), OpenAiCompat);
    m.insert("opencode-go/kimi-k3".into(), OpenAiCompat);
    m.insert("opencode-go/kimi-k2.7-code".into(), OpenAiCompat);
    m.insert("opencode-go/kimi-k2.6".into(), OpenAiCompat);
    m.insert("opencode-go/deepseek-v4-pro".into(), OpenAiCompat);
    m.insert("opencode-go/deepseek-v4-flash".into(), OpenAiCompat);
    m.insert("opencode-go/mimo-v2.5".into(), OpenAiCompat);
    m.insert("opencode-go/mimo-v2.5-pro".into(), OpenAiCompat);
    m.insert("opencode-go/hy3".into(), OpenAiCompat);
    // anthropic
    m.insert("opencode-go/minimax-m3".into(), Anthropic);
    m.insert("opencode-go/minimax-m2.7".into(), Anthropic);
    m.insert("opencode-go/minimax-m2.5".into(), Anthropic);
    m.insert("opencode-go/qwen3.8-max".into(), Anthropic);
    m.insert("opencode-go/qwen3.7-max".into(), Anthropic);
    m.insert("opencode-go/qwen3.7-plus".into(), Anthropic);
    m.insert("opencode-go/qwen3.6-plus".into(), Anthropic);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_seed_covers_the_previously_verified_routes() {
        let o = overrides();
        // These route shapes were validated by the earlier manual curation; losing any of
        // them would silently downgrade a verified route to the openai-compat default.
        assert_eq!(
            o.get("opencode-go/kimi-k3"),
            Some(&WireFormat::OpenAiCompat)
        );
        assert_eq!(o.get("opencode-go/grok-4.5"), Some(&WireFormat::Responses));
        assert_eq!(
            o.get("opencode-go/minimax-m3"),
            Some(&WireFormat::Anthropic)
        );
    }

    #[test]
    fn overrides_seed_covers_every_committed_route() {
        let committed = agentj_model::catalog::ModelCatalog::builtin();
        for id in committed.ids() {
            assert!(
                overrides().contains_key(&id),
                "{id} is in the committed overlay but not seeded in overrides()"
            );
        }
    }
}
