use crate::kernel::KernelSender;
use crate::module::KernelMessage;
use crate::resolver::AsyncKernelService;
use crate::turn_runner::TurnRunner;
use crate::types::{AgentEvent, RunOverrides, SessionId};
use std::sync::Arc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub prompt: String,
    pub system_prompt: Option<String>,
}

#[derive(Debug)]
pub struct SubAgentResult {
    pub session_id: SessionId,
    pub final_response: String,
}

/// Server side: registered as async kernel service "subagent".
/// Runs a sub-agent turn through the shared TurnRunner on a fresh session.
pub struct SubAgentService {
    runner: TurnRunner,
}

impl SubAgentService {
    pub fn new(runner: TurnRunner) -> Self {
        Self { runner }
    }
}

#[async_trait::async_trait]
impl AsyncKernelService for SubAgentService {
    async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if method != "dispatch" {
            return Err(format!("unknown method: {method}"));
        }
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing prompt".to_string())?
            .to_string();
        let system_prompt = params
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .map(String::from);
        let sid = self.runner.create_session().map_err(|e| e.to_string())?;
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        self.runner
            .spawn_turn(
                sid.clone(),
                prompt,
                system_prompt,
                RunOverrides::default(),
                tx,
            )
            .map_err(|e| e.to_string())?;
        let mut final_response = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Delta(d) => final_response.push_str(&d),
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        Ok(serde_json::json!({ "session_id": sid, "final_response": final_response }))
    }
}

/// Client side: sends ServiceCall{service:"subagent"} and awaits the reply.
#[derive(Clone)]
pub struct Dispatcher {
    sender: KernelSender,
}

impl Dispatcher {
    pub fn new(sender: KernelSender) -> Self {
        Self { sender }
    }

    pub fn dispatch(&self, spec: TaskSpec) -> JoinHandle<Result<SubAgentResult, String>> {
        let sender = self.sender.clone();
        tokio::spawn(async move { Self::do_dispatch(sender, spec).await })
    }

