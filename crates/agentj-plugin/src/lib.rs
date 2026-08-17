//! Plugin crate — manifest, loader traits, and the PluginInstance bridge to the kernel.
//! The kernel only ever sees `PluginInstance::as_module()`; loading mechanism and
//! lifecycle are decoupled (design decision #12).

pub mod manifest;
pub use manifest::{PluginError, PluginManifest};

pub mod plugin_manager;
pub mod state_machine;
pub use plugin_manager::{
    register_plugin_tool, resolve_init_order, resolve_shutdown_order, set_plugin_enabled,
};
pub use state_machine::{PluginState, PluginStateMachine};

/// A loaded plugin, bridged to the kernel as a `Module` (design §插件生命周期).
/// The kernel only ever sees this — loading mechanism and lifecycle are decoupled.
pub trait PluginInstance: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn as_module(&self) -> &dyn agentj_core::module::Module;
}

/// Abstracts the plugin loading mechanism (design decision #12): `LibloadingLoader`
/// is v0.3's first loader, `WasmLoader` is deferred/optional.
pub trait PluginLoader: Send + Sync {
    fn load(&self, manifest: &PluginManifest) -> Result<Box<dyn PluginInstance>, PluginError>;
    fn unload(&self, instance: Box<dyn PluginInstance>) -> Result<(), PluginError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ModuleError;
    use agentj_core::module::{KernelContext, Module, ModuleMetadata, ModuleRegister};
    use agentj_core::types::ModuleId;

    // ---- fake module used by the fake loader ----
    struct FakeModule {
        id: ModuleId,
    }
    impl Module for FakeModule {
        fn id(&self) -> ModuleId {
            self.id.clone()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: self.id.clone(),
                name: "fake".into(),
                version: "0.1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(
            &mut self,
            _ctx: &KernelContext,
            _r: &mut ModuleRegister,
        ) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }

    struct FakeInstance {
        manifest: PluginManifest,
        module: FakeModule,
    }
    impl PluginInstance for FakeInstance {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        fn as_module(&self) -> &dyn Module {
            &self.module
        }
    }

    struct FakeLoader;
    impl PluginLoader for FakeLoader {
        fn load(&self, manifest: &PluginManifest) -> Result<Box<dyn PluginInstance>, PluginError> {
            Ok(Box::new(FakeInstance {
                manifest: manifest.clone(),
                module: FakeModule {
                    id: manifest.id.clone(),
                },
            }))
        }
        fn unload(&self, _instance: Box<dyn PluginInstance>) -> Result<(), PluginError> {
            Ok(())
        }
    }

    #[test]
    fn manifest_parses_from_toml() {
        let toml_str = r#"
            id = "my-plugin"
            name = "My Plugin"
            version = "1.0.0"
            dependencies = ["base"]
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.id, "my-plugin");
        assert_eq!(manifest.dependencies, vec!["base".to_string()]);
    }

    #[test]
    fn manifest_missing_id_is_invalid() {
        let result: Result<PluginManifest, toml::de::Error> = toml::from_str("name = 'x'\n");
        assert!(result.is_err(), "missing id must fail deserialization");
    }

    #[test]
    fn fake_loader_loads_and_bridges_to_module() {
        let manifest = PluginManifest {
            id: "p1".into(),
            name: "P1".into(),
            version: "1".into(),
            description: "".into(),
            author: "".into(),
            dependencies: vec![],
        };
        let loader = FakeLoader;
        let instance = loader.load(&manifest).unwrap();
        assert_eq!(instance.manifest().id, "p1");
        assert_eq!(instance.as_module().id(), "p1");
        loader.unload(instance).unwrap();
    }
}
