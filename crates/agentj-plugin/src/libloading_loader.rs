//! C-ABI contract shared with plugin cdylibs (see agentj-plugin-fixture):
//!   agentj_plugin_load() -> *mut c_void   // ownership of Box<dyn PluginInstance> moves to caller
//!   agentj_plugin_abi_version() -> u32    // guard: must equal 1
pub const ABI_VERSION: u32 = 1;

use crate::manifest::{PluginError, PluginManifest};
use crate::{PluginInstance, PluginLoader};

/// Loads native plugins (.so/.dylib) via libloading — v0.3 first loader
/// (design decision #12; trust model = user-installed).
pub struct LibloadingLoader {
    lib_path: String,
}

impl LibloadingLoader {
    pub fn new(lib_path: String) -> Self {
        Self { lib_path }
    }
}

/// Keeps the cdylib mapped for as long as the instance is alive. Fields drop in
/// declaration order: `inner` (the plugin's code/data) is dropped BEFORE `_lib`
/// (the mapped library) — no use-after-unmap.
pub struct LibPluginInstance {
    inner: Box<dyn PluginInstance>,
    _lib: libloading::Library,
}

impl PluginInstance for LibPluginInstance {
    fn manifest(&self) -> &PluginManifest {
        self.inner.manifest()
    }
    fn as_module(&self) -> &dyn agentj_core::module::Module {
        self.inner.as_module()
    }
}

impl PluginLoader for LibloadingLoader {
    fn load(&self, manifest: &PluginManifest) -> Result<Box<dyn PluginInstance>, PluginError> {
        // Safety: the fixture cdylib is compiled from this workspace with the same
        // PluginInstance layout; ABI version guards against mismatched binaries.
        let lib = unsafe { libloading::Library::new(&self.lib_path) }.map_err(|e| {
            PluginError::LoadFailed {
                id: manifest.id.clone(),
                reason: e.to_string(),
            }
        })?;
        unsafe {
            let abi: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib
                .get(b"agentj_plugin_abi_version")
                .map_err(|e| PluginError::LoadFailed {
                    id: manifest.id.clone(),
                    reason: e.to_string(),
                })?;
            if abi() != ABI_VERSION {
                return Err(PluginError::LoadFailed {
                    id: manifest.id.clone(),
                    reason: "ABI version mismatch".into(),
                });
            }
            let load: libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void> = lib
                .get(b"agentj_plugin_load")
                .map_err(|e| PluginError::LoadFailed {
                    id: manifest.id.clone(),
                    reason: e.to_string(),
                })?;
            let raw = load();
            if raw.is_null() {
                return Err(PluginError::LoadFailed {
                    id: manifest.id.clone(),
                    reason: "null instance".into(),
                });
            }
            let inner: Box<dyn PluginInstance> =
                *Box::from_raw(raw as *mut Box<dyn PluginInstance>);
            // `lib` moves into the wrapper → the library stays mapped for the instance's lifetime
            Ok(Box::new(LibPluginInstance { inner, _lib: lib }))
        }
    }

    fn unload(&self, _instance: Box<dyn PluginInstance>) -> Result<(), PluginError> {
        Ok(()) // dropping the instance drops the wrapper → inner first, then the Library
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

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