    async fn do_dispatch(sender: KernelSender, spec: TaskSpec) -> Result<SubAgentResult, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        sender
            .send(KernelMessage::ServiceCall {
                service: "subagent".into(),
                method: "dispatch".into(),
                params: serde_json::json!({
                    "prompt": spec.prompt,
                    "system_prompt": spec.system_prompt,
                }),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .map_err(|e| e.to_string())?;
        let reply: Result<serde_json::Value, String> = rx
            .await
            .map_err(|_| "subagent service dropped".to_string())?;
        let value = reply?;
        Ok(SubAgentResult {
            session_id: value["session_id"].as_str().unwrap_or_default().to_string(),
            final_response: value["final_response"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ModelError, ModuleError};
    use crate::kernel::Kernel;
    use crate::module::{
        ConfigProvider, KernelMessage, KernelModule, ModelProvider, Module, ModuleMetadata,
        ModuleRegister, ProviderRegistry, SessionStore, ToolExecutionOutcome, ToolRegistry,
    };
    use crate::types::{ChatMessage, ModelEvent, ModelOptions, Role, ToolCall, ToolDefinition};
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        chunks: Vec<String>,
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            use crate::types::ModelEvent;
            let chunks: Vec<_> = self
                .chunks
                .iter()
                .map(|s| Ok(ModelEvent::Delta(s.clone())))
                .chain(std::iter::once(Ok(ModelEvent::Done)))
                .collect();
            Ok(Box::pin(stream::iter(chunks)))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<crate::types::ChatResponse, ModelError> {
            Ok(crate::types::ChatResponse {
                content: "mock".into(),
                tool_calls: vec![],
            })
        }
    }

    struct MockSession {
        history: Mutex<Vec<ChatMessage>>,
    }
    impl SessionStore for MockSession {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("s-test".into())
        }
        fn append(&self, _s: &SessionId, m: ChatMessage) -> Result<(), ModuleError> {
            self.history.lock().unwrap().push(m);
            Ok(())
        }
        fn get(&self, _s: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(self.history.lock().unwrap().clone())
        }
        fn estimate_tokens(&self, _s: &SessionId) -> Result<usize, ModuleError> {
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
    impl crate::module::ContextPipeline for MockPipeline {
        fn assemble(
            &self,
            system: &str,
            history: &[ChatMessage],
            _t: &[(ToolCall, String)],
        ) -> Vec<ChatMessage> {
            let mut msgs = vec![ChatMessage::system(system)];
            msgs.extend_from_slice(history);
            msgs
        }
        fn reflux(&self, history: &mut Vec<ChatMessage>, tc: &ToolCall, result: String) {
            history.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![tc.clone()],
            });
            history.push(ChatMessage::tool_result(&tc.id, result));
        }
    }

    fn loop_test_ctx() -> crate::module::KernelContext {
        crate::module::KernelContext {
            session: Arc::new(MockSession {
                history: Mutex::new(vec![]),
            }),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig),
        }
    }

    struct ToolSpy;
    impl Module for ToolSpy {
        fn id(&self) -> crate::types::ModuleId {
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
        fn init(
            &mut self,
            _c: &crate::module::KernelContext,
            _r: &mut ModuleRegister,
        ) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl KernelModule for ToolSpy {
        fn handle(
            &mut self,
            msg: &crate::module::KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let crate::module::KernelMessage::ToolCallRequest { reply, .. } = msg {
                let reply = reply.clone();
                let mut guard = reply.lock().unwrap();
                if let Some(tx) = guard.take() {
                    let _ = tx.send(Ok(ToolExecutionOutcome {
                        result: serde_json::json!({"result": "ok"}),
                        artifacts: vec![],
                    }));
                }
            }
            Ok(crate::module::KernelResponse::Handled)
        }
    }

    #[tokio::test]
    async fn dispatcher_runs_sub_agent_via_service_call() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(MockProvider {
                chunks: vec!["Hello".into(), " world".into()],
            }),
        );
        let mut register = ModuleRegister::with_services(
            Arc::clone(&ctx.tools),
            Arc::clone(&ctx.providers),
            kernel.services(),
        );
        kernel
            .register_module(Box::new(ToolSpy), &ctx, &mut register)
            .unwrap();
        let runner = TurnRunner::new(
            ctx,
            Arc::new(MockPipeline),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_spy".into(),
        );
        kernel.services().register_kernel_async_service(
            "subagent".into(),
            Arc::new(SubAgentService::new(runner)),
        );
        let handle = kernel.start();

        let dispatcher = Dispatcher::new(kernel.sender());
        let a = dispatcher.dispatch(TaskSpec {
            prompt: "task a".into(),
            system_prompt: None,
        });
        let ra = a.await.unwrap().unwrap();
        assert_eq!(ra.final_response, "Hello world");
        assert!(!ra.session_id.is_empty());
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn subagent_dispatch_unknown_method_returns_error() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let runner = TurnRunner::new(
            ctx,
            Arc::new(MockPipeline),
            "sys".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel.services().register_kernel_async_service(
            "subagent".into(),
            Arc::new(SubAgentService::new(runner)),
        );
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "subagent".into(),
                method: "bogus".into(),
                params: serde_json::json!({}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<serde_json::Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert!(reply.is_err());
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    // Acceptance: sub-agent tool calls go through the bus (ToolCallRequest)
    struct ScriptedProvider {
        calls: Arc<AtomicUsize>,
        tool: ToolCall,
    }
    #[async_trait]
    impl ModelProvider for ScriptedProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            use crate::types::ModelEvent;
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let events: Vec<Result<ModelEvent, ModelError>> = if n == 0 {
                vec![
                    Ok(ModelEvent::Delta("Hi".into())),
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

    struct CountingSpy {
        count: Arc<AtomicUsize>,
    }
    impl Module for CountingSpy {
        fn id(&self) -> crate::types::ModuleId {
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
        fn init(
            &mut self,
            _c: &crate::module::KernelContext,
            _r: &mut ModuleRegister,
        ) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl KernelModule for CountingSpy {
        fn handle(
            &mut self,
            msg: &crate::module::KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let crate::module::KernelMessage::ToolCallRequest { reply, .. } = msg {
                self.count.fetch_add(1, Ordering::SeqCst);
                let reply = reply.clone();
                let mut guard = reply.lock().unwrap();
                if let Some(tx) = guard.take() {
                    let _ = tx.send(Ok(ToolExecutionOutcome {
                        result: serde_json::json!("ok"),
                        artifacts: vec![],
                    }));
                }
            }
            Ok(crate::module::KernelResponse::Handled)
        }
    }

    #[tokio::test]
    async fn subagent_tool_calls_go_through_bus() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let tool_hits = Arc::new(AtomicUsize::new(0));
        let spy = CountingSpy {
            count: Arc::clone(&tool_hits),
        };
        let mut register = ModuleRegister::with_services(
            Arc::clone(&ctx.tools),
            Arc::clone(&ctx.providers),
            kernel.services(),
        );
        kernel
            .register_module(Box::new(spy), &ctx, &mut register)
            .unwrap();
        ctx.providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
            }),
        );
        let runner = TurnRunner::new(
            ctx,
            Arc::new(MockPipeline),
            "sys".into(),
            kernel.sender(),
            "tool_spy".into(),
        );
        kernel.services().register_kernel_async_service(
            "subagent".into(),
            Arc::new(SubAgentService::new(runner)),
        );
        let handle = kernel.start();
        let dispatcher = Dispatcher::new(kernel.sender());
        let h = dispatcher.dispatch(TaskSpec {
            prompt: "hi".into(),
            system_prompt: None,
        });
        let res = h.await.unwrap().unwrap();
        assert_eq!(res.final_response, "Hi done");
        assert_eq!(tool_hits.load(Ordering::SeqCst), 1);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn dispatcher_unknown_method_via_client_returns_error() {
        let mut kernel = Kernel::new();
        let ctx = loop_test_ctx();
        let runner = TurnRunner::new(
            ctx,
            Arc::new(MockPipeline),
            "sys".into(),
            kernel.sender(),
            "tool_spy".into(),
        );
        kernel.services().register_kernel_async_service(
            "subagent".into(),
            Arc::new(SubAgentService::new(runner)),
        );
        let handle = kernel.start();
        // Direct ServiceCall with unknown method must not hang
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ServiceCall {
                service: "subagent".into(),
                method: "unknown".into(),
                params: serde_json::json!({"prompt": "hi"}),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<serde_json::Value, String> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert!(reply.is_err(), "unknown method must produce Err");
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }
}
