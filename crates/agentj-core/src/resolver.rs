// T1 (v0.3 ecosystem): ModuleResolver service location + KernelMessage::ServiceCall.
// Tests only — implementation lands in the follow-up commit.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Kernel;
    use crate::module::*;
    use std::sync::Arc;

    struct EchoService;
    impl KernelService for EchoService {
        fn call(&self, _method: &str, params: Value) -> Result<Value, String> {
            Ok(params)
        }
    }

    #[test]
    fn service_registry_registers_and_resolves_concrete_type() {
        let registry = ServiceRegistry::new();
        assert!(registry.resolve::<EchoService>(&"echo".into()).is_none());
        registry.register("echo".into(), Arc::new(EchoService));
        let svc = registry.resolve::<EchoService>(&"echo".into()).unwrap();
        assert_eq!(svc.call("echo", serde_json::json!({"x": 1})).unwrap()["x"], 1);
    }

    #[test]
    fn kernel_service_route_resolves_via_dedicated_map() {
        // ServiceCall routing resolves the TRAIT OBJECT — Arc<dyn Any>::downcast requires
        // Sized, so trait-object resolution must NOT go through the Any map. It uses the
        // dedicated KernelService map instead (see impl_code).
        let registry = ServiceRegistry::new();
        assert!(registry.get_kernel_service(&"echo".into()).is_none());
        registry.register_kernel_service("echo".into(), Arc::new(EchoService));
        let svc = registry.get_kernel_service(&"echo".into()).unwrap();
        assert_eq!(svc.call("echo", serde_json::json!({"x": 1})).unwrap()["x"], 1);
    }

    #[tokio::test]
    async fn kernel_routes_service_call_and_returns_reply() {
        let mut kernel = Kernel::new();
        let services = kernel.services();
        services.register_kernel_service("echo".into(), Arc::new(EchoService));
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel.sender().send(KernelMessage::ServiceCall {
            service: "echo".into(),
            method: "echo".into(),
            params: serde_json::json!({"hello": "world"}),
            reply: Arc::new(std::sync::Mutex::new(Some(tx))),
        }).unwrap();
        // rx: oneshot::Receiver<Result<Value, String>> → rx.await yields
        // Result<Result<Value,String>, RecvError>; timeout() wraps that with Elapsed, so
        // TWO expects peel Elapsed then RecvError — leaving Result<Value, String>.
        let reply: Result<Value, String> = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await
            .expect("reply must arrive")
            .expect("sender dropped without sending");
        assert_eq!(reply.unwrap()["hello"], "world");
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn service_call_to_unknown_service_returns_error_reply() {
        let mut kernel = Kernel::new();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel.sender().send(KernelMessage::ServiceCall {
            service: "ghost".into(),
            method: "x".into(),
            params: serde_json::json!({}),
            reply: Arc::new(std::sync::Mutex::new(Some(tx))),
        }).unwrap();
        let reply: Result<Value, String> = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await
            .expect("reply must arrive for unknown service")
            .expect("sender dropped without sending");
        assert!(reply.is_err(), "unknown service must produce an error reply, not hang");
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &str { "dummy" }
        fn description(&self) -> &str { "" }
        fn input_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        fn execute(&self, _i: serde_json::Value) -> Result<serde_json::Value, crate::error::ToolError> {
            Ok(serde_json::Value::Null)
        }
    }

    #[test]
    fn tool_source_enable_filters_definitions() {
        // design P1 缺口闭环 — the ToolRegistry source-enable primitive lands in T1 so
        // Group B stays file-disjoint (T1 is the only Group-B task editing module.rs).
        // The plugin-level consumer test (set_plugin_enabled) lives in T5.
        let registry = ToolRegistry::new();
        registry.register_tool_with_source(Arc::new(DummyTool), ToolSource::Plugin { plugin_id: "p1".into() }, ReplacePolicy::Reject).unwrap();
        assert_eq!(registry.definitions().len(), 1);
        registry.set_source_enabled(&ToolSource::Plugin { plugin_id: "p1".into() }, false);
        assert!(registry.definitions().is_empty(), "disabled source tools must not reach the model tool list");
        assert_eq!(registry.tools().len(), 1, "entries stay registered while disabled");
        registry.set_source_enabled(&ToolSource::Plugin { plugin_id: "p1".into() }, true);
        assert_eq!(registry.definitions().len(), 1);
    }
}
