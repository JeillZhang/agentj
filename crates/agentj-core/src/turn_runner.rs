use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::artifacts::TurnArtifact;
use crate::error::ModuleError;
use crate::kernel::KernelSender;
use crate::module::{ContextPipeline, KernelContext, KernelMessage, ToolExecutionOutcome};
use crate::types::{
    AgentEvent, ChatMessage, ModelEvent, ModelOptions, ModuleId, SessionId, TokenUsage, ToolCall,
};

pub struct TurnHandle {
    cancel_tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[allow(clippy::type_complexity)]
#[derive(Clone)]
pub struct TurnRunner {
    ctx: KernelContext,
    pipeline: Arc<dyn ContextPipeline>,
    default_system_prompt: String,
    max_tool_iterations: usize,
    on_turn_complete: Option<Arc<dyn Fn(&SessionId, Option<TokenUsage>) + Send + Sync>>,
    sender: KernelSender,
    tool_executor_id: ModuleId,
    turns: Arc<Mutex<HashMap<SessionId, TurnHandle>>>,
    todo_revisions: Arc<Mutex<HashMap<SessionId, u64>>>,
}

impl TurnRunner {
    pub fn new(
        ctx: KernelContext,
        pipeline: Arc<dyn ContextPipeline>,
        default_system_prompt: String,
        sender: KernelSender,
        tool_executor_id: ModuleId,
    ) -> Self {
        let max_tool_iterations = ctx
            .config
            .get_int("agent.max_tool_iterations")
            .and_then(|v| if v > 0 { Some(v as usize) } else { None })
            .or_else(|| {
                ctx.config
                    .get_int("max_tool_iterations")
                    .and_then(|v| if v > 0 { Some(v as usize) } else { None })
            })
            .unwrap_or(50);
        Self {
            ctx,
            pipeline,
            default_system_prompt,
            max_tool_iterations,
            on_turn_complete: None,
            sender,
            tool_executor_id,
            turns: Arc::new(Mutex::new(HashMap::new())),
            todo_revisions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn set_on_turn_complete(
        &mut self,
        f: Arc<dyn Fn(&SessionId, Option<TokenUsage>) + Send + Sync>,
    ) {
        self.on_turn_complete = Some(f);
    }

    pub fn create_session(&self) -> Result<SessionId, ModuleError> {
        self.ctx.session.create()
    }

    #[allow(clippy::collapsible_if)]
    pub fn cancel(&self, session_id: &SessionId) -> bool {
        if let Ok(turns) = self.turns.lock() {
            if let Some(handle) = turns.get(session_id)
                && let Some(tx) = handle.cancel_tx.lock().unwrap().take()
            {
                let _ = tx.send(());
                return true;
            }
        }
        false
    }

    pub fn shutdown(&self) {
        if let Ok(mut turns) = self.turns.lock() {
            for (_, handle) in turns.drain() {
                if let Some(tx) = handle.cancel_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            }
        }
    }

    pub fn spawn_turn(
        &self,
        session_id: SessionId,
        content: String,
        system_prompt: Option<String>,
        reply: mpsc::Sender<AgentEvent>,
    ) -> Result<(), ModuleError> {
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let ctx = self.ctx.clone();
        let pipeline = Arc::clone(&self.pipeline);
        let system_prompt = system_prompt.unwrap_or_else(|| self.default_system_prompt.clone());
        let max_iter = self.max_tool_iterations;
        let on_done = self.on_turn_complete.clone();
        let sender = self.sender.clone();
        let tool_executor_id = self.tool_executor_id.clone();
        let turns = Arc::clone(&self.turns);
        let todo_revisions = Arc::clone(&self.todo_revisions);
        {
            let mut map = self
                .turns
                .lock()
                .map_err(|_| ModuleError::Init("turns poisoned".into()))?;
            map.insert(
                session_id.clone(),
                TurnHandle {
                    cancel_tx: std::sync::Mutex::new(Some(cancel_tx)),
                },
            );
        }
        tokio::spawn(async move {
            let result = Self::run_turn(
                ctx,
                pipeline,
                session_id.clone(),
                content,
                system_prompt,
                max_iter,
                reply.clone(),
                sender,
                tool_executor_id,
                todo_revisions,
                cancel_rx,
                on_done.clone(),
            )
            .await;
            if let Err(e) = &result {
                let _ = reply.send(AgentEvent::Error(e.clone())).await;
            }
            if let Ok(mut map) = turns.lock() {
                map.remove(&session_id);
            }
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    async fn run_turn(
        ctx: KernelContext,
        pipeline: Arc<dyn ContextPipeline>,
        session_id: SessionId,
        user_input: String,
        system_prompt: String,
        max_iter: usize,
        reply: mpsc::Sender<AgentEvent>,
        sender: KernelSender,
        tool_executor_id: ModuleId,
        todo_revisions: Arc<Mutex<HashMap<SessionId, u64>>>,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
        on_done: Option<Arc<dyn Fn(&SessionId, Option<TokenUsage>) + Send + Sync>>,
    ) -> Result<(), String> {
        let route = |reason: String| format!("agent error: {reason}");
        ctx.session
            .append(&session_id, ChatMessage::user(user_input))
            .map_err(|e| route(e.to_string()))?;
        let history = ctx
            .session
            .get(&session_id)
            .map_err(|e| route(e.to_string()))?;
        let provider = ctx
            .providers
            .get_default()
            .ok_or_else(|| route("no default provider".into()))?;
        let tool_defs = ctx.tools.definitions();
        let mut messages = pipeline.assemble(&system_prompt, &history, &[]);
        let mut all_tool_calls: Vec<ToolCall> = vec![];
        let model = ctx
            .config
            .get_str("model.default_model")
            .unwrap_or_else(|| "deepseek-chat".into());
        let default_options = ModelOptions {
            model,
            ..Default::default()
        };
        let mut cancelled = false;
        let mut usage: Option<TokenUsage> = None;
        'outer: for _iter in 0..max_iter {
            let stream = provider
                .chat_stream(messages.clone(), tool_defs.clone(), default_options.clone())
                .await
                .map_err(|e| route(e.to_string()))?;
            futures::pin_mut!(stream);
            let mut content = String::new();
            let mut tc_list: Vec<ToolCall> = vec![];
            loop {
                let next = tokio::select! {
                    r = stream.next() => r,
                    _ = &mut cancel_rx => { cancelled = true; None },
                };
                match next {
                    Some(result) => match result.map_err(|e| route(e.to_string()))? {
                        ModelEvent::Delta(text) => {
                            content.push_str(&text);
                            if reply.send(AgentEvent::Delta(text)).await.is_err() {
                                return Ok(());
                            }
                        }
                        ModelEvent::ToolCall(tc) => tc_list.push(tc),
                        ModelEvent::Usage(tu) => match &mut usage {
                            Some(acc) => acc.accumulate(tu),
                            None => usage = Some(tu),
                        },
                        ModelEvent::Done => break,
                    },
                    None => {
                        if cancelled {
                            break 'outer;
                        }
                        break;
                    }
                }
            }
            if cancelled {
                break 'outer;
            }
            if tc_list.is_empty() {
                ctx.session
                    .append(&session_id, ChatMessage::assistant(content.clone()))
                    .map_err(|e| route(e.to_string()))?;
                if reply
                    .send(AgentEvent::Done {
                        final_response: content,
                        tool_calls: all_tool_calls,
                        usage,
                    })
                    .await
                    .is_err()
                {
                    return Ok(());
                }
                if let Some(f) = &on_done {
                    f(&session_id, usage);
                }
                return Ok(());
            }
            for tc in tc_list {
                if reply.send(AgentEvent::ToolCall(tc.clone())).await.is_err() {
                    return Ok(());
                }
                all_tool_calls.push(tc.clone());
                let (tx, mut tool_rx) = tokio::sync::oneshot::channel();
                if sender
                    .send(KernelMessage::ToolCallRequest {
                        session_id: session_id.clone(),
                        tool_call: tc.clone(),
                        target: tool_executor_id.clone(),
                        reply: Arc::new(std::sync::Mutex::new(Some(tx))),
                    })
                    .is_err()
                {
                    if reply
                        .send(AgentEvent::Error("bus send failed".into()))
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                    return Ok(());
                }
                let (tool_result, artifacts) = tokio::select! {
                    r = &mut tool_rx => match r {
                        Ok(Ok(outcome)) => {
                            let ToolExecutionOutcome { result, artifacts } = outcome;
                            let text = result
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| result.to_string());
                            (text, artifacts)
                        }
                        Ok(Err(e)) => (format!("Error: {e}"), Vec::new()),
                        Err(_) => {
                            if reply
                                .send(AgentEvent::Error("tool executor dropped".into()))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                            return Ok(());
                        }
                    },
                    _ = &mut cancel_rx => { cancelled = true; break; },
                };
                if cancelled {
                    break;
                }
                if reply
                    .send(AgentEvent::ToolResult {
                        call_id: tc.id.clone(),
                        result: tool_result.clone(),
                    })
                    .await
                    .is_err()
                {
                    return Ok(());
                }
                if !Self::forward_artifacts(&reply, artifacts, &session_id, &todo_revisions).await {
                    return Ok(());
                }
                pipeline.reflux(&mut messages, &tc, tool_result);
            }
            if cancelled {
                break 'outer;
            }
        }
        if cancelled {
            if reply
                .send(AgentEvent::Error("cancelled".into()))
                .await
                .is_err()
            {
                return Ok(());
            }
        } else if reply
            .send(AgentEvent::Error("max tool iterations exceeded".into()))
            .await
            .is_err()
        {
            return Ok(());
        }
        Ok(())
    }

    async fn forward_artifacts(
        reply: &mpsc::Sender<AgentEvent>,
        artifacts: Vec<TurnArtifact>,
        session_id: &SessionId,
        revisions: &Mutex<HashMap<SessionId, u64>>,
    ) -> bool {
        for artifact in artifacts {
            let event = match artifact {
                TurnArtifact::Todo(snapshot) => AgentEvent::Todo {
                    revision: next_todo_revision(revisions, session_id),
                    snapshot,
                },
                TurnArtifact::FileChange(change) => AgentEvent::FileChange(change),
            };
            if reply.send(event).await.is_err() {
                return false;
            }
        }
        true
    }
}

fn next_todo_revision(revisions: &Mutex<HashMap<SessionId, u64>>, session_id: &SessionId) -> u64 {
    let mut map = revisions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = map.entry(session_id.clone()).or_insert(0);
    *next += 1;
    *next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{TodoSnapshot, TurnArtifact, todo_snapshot_from_args, write_change};
    use crate::error::{ModelError, ModuleError};
    use crate::module::{
        ConfigProvider, KernelModule, Module, ModuleMetadata, ModuleRegister, ProviderRegistry,
        SessionStore, ToolExecutionOutcome, ToolRegistry,
    };
    use crate::types::{ChatMessage, Role, ToolDefinition};
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct MockSession {
        history: StdMutex<Vec<ChatMessage>>,
    }
    impl SessionStore for MockSession {
        fn create(&self) -> Result<SessionId, ModuleError> {
            Ok("test-session".into())
        }
        fn append(&self, _sid: &SessionId, msg: ChatMessage) -> Result<(), ModuleError> {
            self.history.lock().unwrap().push(msg);
            Ok(())
        }
        fn get(&self, _sid: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(self.history.lock().unwrap().clone())
        }
        fn estimate_tokens(&self, _sid: &SessionId) -> Result<usize, ModuleError> {
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

    struct ToolSpy;
    impl Module for ToolSpy {
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
    impl KernelModule for ToolSpy {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg {
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
    impl KernelModule for BlockingSpy {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg {
                // Take the sender and leak it so tool_rx stays pending forever.
                if let Some(tx) = reply.lock().unwrap().take() {
                    std::mem::forget(tx);
                }
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

    fn spy_kernel_with<S: KernelModule + 'static>(
        spy: S,
    ) -> (crate::kernel::Kernel, crate::kernel::KernelSender) {
        let mut kernel = crate::kernel::Kernel::new();
        let mut register = ModuleRegister::new(
            Arc::new(ToolRegistry::new()),
            Arc::new(ProviderRegistry::new()),
        );
        let ctx = loop_test_ctx();
        kernel
            .register_module(Box::new(spy), &ctx, &mut register)
            .unwrap();
        let sender = kernel.sender();
        let _handle = kernel.start();
        (kernel, sender)
    }

    #[tokio::test]
    async fn turn_streams_deltas_tool_boundary_and_done_via_bus() {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        let hits = Arc::new(AtomicUsize::new(0));
        let mut runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let h = Arc::clone(&hits);
        runner.set_on_turn_complete(Arc::new(move |_sid, _usage| {
            h.fetch_add(1, Ordering::SeqCst);
        }));
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        let mut kinds = vec![];
        let mut text = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Delta(d) => {
                    text.push_str(&d);
                    kinds.push("delta".to_string());
                }
                AgentEvent::ToolCall(_) => kinds.push("tool_call".to_string()),
                AgentEvent::ToolResult { .. } => kinds.push("tool_result".to_string()),
                AgentEvent::Todo { .. } => kinds.push("todo".to_string()),
                AgentEvent::FileChange(_) => kinds.push("file_change".to_string()),
                AgentEvent::Done { .. } => {
                    kinds.push("done".to_string());
                    break;
                }
                AgentEvent::Error(e) => panic!("turn error: {e}"),
            }
        }
        assert_eq!(text, "Hello done");
        assert_eq!(
            kinds,
            vec!["delta", "tool_call", "tool_result", "delta", "done"]
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while hits.load(Ordering::SeqCst) < 1 && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    #[tokio::test]
    async fn cancel_signals_turn_to_stop_with_cancelled_error() {
        let (kernel, sender) = spy_kernel_with(BlockingSpy);
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        loop {
            match rx.recv().await.unwrap() {
                AgentEvent::ToolCall(_) => break,
                AgentEvent::Delta(_) => continue,
                AgentEvent::Error(e) => panic!("unexpected error: {e}"),
                _ => {}
            }
        }
        assert!(runner.cancel(&sid), "cancel must find the active turn");
        let mut saw_cancelled = false;
        while let Some(ev) = rx.recv().await {
            if let AgentEvent::Error(e) = ev {
                assert_eq!(e, "cancelled");
                saw_cancelled = true;
                break;
            }
        }
        assert!(saw_cancelled);
        // ensure no partial assistant message appended (only user message)
        let history = ctx.session.get(&sid).unwrap();
        assert!(
            !history.iter().any(|m| m.role == Role::Assistant),
            "cancelled turn must not append partial assistant message"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    #[tokio::test]
    async fn shutdown_cancels_all_active_turns() {
        let (kernel, sender) = spy_kernel_with(BlockingSpy);
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        while let Some(ev) = rx.recv().await {
            if let AgentEvent::ToolCall(_) = ev {
                break;
            }
        }
        runner.shutdown();
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
    }

    #[tokio::test]
    async fn reply_receiver_dropped_aborts_without_panic() {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        // provider that emits many deltas — if receiver dropped, turn must abort quickly
        struct ManyDeltas;
        #[async_trait]
        impl crate::module::ModelProvider for ManyDeltas {
            async fn chat_stream(
                &self,
                _m: Vec<ChatMessage>,
                _t: Vec<ToolDefinition>,
                _o: ModelOptions,
            ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError>
            {
                let events: Vec<Result<ModelEvent, ModelError>> = (0..20)
                    .map(|i| Ok(ModelEvent::Delta(format!("{i}"))))
                    .chain(std::iter::once(Ok(ModelEvent::Done)))
                    .collect();
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
        providers.register("mock".into(), Arc::new(ManyDeltas));
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        drop(rx);
        // give turn time to attempt sends and abort
        tokio::time::sleep(Duration::from_millis(100)).await;
        // if we get here without panic or busy-loop, test passes
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    struct UsageScriptedProvider {
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl crate::module::ModelProvider for UsageScriptedProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let events: Vec<Result<ModelEvent, ModelError>> = if n == 0 {
                vec![
                    Ok(ModelEvent::ToolCall(tc())),
                    Ok(ModelEvent::Usage(crate::types::TokenUsage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                    })),
                    Ok(ModelEvent::Done),
                ]
            } else {
                vec![
                    Ok(ModelEvent::Delta("ok".into())),
                    Ok(ModelEvent::Usage(crate::types::TokenUsage {
                        prompt_tokens: 90,
                        completion_tokens: 15,
                    })),
                    Ok(ModelEvent::Done),
                ]
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

    #[tokio::test]
    async fn turn_aggregates_usage_across_iterations_and_forwards_to_done_and_callback() {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(UsageScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
            }),
        );
        let mut runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let captured = Arc::new(StdMutex::new(None));
        let cap = captured.clone();
        runner.set_on_turn_complete(Arc::new(move |_sid, usage| {
            *cap.lock().unwrap() = usage;
        }));
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        let mut done_usage = None;
        while let Some(ev) = rx.recv().await {
            if let AgentEvent::Done { usage, .. } = ev {
                done_usage = usage;
                break;
            }
            if let AgentEvent::Error(e) = ev {
                panic!("unexpected error: {e}");
            }
        }
        assert_eq!(
            done_usage,
            Some(crate::types::TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20
            })
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while captured.lock().unwrap().is_none() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            *captured.lock().unwrap(),
            Some(crate::types::TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20
            })
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    fn todo_snapshot(content: &str) -> TodoSnapshot {
        todo_snapshot_from_args(&serde_json::json!({"items": [{"content": content}]}))
            .expect("valid todo args")
    }

    struct ArtifactSpy {
        artifacts: Vec<TurnArtifact>,
    }

    impl Module for ArtifactSpy {
        fn id(&self) -> ModuleId {
            "tool_spy".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "tool_spy".into(),
                name: "artifact_spy".into(),
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

    impl KernelModule for ArtifactSpy {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg {
                let reply = reply.clone();
                let mut guard = reply.lock().unwrap();
                if let Some(tx) = guard.take() {
                    let _ = tx.send(Ok(ToolExecutionOutcome {
                        result: serde_json::json!({"result": "ok"}),
                        artifacts: self.artifacts.clone(),
                    }));
                }
            }
            Ok(crate::module::KernelResponse::Handled)
        }
    }

    #[tokio::test]
    async fn turn_runner_emits_todo_with_incrementing_revision() {
        let first = todo_snapshot("first");
        let second = todo_snapshot("second");
        let (kernel, sender) = spy_kernel_with(ArtifactSpy {
            artifacts: vec![
                TurnArtifact::Todo(first.clone()),
                TurnArtifact::Todo(second.clone()),
            ],
        });
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        let mut kinds: Vec<String> = vec![];
        let mut todos: Vec<(u64, TodoSnapshot)> = vec![];
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Delta(_) => kinds.push("delta".into()),
                AgentEvent::ToolCall(_) => kinds.push("tool_call".into()),
                AgentEvent::ToolResult { .. } => kinds.push("tool_result".into()),
                AgentEvent::Todo { revision, snapshot } => {
                    kinds.push(format!("todo#{revision}"));
                    todos.push((revision, snapshot));
                }
                AgentEvent::FileChange(_) => kinds.push("file_change".into()),
                AgentEvent::Done { .. } => {
                    kinds.push("done".into());
                    break;
                }
                AgentEvent::Error(e) => panic!("turn error: {e}"),
            }
        }
        assert_eq!(
            kinds,
            vec![
                "delta",
                "tool_call",
                "tool_result",
                "todo#1",
                "todo#2",
                "delta",
                "done"
            ]
        );
        assert_eq!(todos, vec![(1, first), (2, second)]);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    #[tokio::test]
    async fn turn_runner_emits_file_change_after_tool_result() {
        let change = write_change("src/a.rs", None, "x\n", Some("c1"));
        let (kernel, sender) = spy_kernel_with(ArtifactSpy {
            artifacts: vec![TurnArtifact::FileChange(change.clone())],
        });
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: tc(),
            }),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, tx)
            .unwrap();
        let mut kinds: Vec<String> = vec![];
        let mut received = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Delta(_) => kinds.push("delta".into()),
                AgentEvent::ToolCall(_) => kinds.push("tool_call".into()),
                AgentEvent::ToolResult { .. } => kinds.push("tool_result".into()),
                AgentEvent::Todo { .. } => kinds.push("todo".into()),
                AgentEvent::FileChange(artifact) => {
                    kinds.push("file_change".into());
                    received = Some(artifact);
                }
                AgentEvent::Done { .. } => {
                    kinds.push("done".into());
                    break;
                }
                AgentEvent::Error(e) => panic!("turn error: {e}"),
            }
        }
        assert_eq!(
            kinds,
            vec![
                "delta",
                "tool_call",
                "tool_result",
                "file_change",
                "delta",
                "done"
            ]
        );
        assert_eq!(received, Some(change));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }
}
