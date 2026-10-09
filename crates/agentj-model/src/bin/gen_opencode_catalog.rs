//! Regenerate `crates/agentj-model/data/opencode-go.json` from the live `/zen/go/v1/models`
//! list plus models.dev wire-format metadata (design D3). Network-dependent — run manually or
//! from CI refresh; `--check` asserts the committed snapshot is not stale.
//!
//! Usage: `cargo run -p agentj-model --bin gen_opencode_catalog [-- --check]`

use agentj_model::catalog::OPENCODE_GO_BASE;

/// Generated snapshot path resolved from the crate root, so the bin can run from any cwd.
fn overlay_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/opencode-go.json")
}

#[tokio::main]
async fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let live = agentj_model::catalog_source::fetch_live_models(OPENCODE_GO_BASE, None)
        .await
        .expect("live /models must be reachable for generation");
    let dev: serde_json::Value = reqwest::get("https://models.dev/api.json")
        .await
        .expect("models.dev api")
        .json()
        .await
        .expect("models.dev json");
    let entries = agentj_model::catalog_gen::build_overlay(&live, &dev);
    let rendered = agentj_model::catalog_gen::overlay_json(OPENCODE_GO_BASE, &entries);
    if check {
        assert_eq!(
            rendered.trim(),
            std::fs::read_to_string(overlay_path())
                .expect("read committed snapshot")
                .trim(),
            "committed snapshot is stale; re-run without --check"
        );
        return;
    }
    let path = overlay_path();
    std::fs::write(&path, format!("{rendered}\n")).expect("write snapshot");
    println!("wrote {} ({} models)", path.display(), entries.len());
}
