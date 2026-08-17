//! Event bus (publish/subscribe) — v0.3 T0. Tests land first (TDD); the EventBus
//! implementation is added in the follow-up commit.

use crate::module::{EventHandler, EventType, KernelContext};
use std::sync::{Arc, Mutex, RwLock};

/// Event bus: publish(event) fans out to every handler whose subscribed event matches.
/// v0.3 consumes the v0.1 EventType/EventHandler contract (design §7 预留接口 #4).
/// The KernelContext handed to handlers is stored on the bus (Kernel sets it at first
/// register_module); non-module publishers (session store, MCP manager) use the Kernel's
/// bus in production or `with_ctx` in tests.
/// Handlers are `Arc<dyn EventHandler>` (shared, cloneable — tests register an Arc and
/// keep a clone to assert on after publish). The Kernel converts v0.1's Box registrations
/// from `ModuleRegister.events` via `Arc::from(box)` at register time.
/// Locks are handled poison-safe: a panicked handler thread must not crash the kernel.
type HandlerEntry = (EventType, Arc<dyn EventHandler>);

pub struct EventBus {
    handlers: Arc<Mutex<Vec<HandlerEntry>>>,
    ctx: Arc<RwLock<Option<Arc<KernelContext>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(Mutex::new(vec![])),
            ctx: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_ctx(ctx: Arc<KernelContext>) -> Self {
        Self {
            handlers: Arc::new(Mutex::new(vec![])),
            ctx: Arc::new(RwLock::new(Some(ctx))),
        }
    }

    pub fn set_ctx(&self, ctx: Option<Arc<KernelContext>>) {
        if let Ok(mut guard) = self.ctx.write() {
            *guard = ctx;
        }
    }

    pub fn register(&self, event: EventType, handler: Arc<dyn EventHandler>) {
        if let Ok(mut guard) = self.handlers.lock() {
            guard.push((event, handler));
        }
    }

