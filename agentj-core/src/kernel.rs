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
        fn create(&self) -> Result<SessionId, ModuleError> { Ok("s1".into()) }
        fn append(&self, _s: &SessionId, _m: ChatMessage) -> Result<(), ModuleError> { Ok(()) }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> { Ok(vec![]) }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> { Ok(0) }
    }

    struct NoopConfig;
    impl ConfigProvider for NoopConfig {
        fn get_str(&self, _k: &str) -> Option<String> { None }
        fn get_int(&self, _k: &str) -> Option<i64> { None }
        fn providers(&self) -> Vec<(String, String)> { vec![] }
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
        fn id(&self) -> ModuleId { self.id.to_string() }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata { id: self.id.to_string(), name: self.id.to_string(), version: "0.1".into(), description: "".into(), author: "".into() }
        }
        fn init(&mut self, _ctx: &KernelContext, _r: &mut ModuleRegister) -> Result<(), ModuleError> {
            if self.init_fails { Err(ModuleError::Init("boom".into())) } else { Ok(()) }
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            self.shutdowns.lock().unwrap().push(self.id.to_string());
            if let Some(d) = self.shutdown_delay { std::thread::sleep(d); }
            Ok(())
        }
    }

    impl KernelModule for SpyModule {
        fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
            self.handled.lock().unwrap().push(msg.clone());
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
        SpyModule { id, handled, shutdowns, shutdown_delay, init_fails }
    }

    #[tokio::test]
    async fn kernel_routes_message_and_shuts_down() {
        let handled = Arc::new(StdMutex::new(vec![]));
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(Arc::new(ToolRegistry::new()), Arc::new(ProviderRegistry::new()));
        let mut kernel = Kernel::new();
        kernel.register_module(
            Box::new(spy("spy", handled.clone(), shutdowns.clone(), None, false)),
            &dummy_kernel_context(),
            &mut register,
        ).unwrap();
        assert_eq!(kernel.module_state("spy"), Some(ModuleState::Ready));

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::UserRequest {
            session_id: "s1".into(), content: "hello".into(), attachments: vec![],
        }).unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(handled.lock().unwrap().len(), 1);

        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
        assert_eq!(*shutdowns.lock().unwrap(), vec!["spy".to_string()]);
        assert_eq!(kernel.module_state("spy"), Some(ModuleState::Stopped));
    }

    #[tokio::test]
    async fn shutdown_runs_in_reverse_registration_order() {
        let shutdowns = Arc::new(StdMutex::new(vec![]));
        let mut register = ModuleRegister::new(Arc::new(ToolRegistry::new()), Arc::new(ProviderRegistry::new()));
        let mut kernel = Kernel::new();
        kernel.register_module(
            Box::new(spy("a", Arc::new(StdMutex::new(vec![])), shutdowns.clone(), None, false)),
            &dummy_kernel_context(), &mut register,
        ).unwrap();
        kernel.register_module(
            Box::new(spy("b", Arc::new(StdMutex::new(vec![])), shutdowns.clone(), None, false)),
            &dummy_kernel_context(), &mut register,
        ).unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
        assert_eq!(*shutdowns.lock().unwrap(), vec!["b".to_string(), "a".to_string()]);
    }

    #[tokio::test]
    async fn init_failure_is_reported_and_isolated() {
        let mut register = ModuleRegister::new(Arc::new(ToolRegistry::new()), Arc::new(ProviderRegistry::new()));
        let mut kernel = Kernel::new();
        let err = kernel.register_module(
            Box::new(spy("bad", Arc::new(StdMutex::new(vec![])), Arc::new(StdMutex::new(vec![])), None, true)),
            &dummy_kernel_context(), &mut register,
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
        let mut register = ModuleRegister::new(Arc::new(ToolRegistry::new()), Arc::new(ProviderRegistry::new()));
        let mut kernel = Kernel::with_shutdown_timeout(Duration::from_millis(10));
        kernel.register_module(
            Box::new(spy("slow", Arc::new(StdMutex::new(vec![])), Arc::new(StdMutex::new(vec![])), Some(Duration::from_millis(200)), false)),
            &dummy_kernel_context(), &mut register,
        ).unwrap();

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
        let mut register = ModuleRegister::new(Arc::new(ToolRegistry::new()), Arc::new(ProviderRegistry::new()));
        let mut kernel = Kernel::with_timeouts(
            Duration::from_secs(2),          // per-module timeout
            Duration::from_millis(10),       // overall graceful shutdown timeout
        );
        kernel.register_module(
            Box::new(spy("a", Arc::new(StdMutex::new(vec![])), shutdowns.clone(), Some(Duration::from_millis(50)), false)),
            &dummy_kernel_context(), &mut register,
        ).unwrap();
        kernel.register_module(
            Box::new(spy("b", Arc::new(StdMutex::new(vec![])), shutdowns.clone(), Some(Duration::from_millis(50)), false)),
            &dummy_kernel_context(), &mut register,
        ).unwrap();

        let handle = kernel.start();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        let result = handle.await.unwrap();
        assert!(matches!(result, Err(KernelError::ShutdownTimeout(_))));
    }
}
