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

/// G7: maximum publish recursion depth per thread. A longer chain is a bug
/// (cross-type cycle); the deepest frame is dropped with a warning.
const MAX_EVENT_DEPTH: u8 = 16;

thread_local! {
    /// Current publish recursion depth on this thread (G7 guard).
    static EVENT_DEPTH: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    /// Events currently on the publish stack on this thread (G7 cycle guard).
    static EVENT_STACK: std::cell::RefCell<Vec<EventType>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// RAII reentrancy guard: increments depth and pushes the event on entry;
/// rolls both back on drop — including during unwind from a panicked handler.
struct EventGuard;

impl EventGuard {
    fn enter(event: &EventType) -> Option<Self> {
        let admitted = EVENT_DEPTH.with(|depth| {
            if depth.get() >= MAX_EVENT_DEPTH {
                return false;
            }
            let cycle = EVENT_STACK.with(|stack| stack.borrow().contains(event));
            if cycle {
                return false;
            }
            depth.set(depth.get() + 1);
            EVENT_STACK.with(|stack| stack.borrow_mut().push(event.clone()));
            true
        });
        if admitted { Some(Self) } else { None }
    }
}

impl Drop for EventGuard {
    fn drop(&mut self) {
        EVENT_DEPTH.with(|depth| depth.set(depth.get() - 1));
        EVENT_STACK.with(|stack| {
            let _ = stack.borrow_mut().pop();
        });
    }
}

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
        let Some(_guard) = EventGuard::enter(event) else {
            tracing::warn!(
                ?event,
                "dropping reentrant event publish (depth cap or cycle)"
            );
            return;
        };
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
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                handler.on_event(event, &ctx);
            }));
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
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

    /// Handler that panics inside on_event. BUG-002: a panicking handler must
    /// not poison the bus mutex nor propagate the panic to the publisher.
    struct PanicHandler;
    impl EventHandler for PanicHandler {
        fn on_event(&self, _event: &EventType, _ctx: &KernelContext) {
            panic!("handler boom");
        }
    }

    #[test]
    fn publish_isolates_handler_panic_and_keeps_bus_functional() {
        let bus = EventBus::with_ctx(Arc::new(dummy_context()));
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        bus.register(EventType::ModuleLoaded("m".into()), Arc::new(PanicHandler));
        bus.register(EventType::ModuleLoaded("m".into()), spy.clone());
        bus.publish(&EventType::ModuleLoaded("m".into()));
        bus.publish(&EventType::ModuleLoaded("m".into()));
        assert_eq!(spy.events.lock().unwrap().len(), 2);
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

    /// Handler that re-publishes the SAME event it received (self-trigger).
    /// G7: without the cycle guard this recurses infinitely (stack overflow).
    struct SelfTriggerHandler {
        bus: Arc<EventBus>,
        calls: Arc<AtomicUsize>,
    }
    impl EventHandler for SelfTriggerHandler {
        fn on_event(&self, event: &EventType, _ctx: &KernelContext) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.bus.publish(event); // same event → cycle
        }
    }

    #[test]
    fn publish_drops_same_event_self_trigger_cycle() {
        let bus = Arc::new(EventBus::with_ctx(Arc::new(dummy_context())));
        let calls = Arc::new(AtomicUsize::new(0));
        bus.register(
            EventType::ModuleLoaded("m".into()),
            Arc::new(SelfTriggerHandler {
                bus: bus.clone(),
                calls: calls.clone(),
            }),
        );
        bus.publish(&EventType::ModuleLoaded("m".into()));
        // Cycle detected at depth 1: the handler runs exactly once, no recursion.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Chain m0 → m1 → m2 → …: distinct events, bounded by MAX_EVENT_DEPTH.
    struct ChainHandler {
        bus: Arc<EventBus>,
        next: EventType,
        calls: Arc<AtomicUsize>,
    }
    impl EventHandler for ChainHandler {
        fn on_event(&self, _event: &EventType, _ctx: &KernelContext) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.bus.publish(&self.next);
        }
    }

    #[test]
    fn publish_bounds_cross_type_cycle_by_depth() {
        let bus = Arc::new(EventBus::with_ctx(Arc::new(dummy_context())));
        let calls = Arc::new(AtomicUsize::new(0));
        for i in 0..(MAX_EVENT_DEPTH as usize + 2) {
            bus.register(
                EventType::ModuleLoaded(format!("m{i}").into()),
                Arc::new(ChainHandler {
                    bus: bus.clone(),
                    next: EventType::ModuleLoaded(format!("m{}", i + 1).into()),
                    calls: calls.clone(),
                }),
            );
        }
        bus.publish(&EventType::ModuleLoaded("m0".into()));
        assert!(
            calls.load(Ordering::SeqCst) <= MAX_EVENT_DEPTH as usize,
            "cross-type cycle must be depth-bounded, got {}",
            calls.load(Ordering::SeqCst)
        );
    }

    /// A panic inside a nested publish must still roll back the thread-local
    /// guard, so the same event can be published again afterwards.
    struct DepthPanicHandler {
        panicked: Arc<AtomicBool>,
    }
    impl EventHandler for DepthPanicHandler {
        fn on_event(&self, _event: &EventType, _ctx: &KernelContext) {
            self.panicked.store(true, Ordering::SeqCst);
            panic!("handler boom inside nested publish");
        }
    }
    struct NestedPublisher {
        bus: Arc<EventBus>,
    }
    impl EventHandler for NestedPublisher {
        fn on_event(&self, _event: &EventType, _ctx: &KernelContext) {
            self.bus.publish(&EventType::ModuleFailed("m".into()));
        }
    }

    #[test]
    fn publish_recovers_guard_after_nested_panic() {
        let bus = Arc::new(EventBus::with_ctx(Arc::new(dummy_context())));
        let panicked = Arc::new(AtomicBool::new(false));
        bus.register(
            EventType::ModuleFailed("m".into()),
            Arc::new(DepthPanicHandler {
                panicked: panicked.clone(),
            }),
        );
        bus.register(
            EventType::ModuleLoaded("m".into()),
            Arc::new(NestedPublisher { bus: bus.clone() }),
        );
        bus.publish(&EventType::ModuleLoaded("m".into()));
        assert!(panicked.load(Ordering::SeqCst));
        // Guard must have rolled back during unwind: ModuleLoaded("m") is no
        // longer on the stack, so a fresh publish delivers (not cycle-dropped).
        let spy = Arc::new(SpyHandler {
            events: Arc::new(Mutex::new(vec![])),
        });
        bus.register(EventType::ModuleLoaded("m".into()), spy.clone());
        bus.publish(&EventType::ModuleLoaded("m".into()));
        assert_eq!(spy.events.lock().unwrap().len(), 1);
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
