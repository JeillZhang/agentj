//! Plugin dependency graph and tool registration management (T5).
//! The state-machine types are imported here so the tests module (`use super::*`) can
//! reach them: `use crate::state_machine::{PluginState, PluginStateMachine};`

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use agentj_core::module::{ReplacePolicy, Tool, ToolRegistry, ToolSource};

use crate::manifest::{PluginError, PluginManifest};
#[cfg(test)]
use crate::state_machine::{PluginState, PluginStateMachine};

/// Kahn's algorithm over manifest dependencies. Cycle → PluginError::DependencyCycle.
pub fn resolve_init_order(manifests: &[PluginManifest]) -> Result<Vec<String>, PluginError> {
    let deps: HashMap<String, Vec<String>> = manifests
        .iter()
        .map(|m| (m.id.clone(), m.dependencies.clone()))
        .collect();
    let mut order = Vec::with_capacity(manifests.len());
    let mut frontier: VecDeque<String> = deps
        .iter()
        .filter(|(_, d)| d.is_empty())
        .map(|(id, _)| id.clone())
        .collect();
    // Decrement in-degree style: repeatedly pick plugins whose deps are all in `order`.
    while let Some(id) = frontier.pop_front() {
        if order.contains(&id) {
            continue;
        }
        order.push(id.clone());
        for (cand, cand_deps) in deps.iter() {
            if order.contains(cand) {
                continue;
            }
            if cand_deps.iter().all(|d| order.contains(d)) {
                frontier.push_back(cand.clone());
            }
        }
    }
    if order.len() != manifests.len() {
        let leftover: Vec<String> = manifests
            .iter()
            .map(|m| m.id.clone())
            .filter(|id| !order.contains(id))
            .collect();
        return Err(PluginError::DependencyCycle(leftover));
    }
    Ok(order)
}

/// Reverse-topological shutdown: reverse of the init order (dependencies exit last).
pub fn resolve_shutdown_order(init_order: &[impl AsRef<str>]) -> Vec<String> {
    init_order
        .iter()
        .rev()
        .map(|s| s.as_ref().to_string())
        .collect()
}

/// Register a plugin-owned tool. With `ReplacePolicy::Reject` (design P5 default), a
/// name already taken by any source fails with `PluginError::ToolConflict` carrying the
/// existing owner's source; `Replace` overwrites (the plugin explicitly declared intent).
pub fn register_plugin_tool(
    registry: &ToolRegistry,
    tool: Arc<dyn Tool>,
    plugin_id: &str,
    policy: ReplacePolicy,
) -> Result<(), PluginError> {
    let name = tool.name().to_string();
    let existing = registry
        .tools()
        .iter()
        .find(|e| e.tool.name() == name)
        .map(|e| e.source.clone());
    if let Some(existing) = existing
        && policy == ReplacePolicy::Reject
    {
        return Err(PluginError::ToolConflict { name, existing });
    }
    registry
        .register_tool_with_source(
            tool,
            ToolSource::Plugin {
                plugin_id: plugin_id.into(),
            },
            policy,
        )
        .map_err(|_| PluginError::ToolConflict {
            name,
            existing: ToolSource::Builtin,
        })
}

