//! Comment-preserving write-back of the `[model]` defaults (design D6).
//!
//! `toml_edit` edits the two keys in place so user comments and unrelated tables
//! survive, unlike a full `toml` re-serialization.

use std::path::Path;

/// Set `[model].provider` and `[model].default_model` in `text`, preserving the rest.
pub fn set_model_defaults(text: &str, provider: &str, model: &str) -> Result<String, String> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    if !doc.contains_key("model") {
        doc["model"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["model"]["provider"] = toml_edit::value(provider);
    doc["model"]["default_model"] = toml_edit::value(model);
    Ok(doc.to_string())
}

/// Read `path`, apply [`set_model_defaults`] and write it back in place.
///
/// A missing `path` is not an error: the file is created from an empty base, so
/// `[model]` is built up the same way as in an existing file. Other IO errors
/// (e.g. permission denied) still surface.
pub fn write_model_defaults(path: &Path, provider: &str, model: &str) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    std::fs::write(path, set_model_defaults(&text, provider, model)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_model_defaults_preserves_comments_and_other_tables() {
        let text = "# keep me\n[model]\nprovider = \"deepseek\"\n\n[permission]\nmode = \"ask\"\n";
        let out = set_model_defaults(text, "opencode-go", "opencode-go/kimi-k3").unwrap();
        assert!(out.contains("# keep me"), "comments must survive: {out}");
        assert!(
            out.contains("mode = \"ask\""),
            "unrelated tables must survive: {out}"
        );
        assert!(out.contains("provider = \"opencode-go\""));
        assert!(out.contains("default_model = \"opencode-go/kimi-k3\""));
    }

    #[test]
    fn set_model_defaults_creates_missing_table() {
        let out = set_model_defaults("[session]\ndir = \"./s\"\n", "opencode-go", "m").unwrap();
        assert!(out.contains("[model]") && out.contains("default_model = \"m\""));
    }

    #[test]
    fn write_model_defaults_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(!path.exists(), "precondition: file must be absent");
        write_model_defaults(&path, "opencode-go", "opencode-go/kimi-k3").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[model]"),
            "missing table must be created: {text}"
        );
        assert!(text.contains("provider = \"opencode-go\""), "{text}");
        assert!(
            text.contains("default_model = \"opencode-go/kimi-k3\""),
            "{text}"
        );
    }

    #[test]
    fn write_model_defaults_updates_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# c\n[model]\nprovider = \"deepseek\"\n").unwrap();
        write_model_defaults(&path, "opencode-go", "opencode-go/x").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# c") && text.contains("opencode-go/x"));
    }
}