    pub fn publish(&self, event: &EventType) {
        let ctx = match self.ctx.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        let Some(ctx) = ctx else { return };
        let snapshot = {
            let Ok(handlers) = self.handlers.lock() else {
                return;
            };
            handlers
                .iter()
                .filter(|(sub, _)| sub == event)
                .cloned()
                .collect::<Vec<HandlerEntry>>()
        };
        for (_, handler) in snapshot {
            handler.on_event(event, &ctx);
        }
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.lock().map(|guard| guard.len()).unwrap_or(0)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::*;
    use crate::kernel::Kernel;
    use crate::module::*;
    use crate::types::*;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    struct SpyHandler {
        events: Arc<Mutex<Vec<EventType>>>,
    }
    impl EventHandler for SpyHandler {
        fn on_event(&self, event: &EventType, _ctx: &KernelContext) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    /// Minimal module used to exercise the kernel's lifecycle event publishing.
    struct LifecycleModule;
    impl Module for LifecycleModule {
        fn id(&self) -> ModuleId {
            "lm".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "lm".into(),
                name: "lm".into(),
                version: "1".into(),
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
    impl KernelModule for LifecycleModule {
        fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
            Ok(KernelResponse::Handled)
        }
    }

    fn dummy_context() -> KernelContext {
        KernelContext {
            session: Arc::new(NoopSession),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(NoopConfig),
        }
    }
    struct NoopSession;
    impl SessionStore for NoopSession {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("s".into())
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

    #[test]
    fn event_bus_publishes_to_matching_handlers() {
        let bus = EventBus::with_ctx(Arc::new(dummy_context()));
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        bus.register(EventType::ModuleLoaded("m".into()), spy.clone());
        bus.register(EventType::ModuleStopped("m".into()), spy.clone());
        bus.publish(&EventType::ModuleLoaded("m".into()));
        bus.publish(&EventType::ModuleStopped("m".into()));
        assert_eq!(spy.events.lock().unwrap().len(), 2);
    }

    #[test]
    fn event_bus_filters_non_matching_events() {
        let bus = EventBus::with_ctx(Arc::new(dummy_context()));
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        bus.register(EventType::ModuleLoaded("m".into()), spy.clone());
        bus.publish(&EventType::ModuleFailed("m".into()));
        assert!(spy.events.lock().unwrap().is_empty());
    }

    #[test]
    fn event_bus_without_ctx_is_a_noop() {
        let bus = EventBus::new();
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        bus.register(EventType::ModuleLoaded("m".into()), spy.clone());
        bus.publish(&EventType::ModuleLoaded("m".into()));
        assert!(spy.events.lock().unwrap().is_empty());
    }

    /// Handler that re-enters the bus (register + publish) from inside on_event.
    /// BUG-001: publish held the handlers mutex while invoking handlers, so
    /// re-entry deadlocked on the (non-reentrant) std mutex.
    struct ReentrantHandler {
        bus: Arc<EventBus>,
        tx: Mutex<Option<mpsc::Sender<()>>>,
    }
    impl EventHandler for ReentrantHandler {
        fn on_event(&self, _event: &EventType, _ctx: &KernelContext) {
            self.bus.register(
                EventType::ModuleFailed("m".into()),
                Arc::new(SpyHandler {
                    events: Arc::new(Mutex::new(vec![])),
                }),
            );
            self.bus.publish(&EventType::ModuleFailed("m".into()));
            if let Some(tx) = self.tx.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
        }
    }

    #[test]
    fn publish_snapshots_handlers_so_handler_can_reenter_bus() {
        let bus = Arc::new(EventBus::with_ctx(Arc::new(dummy_context())));
        let (tx, rx) = mpsc::channel();
        bus.register(
            EventType::ModuleLoaded("m".into()),
            Arc::new(ReentrantHandler {
                bus: bus.clone(),
                tx: Mutex::new(Some(tx)),
            }),
        );
        let bus_clone = bus.clone();
        let t = std::thread::spawn(move || {
            bus_clone.publish(&EventType::ModuleLoaded("m".into()));
        });
        // A deadlock (old impl) never sends on the channel; the timeout fails the test.
        rx.recv_timeout(Duration::from_secs(5))
            .expect("handler re-entering the bus must not deadlock publish");
        t.join().expect("publish thread must terminate");
    }

    #[tokio::test]
    async fn kernel_publishes_lifecycle_events() {
        // design §模块生命周期状态机「查询与事件」: Kernel auto-publishes ModuleLoaded on
        // register success and ModuleStopped after graceful shutdown.
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        let mut kernel = Kernel::new();
        kernel
            .events()
            .register(EventType::ModuleLoaded("lm".into()), spy.clone());
        kernel
            .events()
            .register(EventType::ModuleStopped("lm".into()), spy.clone());
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        kernel
            .register_module(Box::new(LifecycleModule), &dummy_context(), &mut register)
            .unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();

        let events = spy.events.lock().unwrap();
        assert!(
            events.contains(&EventType::ModuleLoaded("lm".into())),
            "ModuleLoaded must be published on register"
        );
        assert!(
            events.contains(&EventType::ModuleStopped("lm".into())),
            "ModuleStopped must be published after shutdown"
        );
    }

    /// init() failure must publish ModuleFailed (design §模块生命周期状态机「查询与事件」).
    struct FailingModule;
    impl Module for FailingModule {
        fn id(&self) -> ModuleId {
            "fm".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "fm".into(),
                name: "fm".into(),
                version: "1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(
            &mut self,
            _ctx: &KernelContext,
            _r: &mut ModuleRegister,
        ) -> Result<(), ModuleError> {
            Err(ModuleError::Init("boom".into()))
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl KernelModule for FailingModule {
        fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
            Ok(KernelResponse::Handled)
        }
    }

    #[tokio::test]
    async fn kernel_publishes_module_failed_on_init_error() {
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        let mut kernel = Kernel::new();
        kernel
            .events()
            .register(EventType::ModuleFailed("fm".into()), spy.clone());
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        assert!(
            kernel
                .register_module(Box::new(FailingModule), &dummy_context(), &mut register)
                .is_err()
        );
        assert!(
            spy.events
                .lock()
                .unwrap()
                .contains(&EventType::ModuleFailed("fm".into()))
        );
    }

    #[test]
    fn v02_event_type_variants_exist() {
        let loading = EventType::ModuleLoading("m".into());
        let disabled = EventType::ModuleDisabled("m".into());
        let enabled = EventType::ModuleEnabled("m".into());
        let reloaded = EventType::ModuleReloaded("m".into());
        let connecting = EventType::McpConnecting("srv".into());
        let connected = EventType::McpConnected("srv".into());
        let closed = EventType::McpClosed("srv".into());
        assert_ne!(loading, enabled);
        assert_ne!(disabled, reloaded);
        assert_ne!(connecting, closed);
        assert_ne!(connected, closed);
        assert_eq!(closed, EventType::McpClosed("srv".into()));
    }
}