/// Enable/disable a plugin's tools (design P1 缺口闭环): disable removes its tools from
/// the model tool list (definitions) while keeping the registry entries; enable restores.
pub fn set_plugin_enabled(registry: &ToolRegistry, plugin_id: &str, enabled: bool) {
    registry.set_source_enabled(
        &ToolSource::Plugin {
            plugin_id: plugin_id.into(),
        },
        enabled,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PluginError, PluginManifest};
    use agentj_core::module::{ReplacePolicy, Tool, ToolRegistry, ToolSource};
    use std::sync::Arc;

    fn deps(id: &str, d: Vec<&str>) -> PluginManifest {
        PluginManifest {
            id: id.into(),
            name: id.into(),
            version: "1".into(),
            description: "".into(),
            author: "".into(),
            dependencies: d.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn topological_init_dependencies_first() {
        let manifests = vec![
            deps("c", vec!["a"]),
            deps("b", vec!["a"]),
            deps("a", vec![]),
        ];
        let order = resolve_init_order(&manifests).unwrap();
        // HashMap iteration order is not deterministic — assert the invariant, not an
        // exact sequence: a must precede b and c; b and c are mutually unordered.
        assert_eq!(order.len(), 3);
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("a") < pos("b"), "a must init before b, got {order:?}");
        assert!(pos("a") < pos("c"), "a must init before c, got {order:?}");
    }

    #[test]
    fn reverse_topological_shutdown_order() {
        let order = vec!["a", "b", "c"];
        assert_eq!(resolve_shutdown_order(&order), vec!["c", "b", "a"]);
    }

    #[test]
    fn dependency_cycle_is_detected() {
        let manifests = vec![deps("a", vec!["b"]), deps("b", vec!["a"])];
        let err = resolve_init_order(&manifests).unwrap_err();
        assert!(matches!(err, PluginError::DependencyCycle(_)));
    }

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(
            &self,
            _i: serde_json::Value,
        ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
            Ok(serde_json::Value::Null)
        }
    }

    #[test]
    fn plugin_tool_conflict_rejected_with_existing_source() {
        // design P5: Reject policy on a taken name → PluginError::ToolConflict with the
        // existing owner's source
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Builtin,
                ReplacePolicy::Reject,
            )
            .unwrap();
        let err = register_plugin_tool(&registry, Arc::new(DummyTool), "p1", ReplacePolicy::Reject)
            .unwrap_err();
        assert!(
            matches!(err, PluginError::ToolConflict { ref name, ref existing }
            if name == "dummy" && *existing == ToolSource::Builtin)
        );
        // Replace policy overwrites and succeeds
        assert!(
            register_plugin_tool(&registry, Arc::new(DummyTool), "p1", ReplacePolicy::Replace)
                .is_ok()
        );
    }

    #[test]
    fn disabled_plugin_tools_excluded_from_definitions() {
        // design P1 缺口闭环: a disabled plugin's tools must leave the model tool list
        // (ToolRegistry::definitions), and re-enable must restore them.
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Plugin {
                    plugin_id: "p1".into(),
                },
                ReplacePolicy::Reject,
            )
            .unwrap();
        assert_eq!(registry.definitions().len(), 1);
        set_plugin_enabled(&registry, "p1", false);
        assert!(
            registry.definitions().is_empty(),
            "disabled plugin tools must not reach the model tool list"
        );
        assert_eq!(
            registry.tools().len(),
            1,
            "entries stay registered while disabled"
        );
        set_plugin_enabled(&registry, "p1", true);
        assert_eq!(registry.definitions().len(), 1);
    }

    #[test]
    fn state_machine_transitions_and_reload_rollback() {
        let mut sm = PluginStateMachine::new("p1");
        assert_eq!(sm.state(), PluginState::Unloaded);
        sm.begin_loading();
        assert_eq!(sm.state(), PluginState::Loading);
        sm.initialized();
        assert_eq!(sm.state(), PluginState::Ready);
        sm.disable();
        assert_eq!(sm.state(), PluginState::Disabled);
        sm.enable();
        assert_eq!(sm.state(), PluginState::Ready);
        sm.begin_reload();
        assert_eq!(sm.state(), PluginState::Reloading);
        sm.reload_failed(); // rollback
        assert_eq!(sm.state(), PluginState::Ready);
        assert_eq!(sm.last_error(), Some("reload failed".to_string()));
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        // BUG-012: unguarded transitions must not jump into arbitrary states;
        // an illegal call keeps the current state.
        let mut sm = PluginStateMachine::new("p1");
        assert_eq!(sm.state(), PluginState::Unloaded);
        sm.initialized();
        assert_eq!(
            sm.state(),
            PluginState::Unloaded,
            "initialized from Unloaded"
        );
        sm.reload_failed();
        assert_eq!(
            sm.state(),
            PluginState::Unloaded,
            "reload_failed from Unloaded"
        );
        sm.stopping();
        assert_eq!(sm.state(), PluginState::Unloaded, "stopping from Unloaded");
        sm.stopped();
        assert_eq!(sm.state(), PluginState::Unloaded, "stopped from Unloaded");
        sm.begin_loading();
        assert_eq!(sm.state(), PluginState::Loading);
        sm.reload_failed(); // rollback only valid from Reloading
        assert_eq!(
            sm.state(),
            PluginState::Loading,
            "reload_failed from Loading"
        );
        sm.stopped();
        assert_eq!(sm.state(), PluginState::Loading, "stopped from Loading");
        sm.initialized();
        assert_eq!(sm.state(), PluginState::Ready);
        sm.stopping();
        assert_eq!(sm.state(), PluginState::Stopping);
        sm.stopped();
        assert_eq!(sm.state(), PluginState::Stopped);
        // failure may arrive from any stage, and stopping recovers from Failed
        let mut failed_sm = PluginStateMachine::new("p2");
        failed_sm.mark_failed("boom");
        assert_eq!(failed_sm.state(), PluginState::Failed);
        failed_sm.stopping();
        assert_eq!(failed_sm.state(), PluginState::Stopping);
        failed_sm.stopped();
        assert_eq!(failed_sm.state(), PluginState::Stopped);
    }
}
