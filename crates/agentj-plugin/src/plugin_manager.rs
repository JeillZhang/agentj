//! Plugin dependency graph and tool registration management (T5).
//! The state-machine types are imported here so the tests module (`use super::*`) can
//! reach them: `use crate::state_machine::{PluginState, PluginStateMachine};`

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
        let manifests = vec![deps("c", vec!["a"]), deps("b", vec!["a"]), deps("a", vec![])];
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
            .register_tool_with_source(Arc::new(DummyTool), ToolSource::Builtin, ReplacePolicy::Reject)
            .unwrap();
        let err = register_plugin_tool(&registry, Arc::new(DummyTool), "p1", ReplacePolicy::Reject)
            .unwrap_err();
        assert!(matches!(err, PluginError::ToolConflict { ref name, ref existing }
            if name == "dummy" && *existing == ToolSource::Builtin));
        // Replace policy overwrites and succeeds
        assert!(register_plugin_tool(&registry, Arc::new(DummyTool), "p1", ReplacePolicy::Replace).is_ok());
    }

    #[test]
    fn disabled_plugin_tools_excluded_from_definitions() {
        // design P1 缺口闭环: a disabled plugin's tools must leave the model tool list
        // (ToolRegistry::definitions), and re-enable must restore them.
        let registry = ToolRegistry::new();
        registry
            .register_tool_with_source(
                Arc::new(DummyTool),
                ToolSource::Plugin { plugin_id: "p1".into() },
                ReplacePolicy::Reject,
            )
            .unwrap();
        assert_eq!(registry.definitions().len(), 1);
        set_plugin_enabled(&registry, "p1", false);
        assert!(registry.definitions().is_empty(), "disabled plugin tools must not reach the model tool list");
        assert_eq!(registry.tools().len(), 1, "entries stay registered while disabled");
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
}