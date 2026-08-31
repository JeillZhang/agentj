use std::sync::Arc;

use crate::error::ModuleError;
use crate::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister,
};
use crate::resolver::KernelService;
use crate::turn_runner::TurnRunner;
use crate::types::ModuleId;

pub struct AgentLoopModule {
    runner: TurnRunner,
}

impl AgentLoopModule {
    pub fn new(runner: TurnRunner) -> Self {
        Self { runner }
    }

    pub fn runner(&self) -> TurnRunner {
        self.runner.clone()
    }
}

impl Module for AgentLoopModule {
    fn id(&self) -> ModuleId {
        "agent_loop".into()
    }

    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "agent_loop".into(),
            name: "AgentLoop".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Turn orchestration (bus-mediated)".into(),
            author: "AgentJ".into(),
        }
    }

    fn init(
        &mut self,
        _ctx: &KernelContext,
        register: &mut ModuleRegister,
    ) -> Result<(), ModuleError> {
        let runner = self.runner.clone();
        let service = AgentCancelService { runner };
        register.register_kernel_service("agent".into(), Arc::new(service))?;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ModuleError> {
        self.runner.shutdown();
        Ok(())
    }
}

impl KernelModule for AgentLoopModule {
    fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        if let KernelMessage::UserRequest {
            session_id,
            content,
            attachments: _,
            reply,
        } = msg
        {
            self.runner
                .spawn_turn(session_id.clone(), content.clone(), None, reply.clone())?;
        }
        Ok(KernelResponse::Handled)
    }
}

struct AgentCancelService {
    runner: TurnRunner,
}

impl KernelService for AgentCancelService {
    fn call(&self, _method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let sid = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or("missing session_id")?;
        if self.runner.cancel(&sid.to_string()) {
            Ok(serde_json::json!({}))
        } else {
            Err("no active turn".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ModelError, ModuleError};
    use crate::kernel::Kernel;
    use crate::module::{
        ConfigProvider, ContextPipeline, KernelContext, Module, ModuleMetadata, ModuleRegister,
        ProviderRegistry, SessionStore, ToolRegistry,
    };
    use crate::types::{
        AgentEvent, ChatMessage, ModelEvent, ModelOptions, Role, ToolCall, ToolDefinition,
    };
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use serde_json::Value;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockSession {
        history: StdMutex<Vec<ChatMessage>>,
    }
    impl SessionStore for MockSession {
        fn create(&self) -> Result<crate::types::SessionId, ModuleError> {
            Ok("test-session".into())
        }
        fn append(
            &self,
            _sid: &crate::types::SessionId,
            msg: ChatMessage,
        ) -> Result<(), ModuleError> {
            self.history.lock().unwrap().push(msg);
            Ok(())
        }
        fn get(&self, _sid: &crate::types::SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(self.history.lock().unwrap().clone())
        }
        fn estimate_tokens(&self, _sid: &crate::types::SessionId) -> Result<usize, ModuleError> {
            Ok(0)
        }
    }

    struct MockConfig;
    impl ConfigProvider for MockConfig {
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

    struct MockPipeline;
    impl ContextPipeline for MockPipeline {
        fn assemble(
            &self,
            system: &str,
            history: &[ChatMessage],
            _tool_results: &[(ToolCall, String)],
        ) -> Vec<ChatMessage> {
            let mut msgs = vec![ChatMessage::system(system)];
            msgs.extend_from_slice(history);
            msgs
        }
        fn reflux(&self, history: &mut Vec<ChatMessage>, tool_call: &ToolCall, result: String) {
            history.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![tool_call.clone()],
            });
            history.push(ChatMessage::tool_result(&tool_call.id, result));
        }
    }

    fn tc() -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: "dummy".into(),
            arguments: serde_json::json!({}),
        }
    }

    struct ScriptedProvider {
        calls: Arc<AtomicUsize>,
        tool: ToolCall,
    }
    #[async_trait]
    impl crate::module::ModelProvider for ScriptedProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let events: Vec<Result<ModelEvent, ModelError>> = if n == 0 {
                vec![
                    Ok(ModelEvent::Delta("Hello".into())),
                    Ok(ModelEvent::ToolCall(self.tool.clone())),
                    Ok(ModelEvent::Done),
                ]
            } else {
                vec![Ok(ModelEvent::Delta(" done".into())), Ok(ModelEvent::Done)]
            };
            Ok(Box::pin(stream::iter(events)))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<crate::types::ChatResponse, ModelError> {
            Ok(crate::types::ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    struct BlockingSpy;
    impl Module for BlockingSpy {
        fn id(&self) -> ModuleId {
            "tool_spy".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "tool_spy".into(),
                name: "spy".into(),
                version: "0.1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(&mut self, _c: &KernelContext, _r: &mut ModuleRegister) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl crate::module::KernelModule for BlockingSpy {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg
                && let Some(tx) = reply.lock().unwrap().take()
            {
                std::mem::forget(tx);
            }
            Ok(crate::module::KernelResponse::Handled)
        }
    }

    fn loop_test_ctx() -> KernelContext {
        KernelContext {
            session: Arc::new(MockSession {
                history: StdMutex::new(vec![]),
            }),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig),
        }
    }

    #[tokio::test]
    async fn user_request_runs_turn_and_cancel_signals_it() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let mut register = ModuleRegister::with_services(
            Arc::clone(&ctx.tools),
            Arc::clone(&ctx.providers),
            kernel.services(),
        );
        kernel
            .register_module(Box::new(BlockingSpy), &ctx, &mut register)
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            kernel.sender(),
            "tool_spy".into(),
        );
        kernel
            .register_module(
                Box::new(AgentLoopModule::new(runner.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        kernel
            .sender()
            .send(KernelMessage::UserRequest {
                session_id: sid.clone(),
                content: "hi".into(),
                attachments: vec![],
                reply: tx.clone(),
            })
            .unwrap();
        let mut saw_tool_call = false;
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::ToolCall(_) => {
                    saw_tool_call = true;
                    break;
                }
                AgentEvent::Error(e) => panic!("unexpected error: {e}"),
                _ => {}
            }
        }
        assert!(saw_tool_call, "turn must suspend at tool boundary");
        let (c_tx, c_rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "agent".into(),
                method: "cancel".into(),
                params: serde_json::json!({"session_id": sid}),
                reply: Arc::new(StdMutex::new(Some(c_tx))),
            })
            .unwrap();
        let cancel_reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), c_rx)
                .await
                .expect("cancel reply must arrive")
                .expect("sender dropped");
        assert!(cancel_reply.is_ok());
        let mut saw_cancelled = false;
        while let Some(ev) = rx.recv().await {
            if let AgentEvent::Error(e) = ev {
                assert_eq!(e, "cancelled");
                saw_cancelled = true;
                break;
            }
        }
        assert!(saw_cancelled);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancel_service_unknown_session_returns_error() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let mut register = ModuleRegister::with_services(
            Arc::clone(&ctx.tools),
            Arc::clone(&ctx.providers),
            kernel.services(),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "agent".into(),
                method: "cancel".into(),
                params: serde_json::json!({"session_id": "no-such-session"}),
                reply: Arc::new(StdMutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert!(reply.is_err());
        assert_eq!(reply.unwrap_err(), "no active turn");
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }
}
