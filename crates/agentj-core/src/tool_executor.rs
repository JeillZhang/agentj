use crate::error::ModuleError;
use crate::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister, ToolExecutionOutcome, ToolSource,
};
use crate::permission::PermissionContext;
use crate::types::ModuleId;
use std::path::PathBuf;

/// Executes tool calls routed by `KernelMessage::ToolCallRequest`.
///
/// The permission gate stays at `ToolRegistry::execute_async` (design #17)
/// — this module only moves the execution boundary from `AgentLoop`
/// into a kernel-routed module.
pub struct ToolExecutorModule {
    ctx: KernelContext,
    workspace_root: PathBuf,
}

impl ToolExecutorModule {
    pub fn new(ctx: KernelContext) -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            ctx,
            workspace_root,
        }
    }
}

impl Module for ToolExecutorModule {
    fn id(&self) -> ModuleId {
        "tool_executor".into()
    }

    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "tool_executor".into(),
            name: "ToolExecutor".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Bus-mediated tool execution".into(),
            author: "AgentJ".into(),
        }
    }

    fn init(
        &mut self,
        _ctx: &KernelContext,
        _register: &mut ModuleRegister,
    ) -> Result<(), ModuleError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ModuleError> {
        Ok(())
    }
}

impl KernelModule for ToolExecutorModule {
    fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        if let KernelMessage::ToolCallRequest {
            session_id,
            tool_call,
            reply,
            ..
        } = msg
        {
            let ctx = self.ctx.clone();
            let reply = reply.clone();
            let tool_call = tool_call.clone();
            let session_id = session_id.clone();
            // Session-level path base (design #3 ③): permission context carries the
            // resolved root; T7's execute_async rebases args with it. Missing session
            // or unset root falls back to the process CWD captured at construction.
            let workspace_root = ctx
                .session
                .project_root(&session_id)
                .unwrap_or_else(|| self.workspace_root.clone());
            tokio::spawn(async move {
                let source = ctx
                    .tools
                    .source_of(&tool_call.name)
                    .unwrap_or(ToolSource::Builtin);
                let perm_ctx = PermissionContext {
                    session_id: session_id.clone(),
                    tool: tool_call.name.clone(),
                    source,
                    args: tool_call.arguments.clone(),
                    workspace_root,
                };
                let result = ctx
                    .tools
                    .execute_async(&tool_call.name, tool_call.arguments.clone(), &perm_ctx)
                    .await
                    .map(|result| ToolExecutionOutcome {
                        result,
                        artifacts: vec![],
                    });
                if let Some(tx) = reply.lock().unwrap().take() {
                    let _ = tx.send(result);
                }
            });
        }
        Ok(KernelResponse::Handled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ToolError;
    use crate::kernel::Kernel;
    use crate::module::{Module, ProviderRegistry, Tool, ToolExecutionOutcome, ToolRegistry};
    use crate::permission::{
        PermissionChecker, PermissionContext as PcContext, PermissionDecision, PermissionRequestId,
    };
    use crate::types::ToolCall;
    use async_trait::async_trait;
    use std::sync::Arc;

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
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"done": true}))
        }
    }

    struct AllowAll;
    #[async_trait]
    impl PermissionChecker for AllowAll {
        fn check(&self, _ctx: &PcContext) -> PermissionDecision {
            PermissionDecision::Allow
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct DenyAll;
    #[async_trait]
    impl PermissionChecker for DenyAll {
        fn check(&self, _ctx: &PcContext) -> PermissionDecision {
            PermissionDecision::Deny {
                reason: "denied".into(),
            }
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Deny {
                reason: "denied".into(),
            }
        }
    }

    struct RecordingChecker {
        roots: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    }
    #[async_trait]
    impl PermissionChecker for RecordingChecker {
        fn check(&self, ctx: &PcContext) -> PermissionDecision {
            self.roots.lock().unwrap().push(ctx.workspace_root.clone());
            PermissionDecision::Allow
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct MockSession {
        root: Option<PathBuf>,
    }
    impl crate::module::SessionStore for MockSession {
        fn create(&self) -> Result<crate::types::SessionId, crate::error::ModuleError> {
            Ok("s1".into())
        }
        fn project_root(&self, _s: &crate::types::SessionId) -> Option<PathBuf> {
            self.root.clone()
        }
        fn append(
            &self,
            _s: &crate::types::SessionId,
            _m: crate::types::ChatMessage,
        ) -> Result<(), crate::error::ModuleError> {
            Ok(())
        }
        fn get(
            &self,
            _s: &crate::types::SessionId,
        ) -> Result<Vec<crate::types::ChatMessage>, crate::error::ModuleError> {
            Ok(vec![])
        }
        fn estimate_tokens(
            &self,
            _s: &crate::types::SessionId,
        ) -> Result<usize, crate::error::ModuleError> {
            Ok(0)
        }
    }

    struct MockConfig;
    impl crate::module::ConfigProvider for MockConfig {
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

    fn executor_ctx(checker: Arc<dyn PermissionChecker>) -> KernelContext {
        executor_ctx_with_store(checker, Arc::new(MockSession { root: None }))
    }

    fn executor_ctx_with_store(
        checker: Arc<dyn PermissionChecker>,
        session: Arc<dyn crate::module::SessionStore>,
    ) -> KernelContext {
        let tools = ToolRegistry::new();
        tools.register(Arc::new(DummyTool));
        tools.set_permission_checker(checker);
        KernelContext {
            session,
            tools: Arc::new(tools),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig),
        }
    }

    #[tokio::test]
    async fn tool_call_request_is_executed_and_replied() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn tool_executor_replies_with_outcome() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        let outcome = reply.expect("successful tool call must reply Ok");
        assert_eq!(outcome.result, serde_json::json!({"done": true}));
        assert!(
            outcome.artifacts.is_empty(),
            "artifacts stay empty until T13/T14/T16"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn denied_tool_call_replies_permission_denied() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(DenyAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert!(matches!(reply, Err(ToolError::PermissionDenied { .. })));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn executor_uses_session_project_root() {
        let mut kernel = Kernel::new();
        let roots = Arc::new(std::sync::Mutex::new(Vec::new()));
        let checker = Arc::new(RecordingChecker {
            roots: Arc::clone(&roots),
        });
        let session = Arc::new(MockSession {
            root: Some(PathBuf::from("/tmp/proj")),
        });
        let ctx = executor_ctx_with_store(checker, session);
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({"path": "src/main.rs"}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        let seen = roots.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![PathBuf::from("/tmp/proj")],
            "permission context must use the session project_root"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn executor_without_project_root_uses_process_cwd() {
        let mut kernel = Kernel::new();
        let roots = Arc::new(std::sync::Mutex::new(Vec::new()));
        let checker = Arc::new(RecordingChecker {
            roots: Arc::clone(&roots),
        });
        let session = Arc::new(MockSession { root: None });
        let ctx = executor_ctx_with_store(checker, session);
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        let cwd = std::env::current_dir().expect("process cwd is available");
        assert_eq!(
            roots.lock().unwrap().clone(),
            vec![cwd],
            "unset project_root must preserve the process CWD fallback"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn taken_reply_is_dropped_without_panic() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        // Take the sender immediately so executor finds None.
        let reply_slot = Arc::new(std::sync::Mutex::new(Some(tx)));
        // Simulate already-taken by taking before send.
        reply_slot.lock().unwrap().take();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: reply_slot,
            })
            .unwrap();
        // Give executor time to run; must not panic.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn module_id_is_tool_executor() {
        let ctx = executor_ctx(Arc::new(AllowAll));
        let m = ToolExecutorModule::new(ctx);
        assert_eq!(m.id(), "tool_executor");
    }

    #[test]
    fn non_tool_message_is_ignored() {
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut m = ToolExecutorModule::new(ctx);
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let msg = KernelMessage::UserRequest {
            session_id: "s1".into(),
            content: "hi".into(),
            attachments: vec![],
            reply: tx,
        };
        let res = m.handle(&msg).unwrap();
        assert!(matches!(res, KernelResponse::Handled));
    }
}
