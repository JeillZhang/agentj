// T1 (v0.3 ecosystem): ModuleResolver service location + KernelMessage::ServiceCall.
use crate::sync::{read_or_recover, write_or_recover};
use crate::types::ModuleId;
use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// v0.3 service-location channel (design §7 预留接口 #1): capability interfaces stay
/// `&self + Send + Sync`; this is how the Kernel reaches them without message routing.
pub trait ModuleResolver: Send + Sync {
    fn resolve<T: 'static + Send + Sync>(&self, id: &ModuleId) -> Option<Arc<T>>;
}

/// A routable service invoked by KernelMessage::ServiceCall.
pub trait KernelService: Send + Sync {
    fn call(&self, method: &str, params: Value) -> Result<Value, String>;
}

#[async_trait]
pub trait AsyncKernelService: Send + Sync {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String>;
}

/// Reply channel for KernelMessage::ServiceCall: `Arc` keeps the enum Clone-able;
/// `take()` releases the oneshot after the first (and only) reply.
pub type ServiceCallReply =
    Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Result<Value, String>>>>>;

#[derive(Default)]
pub struct ServiceRegistry {
    // Concrete-type service location (design §7 #1): Arc<dyn Any>::downcast requires
    // Sized, so this map serves resolve::<T> with CONCRETE types only.
    services: RwLock<HashMap<ModuleId, Arc<dyn Any + Send + Sync>>>,
    // KernelService routing map (KernelMessage::ServiceCall): trait objects cannot go
    // through the Any map (E0277: dyn KernelService is unsized) — a dedicated map keeps
    // the enum routing type-safe without ?Sized gymnastics.
    kernel_services: RwLock<HashMap<ModuleId, Arc<dyn KernelService>>>,
    kernel_async_services: RwLock<HashMap<ModuleId, Arc<dyn AsyncKernelService>>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T: 'static + Send + Sync>(&self, id: ModuleId, service: Arc<T>) {
        write_or_recover(&self.services).insert(id, service);
    }

    pub fn register_kernel_service(&self, id: ModuleId, service: Arc<dyn KernelService>) {
        write_or_recover(&self.kernel_services).insert(id, service);
    }

    pub fn unregister(&self, id: &ModuleId) {
        write_or_recover(&self.services).remove(id);
        write_or_recover(&self.kernel_services).remove(id);
        write_or_recover(&self.kernel_async_services).remove(id);
    }

    pub fn get<T: 'static + Send + Sync>(&self, id: &ModuleId) -> Option<Arc<T>> {
        let svc = read_or_recover(&self.services).get(id)?.clone();
        svc.downcast::<T>().ok()
    }

    pub fn get_kernel_service(&self, id: &ModuleId) -> Option<Arc<dyn KernelService>> {
        read_or_recover(&self.kernel_services).get(id).cloned()
    }

    pub fn register_kernel_async_service(
        &self,
        id: ModuleId,
        service: Arc<dyn AsyncKernelService>,
    ) {
        crate::sync::write_or_recover(&self.kernel_async_services).insert(id, service);
    }

    pub fn get_kernel_async_service(&self, id: &ModuleId) -> Option<Arc<dyn AsyncKernelService>> {
        read_or_recover(&self.kernel_async_services)
            .get(id)
            .cloned()
    }
}

