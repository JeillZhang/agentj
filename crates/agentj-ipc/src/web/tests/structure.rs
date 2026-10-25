//! Structural pin for the split itself: the root of the `web` module stays a
//! thin facade. Every behaviour moved into a sibling module, so `mod.rs` growing
//! an `impl WebConfig` or an `async fn` back means a domain silently returned to
//! the front door.

/// The root file's own text, pinned with `include_str!` so it is always current.
const ROOT: &str = include_str!("../mod.rs");

#[test]
fn web_root_stays_thin_and_transport_free() {
    assert!(
        !ROOT.contains("impl WebConfig"),
        "impl WebConfig belongs in config.rs or persist.rs, not web/mod.rs"
    );
    assert!(
        !ROOT.contains("async fn "),
        "async fn belongs in a domain module, not web/mod.rs"
    );
    let lines = ROOT.lines().count();
    assert!(lines <= 200, "web/mod.rs grew to {lines} lines (limit 200)");
}
