use crate::error::KernelError;
use crate::hooks::EventBus;
use crate::module::{
    EventType, KernelContext, KernelMessage, KernelModule, ModuleRegister, ModuleState,
};
use crate::resolver::{ModuleResolver, ServiceRegistry};
use crate::types::ModuleId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::task::JoinHandle;

const CHANNEL_SIZE: usize = 128;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(10); // design #13 CLI 信号兜底

#[derive(Clone)]
pub struct KernelSender {
    tx: tokio::sync::mpsc::Sender<KernelMessage>,
}

impl KernelSender {
    pub fn send(&self, msg: KernelMessage) -> Result<(), KernelError> {
        self.tx
            .try_send(msg)
            .map_err(|e| KernelError::MessageRouteFailed {
                msg: format!("{:?}", e.into_inner()),
                reason: "channel closed or full".into(),
            })
    }
}

/// Kernel: message bus + module registry + lifecycle state machine.
///
/// Module lifecycle states (Unloaded/Initializing/Ready/Stopping/Stopped/Failed) are
/// tracked here, NOT on the Module trait — state belongs to the orchestrator.
pub struct Kernel {
    modules: Arc<Mutex<HashMap<String, Box<dyn KernelModule>>>>,
    order: Arc<Mutex<Vec<String>>>, // registration order, for reverse shutdown
    states: Arc<Mutex<HashMap<String, ModuleState>>>,
    sender: KernelSender,
    rx: Option<tokio::sync::mpsc::Receiver<KernelMessage>>,
    shutdown_timeout: Duration,
    overall_timeout: Duration, // design #13: total graceful-shutdown budget
    events: Arc<EventBus>,
    ctx: Arc<RwLock<Option<KernelContext>>>, // retained at first register_module
    services: Arc<ServiceRegistry>,          // v0.3 service location (design §7 #1)
}

impl Kernel {
    pub fn new() -> Self {
        Self::with_timeouts(DEFAULT_SHUTDOWN_TIMEOUT, DEFAULT_OVERALL_TIMEOUT)
    }

    pub fn with_shutdown_timeout(timeout: Duration) -> Self {
        Self::with_timeouts(timeout, DEFAULT_OVERALL_TIMEOUT)
    }

