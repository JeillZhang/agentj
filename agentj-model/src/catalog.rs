#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_builtin_loads() {
        let catalog = ModelCatalog::builtin();
        assert!(!catalog.entries.is_empty(), "catalog must not be empty");
        for (_, entry) in &catalog.entries {
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
        for (_, entry) in &catalog.entries {
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
