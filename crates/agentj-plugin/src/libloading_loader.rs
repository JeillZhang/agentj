//! C-ABI contract shared with plugin cdylibs (see agentj-plugin-fixture):
//!   agentj_plugin_load() -> *mut c_void   // ownership of Box<dyn PluginInstance> moves to caller
//!   agentj_plugin_abi_version() -> u32    // guard: must equal 1
pub const ABI_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;
    use agentj_core::module::Module;

    #[test]
    fn libloading_loader_loads_fixture_plugin() {
        // The fixture crate builds a cdylib; libloading loads it at runtime.
        let manifest = PluginManifest {
            id: "fixture-plugin".into(),
            name: "Fixture".into(),
            version: "1".into(),
            description: "".into(),
            author: "".into(),
            dependencies: vec![],
        };
        let loader = LibloadingLoader::new(fixture_lib_path());
        let instance = loader.load(&manifest).unwrap();
        assert_eq!(instance.as_module().id(), "fixture-plugin");
        loader.unload(instance).unwrap();
    }

    #[test]
    fn libloading_loader_missing_lib_fails() {
        let loader = LibloadingLoader::new("/nonexistent/lib.so".into());
        let manifest = PluginManifest {
            id: "x".into(),
            name: "x".into(),
            version: "1".into(),
            description: "".into(),
            author: "".into(),
            dependencies: vec![],
        };
        assert!(matches!(
            loader.load(&manifest),
            Err(PluginError::LoadFailed { .. })
        ));
    }

    fn fixture_lib_path() -> String {
        // Build artifacts land under target/<profile>/; the fixture is built as a
        // workspace member so this resolves its cdylib next to the other build outputs.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let mut path = std::path::PathBuf::from(manifest_dir);
        path.push("..");
        path.push("..");
        path.push("target");
        path.push(profile);
        path.push(format!("libagentj_plugin_fixture.{ext}"));
        path.to_string_lossy().to_string()
    }
}
