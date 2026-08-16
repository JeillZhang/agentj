use agentj_core::module::ToolSource;
use agentj_core::types::ModuleId;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin '{id}' manifest invalid: {reason}")]
    ManifestInvalid { id: String, reason: String },
    #[error("plugin dependency cycle: {0:?}")]
    DependencyCycle(Vec<String>),
    #[error("plugin '{id}' reload failed, rolled back to previous version")]
    ReloadFailed { id: String },
    #[error("tool '{name}' already registered by {existing:?}")]
    ToolConflict { name: String, existing: ToolSource },
    #[error("plugin '{id}' load failed: {reason}")]
    LoadFailed { id: String, reason: String },
    #[error("plugin '{0}' not found")]
    NotFound(ModuleId),
}

/// Declarative plugin descriptor (design §插件生命周期). `dependencies` feeds the
/// topological init / reverse-topological shutdown order (T5).
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}
