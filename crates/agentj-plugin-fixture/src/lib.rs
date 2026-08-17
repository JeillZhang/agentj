//! Test plugin exported as a cdylib for LibloadingLoader's integration test.
use agentj_core::error::ModuleError;
use agentj_core::module::{KernelContext, Module, ModuleMetadata, ModuleRegister};
use agentj_core::types::ModuleId;
use agentj_plugin::{PluginInstance, PluginManifest};

struct FixtureModule;
impl Module for FixtureModule {
    fn id(&self) -> ModuleId {
        "fixture-plugin".into()
    }
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "fixture-plugin".into(),
            name: "Fixture".into(),
            version: "1".into(),
            description: "".into(),
            author: "".into(),
        }
    }
    fn init(&mut self, _ctx: &KernelContext, _r: &mut ModuleRegister) -> Result<(), ModuleError> {
        Ok(())
    }
    fn shutdown(&mut self) -> Result<(), ModuleError> {
        Ok(())
    }
}

struct FixtureInstance {
    manifest: PluginManifest,
    module: FixtureModule,
}
impl PluginInstance for FixtureInstance {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn as_module(&self) -> &dyn Module {
        &self.module
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn agentj_plugin_abi_version() -> u32 {
    agentj_plugin::libloading_loader::ABI_VERSION
}

/// Leaks a Box<dyn PluginInstance> to the loader (caller takes ownership).
#[unsafe(no_mangle)]
pub extern "C" fn agentj_plugin_load() -> *mut std::ffi::c_void {
    let manifest = PluginManifest {
        id: "fixture-plugin".into(),
        name: "Fixture".into(),
        version: "1".into(),
        description: "".into(),
        author: "".into(),
        dependencies: vec![],
    };
    let instance: Box<dyn PluginInstance> = Box::new(FixtureInstance {
        manifest,
        module: FixtureModule,
    });
    Box::into_raw(Box::new(instance)) as *mut std::ffi::c_void
}