    pub fn with_timeouts(shutdown_timeout: Duration, overall_timeout: Duration) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(CHANNEL_SIZE);
        Self {
            modules: Arc::new(Mutex::new(HashMap::new())),
            order: Arc::new(Mutex::new(Vec::new())),
            states: Arc::new(Mutex::new(HashMap::new())),
            sender: KernelSender { tx },
            rx: Some(rx),
            shutdown_timeout,
            overall_timeout,
            events: Arc::new(EventBus::new()),
            ctx: Arc::new(RwLock::new(None)),
            services: Arc::new(ServiceRegistry::new()),
        }
    }

    pub fn sender(&self) -> KernelSender {
        self.sender.clone()
    }

    /// The kernel's service registry — register capability services here or via
    /// `ModuleRegister::register_service` during module init.
    pub fn services(&self) -> Arc<ServiceRegistry> {
        self.services.clone()
    }

    /// Locate a concrete-typed capability service (design §7 #1 service channel).
    pub fn resolve<T: 'static + Send + Sync>(&self, id: &ModuleId) -> Option<Arc<T>> {
        self.services.resolve(id)
    }

    /// The kernel's event bus. Register handlers here (or via `ModuleRegister::on_event`)
    /// to observe lifecycle events; the bus is a no-op until the first register_module
    /// provides a KernelContext.
    pub fn events(&self) -> Arc<EventBus> {
        self.events.clone()
    }

    /// Publish an event to the kernel bus (no-op until ctx is set by register_module).
    pub fn publish(&self, event: &EventType) {
        self.events.publish(event);
    }

    /// Query a module's lifecycle state (None if never registered).
    pub fn module_state(&self, id: &str) -> Option<ModuleState> {
        crate::sync::lock_or_recover(&self.states).get(id).copied()
    }

    pub fn modules(&self) -> Vec<(ModuleId, ModuleState)> {
        crate::sync::lock_or_recover(&self.states)
            .iter()
            .map(|(id, st)| (id.clone(), *st))
            .collect()
    }

    /// Register a module: init() runs synchronously; on failure the module is marked
    /// Failed (NOT inserted into the registry) and the error is returned — the kernel
    /// keeps running (one module's failure must not crash the kernel).
    pub fn register_module(
        &mut self,
        mut module: Box<dyn KernelModule>,
        ctx: &KernelContext,
        register: &mut ModuleRegister,
    ) -> Result<(), KernelError> {
        let id = module.id();
        self.set_state(&id, ModuleState::Initializing);
        if let Ok(mut guard) = self.ctx.write() {
            *guard = Some(ctx.clone());
        }
        self.events.set_ctx(Some(Arc::new(ctx.clone())));
        match module.init(ctx, register) {
            Ok(()) => {
                for (evt, handler) in register.events.drain(..) {
                    self.events.register(evt, Arc::from(handler));
                }
                crate::sync::lock_or_recover(&self.modules).insert(id.clone(), module);
                crate::sync::lock_or_recover(&self.order).push(id.clone());
                self.set_state(&id, ModuleState::Ready);
                self.publish(&EventType::ModuleLoaded(id.clone()));
                Ok(())
            }
            Err(e) => {
                self.set_state(&id, ModuleState::Failed);
                self.publish(&EventType::ModuleFailed(id.clone()));
                tracing::warn!(module_id = %id, error = %e, "module init failed, continuing without it");
                Err(KernelError::ModuleInitFailed(id, anyhow::Error::from(e)))
            }
        }
    }

    /// Read-only access to the registered KernelContext (None before the first
    /// register_module). IPC surface queries (e.g. kernel/status) use it to
    /// report boot-time config such as the default model.
    pub fn ctx(&self) -> Option<KernelContext> {
        self.ctx.read().ok()?.clone()
    }

    fn set_state(&self, id: &str, state: ModuleState) {
        crate::sync::lock_or_recover(&self.states).insert(id.to_string(), state);
    }

    /// Start the event loop. Shutdown is checked BEFORE broadcast (so a blocked
    /// module cannot starve the shutdown signal). On Shutdown (or channel close) all
    /// Ready modules are shut down in REVERSE registration order, each with a timeout.
    pub fn start(&mut self) -> JoinHandle<Result<(), KernelError>> {
        let modules = Arc::clone(&self.modules);
        let order = Arc::clone(&self.order);
        let states = Arc::clone(&self.states);
        let events = Arc::clone(&self.events);
        let services = Arc::clone(&self.services);
        let shutdown_timeout = self.shutdown_timeout;
        let overall_timeout = self.overall_timeout;
        let mut rx = self.rx.take().unwrap_or_else(|| {
            let (_, rx) = tokio::sync::mpsc::channel(1);
            rx
        });
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if matches!(msg, KernelMessage::Shutdown) {
                    break; // graceful shutdown handled below — NOT broadcast via handle()
                }
                // A module handler is third-party code: a panic there must cost one
                // message, not the resident kernel (every later client would hang).
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    route_message(&msg, &services, &modules);
                }));
                if let Err(payload) = outcome {
                    let reason = crate::sync::panic_payload_message(payload.as_ref());
                    tracing::error!(%reason, "kernel message handler panicked; loop continues");
                }
            }
            // design #13: overall budget for the whole chain — a hung module shutdown
            // must not block process exit indefinitely.
            tokio::time::timeout(
                overall_timeout,
                Self::shutdown_all(&modules, &order, &states, shutdown_timeout, &events),
            )
            .await
            .map_err(|_| KernelError::ShutdownTimeout(overall_timeout))?
        })
    }

    /// Reverse-order graceful shutdown. A single module's failure/timeout is logged
    /// and recorded (state = Failed), but does NOT block shutting down the rest.
    async fn shutdown_all(
        modules: &Mutex<HashMap<String, Box<dyn KernelModule>>>,
        order: &Mutex<Vec<String>>,
        states: &Mutex<HashMap<String, ModuleState>>,
        timeout: Duration,
        events: &Arc<EventBus>,
    ) -> Result<(), KernelError> {
        let ids: Vec<String> = crate::sync::lock_or_recover(order)
            .iter()
            .rev()
            .cloned()
            .collect();
        let mut first_error: Option<KernelError> = None;
        for id in &ids {
            crate::sync::lock_or_recover(states).insert(id.clone(), ModuleState::Stopping);
            let module = {
                let mut guard = crate::sync::lock_or_recover(modules);
                guard.remove(id) // remove while shutting down: no more messages after this
            };
            let Some(mut module) = module else { continue };
            // shutdown() is a sync fn that may block (e.g. killing a subprocess); run it on
            // the blocking pool so the per-module timeout can still fire.
            let shutdown_task = tokio::task::spawn_blocking(move || module.shutdown());
            let result = tokio::time::timeout(timeout, shutdown_task).await;
            let (state, err) = match result {
                Ok(Ok(Ok(()))) => (ModuleState::Stopped, None),
                Ok(Ok(Err(e))) => {
                    tracing::error!(module_id = %id, error = %e, "module shutdown failed");
                    (
                        ModuleState::Failed,
                        Some(KernelError::ModuleShutdownFailed(
                            id.clone(),
                            anyhow::Error::from(e),
                        )),
                    )
                }
                Ok(Err(e)) => {
                    tracing::error!(module_id = %id, error = %e, "module shutdown task panicked");
                    (
                        ModuleState::Failed,
                        Some(KernelError::ModuleShutdownFailed(
                            id.clone(),
                            anyhow::anyhow!(e.to_string()),
                        )),
                    )
                }
                Err(_) => {
                    tracing::error!(module_id = %id, ?timeout, "module shutdown timeout");
                    (
                        ModuleState::Failed,
                        Some(KernelError::ShutdownTimeout(timeout)),
                    )
                }
            };
            crate::sync::lock_or_recover(states).insert(id.clone(), state);
            match state {
                ModuleState::Stopped => {
                    events.publish(&EventType::ModuleStopped(id.clone()));
                }
                ModuleState::Failed => {
                    events.publish(&EventType::ModuleFailed(id.clone()));
                }
                _ => {}
            }
            if first_error.is_none() {
                first_error = err;
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Route one kernel message: service calls are answered by the registry, module
/// messages go to the addressed module (or broadcast to all). Extracted from the
/// event loop so the whole dispatch can sit behind one `catch_unwind` guard.
///
/// Panics inside a handler propagate to that guard; the caller logs and keeps
/// looping, and the registry lock stays usable thanks to
/// [`crate::sync::lock_or_recover`].
fn route_message(
    msg: &KernelMessage,
    services: &ServiceRegistry,
    modules: &Mutex<HashMap<String, Box<dyn KernelModule>>>,
) {
    // v0.3 service routing (design §7 #1): ServiceCall is served by the
    // registry BEFORE module broadcast; unknown services get an error
    // reply so the caller never hangs.
    if let KernelMessage::ServiceCall {
        service,
        method,
        params,
        reply,
    } = msg
    {
        if let Some(svc) = services.get_kernel_service(service) {
            let out = svc.call(method, params.clone());
            if let Some(tx) = crate::sync::lock_or_recover(reply).take() {
                let _ = tx.send(out);
            }
            return;
        }
        if let Some(svc) = services.get_kernel_async_service(service) {
            let svc = svc.clone();
            let method = method.clone();
            let params = params.clone();
            let reply = reply.clone();
            tokio::spawn(async move {
                let out = svc.call(&method, params).await;
                if let Some(tx) = crate::sync::lock_or_recover(&reply).take() {
                    let _ = tx.send(out);
                }
            });
            return;
        }
        let out: Result<serde_json::Value, String> = Err(format!("service '{service}' not found"));
        if let Some(tx) = crate::sync::lock_or_recover(reply).take() {
            let _ = tx.send(out);
        }
        return;
    }
    if let KernelMessage::ToolCallRequest { target, .. } = msg {
        let mut guard = crate::sync::lock_or_recover(modules);
        if let Some(module) = guard.get_mut(target) {
            if let Err(e) = module.handle(msg) {
                tracing::error!(?e, "module handler error");
            }
        } else {
            tracing::warn!(%target, "ToolCallRequest to unregistered module");
        }
        return;
    }
    let mut guard = crate::sync::lock_or_recover(modules);
    for module in guard.values_mut() {
        if let Err(e) = module.handle(msg) {
            tracing::error!(?e, "module handler error");
        }
    }
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::*;
    use crate::module::*;
    use crate::types::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;

    struct NoopSession;
    impl SessionStore for NoopSession {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("s1".into())
        }
        fn append(&self, _s: &SessionId, _m: ChatMessage) -> Result<(), ModuleError> {
            Ok(())
        }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(vec![])
        }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> {
            Ok(0)
        }
    }

    struct NoopConfig;
    impl ConfigProvider for NoopConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    fn dummy_kernel_context() -> KernelContext {
        KernelContext {
            session: Arc::new(NoopSession),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(NoopConfig),
        }
    }

    struct SpyModule {
        id: &'static str,
        handled: Arc<StdMutex<Vec<KernelMessage>>>,
        shutdowns: Arc<StdMutex<Vec<String>>>,
        shutdown_delay: Option<Duration>,
        init_fails: bool,
    }

    impl Module for SpyModule {
        fn id(&self) -> ModuleId {
            self.id.to_string()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: self.id.to_string(),
                name: self.id.to_string(),
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
            if self.init_fails {
                Err(ModuleError::Init("boom".into()))
            } else {
                Ok(())
            }
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            crate::sync::lock_or_recover(&self.shutdowns).push(self.id.to_string());
            if let Some(d) = self.shutdown_delay {
                std::thread::sleep(d);
            }
            Ok(())
        }
    }

    impl KernelModule for SpyModule {
        fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
            crate::sync::lock_or_recover(&self.handled).push(msg.clone());
            Ok(KernelResponse::Handled)
        }
    }

    fn spy(
        id: &'static str,
        handled: Arc<StdMutex<Vec<KernelMessage>>>,
        shutdowns: Arc<StdMutex<Vec<String>>>,
        shutdown_delay: Option<Duration>,
        init_fails: bool,
    ) -> SpyModule {
        SpyModule {
            id,
            handled,
            shutdowns,
            shutdown_delay,
            init_fails,
        }
    }

    #[tokio::test]
    async fn kernel_routes_message_and_shuts_down() {
        let handled = Arc::new(StdMutex::new(vec![]));
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(
                Box::new(spy("spy", handled.clone(), shutdowns.clone(), None, false)),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        assert_eq!(kernel.module_state("spy"), Some(ModuleState::Ready));

        let handle = kernel.start();
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        kernel
            .sender()
            .send(KernelMessage::user_request("s1", "hello", tx))
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(crate::sync::lock_or_recover(&handled).len(), 1);

        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
        assert_eq!(
            *crate::sync::lock_or_recover(&shutdowns),
            vec!["spy".to_string()]
        );
        assert_eq!(kernel.module_state("spy"), Some(ModuleState::Stopped));
    }

    #[tokio::test]
    async fn tool_call_request_routes_to_target_module_only() {
        let handled_a = Arc::new(StdMutex::new(vec![]));
        let handled_b = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(
                Box::new(spy(
                    "a",
                    handled_a.clone(),
                    Arc::new(StdMutex::new(vec![])),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        kernel
            .register_module(
                Box::new(spy(
                    "b",
                    handled_b.clone(),
                    Arc::new(StdMutex::new(vec![])),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "a".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(crate::sync::lock_or_recover(&handled_a).len(), 1);
        assert_eq!(
            crate::sync::lock_or_recover(&handled_b).len(),
            0,
            "target routing must not broadcast"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn tool_call_request_to_unregistered_target_keeps_kernel_running() {
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(
                Box::new(spy(
                    "a",
                    Arc::new(StdMutex::new(vec![])),
                    Arc::new(StdMutex::new(vec![])),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "ghost".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn modules_query_returns_registered_ids_and_states() {
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(
                Box::new(spy(
                    "a",
                    Arc::new(StdMutex::new(vec![])),
                    Arc::new(StdMutex::new(vec![])),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        assert_eq!(
            kernel.modules(),
            vec![("a".to_string(), ModuleState::Ready)]
        );
    }

    #[tokio::test]
    async fn shutdown_runs_in_reverse_registration_order() {
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(
                Box::new(spy(
                    "a",
                    Arc::new(StdMutex::new(vec![])),
                    shutdowns.clone(),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        kernel
            .register_module(
                Box::new(spy(
                    "b",
                    Arc::new(StdMutex::new(vec![])),
                    shutdowns.clone(),
                    None,
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
        assert_eq!(
            *crate::sync::lock_or_recover(&shutdowns),
            vec!["b".to_string(), "a".to_string()]
        );
    }

    #[tokio::test]
    async fn init_failure_is_reported_and_isolated() {
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        let err = kernel.register_module(
            Box::new(spy(
                "bad",
                Arc::new(StdMutex::new(vec![])),
                Arc::new(StdMutex::new(vec![])),
                None,
                true,
            )),
            &dummy_kernel_context(),
            &mut register,
        );
        assert!(matches!(err, Err(KernelError::ModuleInitFailed(ref id, _)) if id == "bad"));
        assert_eq!(kernel.module_state("bad"), Some(ModuleState::Failed));

        // kernel survives and still shuts down cleanly
        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shutdown_timeout_yields_shutdown_timeout() {
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::with_shutdown_timeout(Duration::from_millis(10));
        kernel
            .register_module(
                Box::new(spy(
                    "slow",
                    Arc::new(StdMutex::new(vec![])),
                    Arc::new(StdMutex::new(vec![])),
                    Some(Duration::from_millis(200)),
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        let result = handle.await.unwrap();
        assert!(matches!(result, Err(KernelError::ShutdownTimeout(_))));
        assert_eq!(kernel.module_state("slow"), Some(ModuleState::Failed));
    }

    #[tokio::test]
    async fn overall_shutdown_timeout_yields_timeout() {
        // Overall (design #13) timeout: 10ms total budget for the whole chain,
        // two modules each blocking 50ms — the overall budget fires first.
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::with_timeouts(
            Duration::from_secs(2),    // per-module timeout
            Duration::from_millis(10), // overall graceful shutdown timeout
        );
        kernel
            .register_module(
                Box::new(spy(
                    "a",
                    Arc::new(StdMutex::new(vec![])),
                    shutdowns.clone(),
                    Some(Duration::from_millis(50)),
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        kernel
            .register_module(
                Box::new(spy(
                    "b",
                    Arc::new(StdMutex::new(vec![])),
                    shutdowns.clone(),
                    Some(Duration::from_millis(50)),
                    false,
                )),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        let result = handle.await.unwrap();
        assert!(matches!(result, Err(KernelError::ShutdownTimeout(_))));
    }

    #[test]
    fn ctx_is_none_until_first_register_module() {
        let mut kernel = Kernel::new();
        assert!(kernel.ctx().is_none());
        let handled = Arc::new(StdMutex::new(vec![]));
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        kernel
            .register_module(
                Box::new(spy("spy", handled.clone(), shutdowns.clone(), None, false)),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();
        let ctx = kernel.ctx().expect("ctx is set once a module registered");
        assert!(ctx.config().get_str("model.default_model").is_none());
        assert!(ctx.config().providers().is_empty());
    }

    /// A module whose handler always panics — the kernel must survive it.
    struct BoomModule;

    impl Module for BoomModule {
        fn id(&self) -> ModuleId {
            "boom".to_string()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "boom".to_string(),
                name: "boom".to_string(),
                version: "0.1".into(),
                description: String::new(),
                author: String::new(),
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

    impl KernelModule for BoomModule {
        fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
            panic!("handler boom")
        }
    }

    #[tokio::test]
    async fn a_panicking_module_handler_does_not_stop_the_loop() {
        let handled = Arc::new(StdMutex::new(vec![]));
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let mut kernel = Kernel::new();
        kernel
            .register_module(Box::new(BoomModule), &dummy_kernel_context(), &mut register)
            .unwrap();
        kernel
            .register_module(
                Box::new(spy("spy", handled.clone(), shutdowns.clone(), None, false)),
                &dummy_kernel_context(),
                &mut register,
            )
            .unwrap();

        let handle = kernel.start();
        // Target the panicking module only, so the panic is deterministic and the
        // next message proves the loop (and its poisoned modules lock) still works.
        send_tool_call(&kernel, "boom");
        tokio::time::sleep(Duration::from_millis(40)).await;
        send_tool_call(&kernel, "spy");
        tokio::time::sleep(Duration::from_millis(40)).await;

        assert_eq!(
            crate::sync::lock_or_recover(&handled).len(),
            1,
            "the kernel loop must keep routing after a handler panic"
        );

        // Shutdown still completes: the modules lock was never left unusable.
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    /// Send a targeted `ToolCallRequest` at `target` (used to make panics deterministic).
    fn send_tool_call(kernel: &Kernel, target: &str) {
        let (tx, _rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: target.into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
    }

    /// A poisoned state map must not turn `modules()` into a permanent panic: the
    /// recover helper reuses the inner value instead of unwrapping the poison
    /// (BUG-001 lock convergence).
    #[test]
    fn modules_survive_a_poisoned_state_mutex() {
        let kernel = Kernel::new();
        let states = kernel.states.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = states.lock().unwrap();
            panic!("poison the state map");
        }));
        assert!(states.is_poisoned(), "the panic must poison the mutex");
        assert!(kernel.modules().is_empty());
    }
}