impl ModuleResolver for ServiceRegistry {
    fn resolve<T: 'static + Send + Sync>(&self, id: &ModuleId) -> Option<Arc<T>> {
        self.get(id)
    }
}

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
        assert_eq!(
            svc.call("echo", serde_json::json!({"x": 1})).unwrap()["x"],
            1
        );
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
        assert_eq!(
            svc.call("echo", serde_json::json!({"x": 1})).unwrap()["x"],
            1
        );
    }

    #[tokio::test]
    async fn kernel_routes_service_call_and_returns_reply() {
        let mut kernel = Kernel::new();
        let services = kernel.services();
        services.register_kernel_service("echo".into(), Arc::new(EchoService));
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "echo".into(),
                method: "echo".into(),
                params: serde_json::json!({"hello": "world"}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        // rx: oneshot::Receiver<Result<Value, String>> → rx.await yields
        // Result<Result<Value,String>, RecvError>; timeout() wraps that with Elapsed, so
        // TWO expects peel Elapsed then RecvError — leaving Result<Value, String>.
        let reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
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
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "ghost".into(),
                method: "x".into(),
                params: serde_json::json!({}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive for unknown service")
                .expect("sender dropped without sending");
        assert!(
            reply.is_err(),
            "unknown service must produce an error reply, not hang"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    struct SlowService;
    #[async_trait::async_trait]
    impl AsyncKernelService for SlowService {
        async fn call(&self, _method: &str, params: Value) -> Result<Value, String> {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(params)
        }
    }

    #[tokio::test]
    async fn async_service_call_replies_after_completion_without_blocking_loop() {
        let mut kernel = Kernel::new();
        let services = kernel.services();
        services.register_kernel_async_service("slow".into(), Arc::new(SlowService));
        services.register_kernel_service("echo".into(), Arc::new(EchoService));
        let handle = kernel.start();

        // Send async call and measure elapsed.
        let (tx_slow, rx_slow) = tokio::sync::oneshot::channel();
        let start = std::time::Instant::now();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "slow".into(),
                method: "do".into(),
                params: serde_json::json!({"v": 42}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx_slow))),
            })
            .unwrap();

        // While slow is pending, a sync call must still be serviced quickly
        // (proves the loop was not blocked by the async handler).
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let (tx_echo, rx_echo) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "echo".into(),
                method: "echo".into(),
                params: serde_json::json!({"ping": 1}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx_echo))),
            })
            .unwrap();
        let echo_reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx_echo)
                .await
                .expect("echo reply must arrive while slow pending")
                .expect("sender dropped");
        assert_eq!(echo_reply.unwrap()["ping"], 1);
        // echo should have been fast (<80ms) despite slow still running.
        assert!(
            start.elapsed() < std::time::Duration::from_millis(80),
            "sync path must not be blocked by async service"
        );

        let slow_reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx_slow)
                .await
                .expect("slow reply must arrive")
                .expect("sender dropped without sending");
        let elapsed = start.elapsed();
        assert_eq!(slow_reply.unwrap()["v"], 42);
        assert!(
            elapsed >= std::time::Duration::from_millis(90),
            "async service should delay ~100ms, got {elapsed:?}"
        );

        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn async_service_call_to_unknown_service_returns_error() {
        let mut kernel = Kernel::new();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "ghost_async".into(),
                method: "x".into(),
                params: serde_json::json!({}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive for unknown async service")
                .expect("sender dropped without sending");
        assert!(
            reply.is_err(),
            "unknown async service must produce an error reply, not hang"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
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
        ) -> Result<serde_json::Value, crate::error::ToolError> {
            Ok(serde_json::Value::Null)
        }
    }

    #[test]
    fn tool_source_enable_filters_definitions() {
        // design P1 缺口闭环 — the ToolRegistry source-enable primitive lands in T1 so
        // Group B stays file-disjoint (T1 is the only Group-B task editing module.rs).
        // The plugin-level consumer test (set_plugin_enabled) lives in T5.
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
        registry.set_source_enabled(
            &ToolSource::Plugin {
                plugin_id: "p1".into(),
            },
            false,
        );
        assert!(
            registry.definitions().is_empty(),
            "disabled source tools must not reach the model tool list"
        );
        assert_eq!(
            registry.tools().len(),
            1,
            "entries stay registered while disabled"
        );
        registry.set_source_enabled(
            &ToolSource::Plugin {
                plugin_id: "p1".into(),
            },
            true,
        );
        assert_eq!(registry.definitions().len(), 1);
    }
}
