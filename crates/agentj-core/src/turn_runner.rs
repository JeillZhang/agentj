use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::artifacts::TurnArtifact;
use crate::error::ModuleError;
use crate::kernel::KernelSender;
use crate::module::{ContextPipeline, KernelContext, KernelMessage, ToolExecutionOutcome};
use crate::types::{
    AgentEvent, ChatMessage, ModelEvent, ModelOptions, ModuleId, RunOverrides, SessionId,
    TokenUsage, ToolCall,
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
    todo_revisions: Arc<Mutex<HashMap<String, u64>>>,
    file_change_seqs: Arc<Mutex<HashMap<String, u64>>>,
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
            file_change_seqs: Arc::new(Mutex::new(HashMap::new())),
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
        overrides: RunOverrides,
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
        let file_change_seqs = Arc::clone(&self.file_change_seqs);
        let run_id = uuid::Uuid::new_v4().to_string();
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
                run_id.clone(),
                content,
                system_prompt,
                overrides,
                max_iter,
                reply.clone(),
                sender,
                tool_executor_id,
                todo_revisions,
                file_change_seqs,
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
        run_id: String,
        user_input: String,
        system_prompt: String,
        overrides: RunOverrides,
        max_iter: usize,
        reply: mpsc::Sender<AgentEvent>,
        sender: KernelSender,
        tool_executor_id: ModuleId,
        todo_revisions: Arc<Mutex<HashMap<String, u64>>>,
        file_change_seqs: Arc<Mutex<HashMap<String, u64>>>,
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
        let provider = match overrides
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            Some(name) => ctx.providers.get(name).ok_or_else(|| {
                route(format!(
                    "unknown provider: {name} (registered: {})",
                    ctx.providers.list().join(", ")
                ))
            })?,
            None => ctx
                .providers
                .get_default()
                .ok_or_else(|| route("no default provider".into()))?,
        };
        let tool_defs = ctx.tools.definitions();
        let mut messages = pipeline.assemble(&system_prompt, &history, &[]);
        let mut all_tool_calls: Vec<ToolCall> = vec![];
        let model = overrides
            .model
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                ctx.config
                    .get_str("model.default_model")
                    .unwrap_or_else(|| "deepseek-chat".into())
            });
        let default_options = ModelOptions {
            model,
            session_id: Some(session_id.clone()),
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
                        run_id: Some(run_id.clone()),
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
                if !Self::forward_artifacts(
                    &reply,
                    artifacts,
                    &run_id,
                    &todo_revisions,
                    &file_change_seqs,
                )
                .await
                {
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
        run_id: &str,
        revisions: &Mutex<HashMap<String, u64>>,
        seqs: &Mutex<HashMap<String, u64>>,
    ) -> bool {
        for artifact in artifacts {
            let event = match artifact {
                TurnArtifact::Todo(snapshot) => AgentEvent::Todo {
                    run_id: Some(run_id.to_string()),
                    revision: next_todo_revision(revisions, run_id),
                    snapshot,
                },
                TurnArtifact::FileChange(mut change) => {
                    change.run_id = Some(run_id.to_string());
                    change.seq = Some(next_file_change_seq(seqs, run_id));
                    AgentEvent::FileChange(change)
                }
            };
            if reply.send(event).await.is_err() {
                return false;
            }
        }
        true
    }
}

fn next_todo_revision(revisions: &Mutex<HashMap<String, u64>>, run_id: &str) -> u64 {
    let mut map = revisions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = map.entry(run_id.to_string()).or_insert(0);
    *next += 1;
    *next
}

fn next_file_change_seq(seqs: &Mutex<HashMap<String, u64>>, run_id: &str) -> u64 {
    let mut map = seqs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let next = map.entry(run_id.to_string()).or_insert(0);
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
            let events: Vec<Result<ModelEvent, ModelError>> = if n.is_multiple_of(2) {
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        let mut kinds: Vec<String> = vec![];
        let mut todos: Vec<(u64, TodoSnapshot)> = vec![];
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Delta(_) => kinds.push("delta".into()),
                AgentEvent::ToolCall(_) => kinds.push("tool_call".into()),
                AgentEvent::ToolResult { .. } => kinds.push("tool_result".into()),
                AgentEvent::Todo {
                    revision, snapshot, ..
                } => {
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
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
        let received = received.expect("file_change must be received");
        // legacy fields must remain valid (no regression)
        assert_eq!(received.path, change.path);
        assert_eq!(received.op, change.op);
        assert_eq!(received.additions, change.additions);
        assert_eq!(received.deletions, change.deletions);
        assert_eq!(received.tool_call_id, change.tool_call_id);
        assert_eq!(received.diff, change.diff);
        assert_eq!(received.diff_truncated, change.diff_truncated);
        // new per-run fields must be populated
        assert!(received.run_id.is_some(), "run_id must be Some");
        assert!(uuid::Uuid::parse_str(received.run_id.as_ref().unwrap()).is_ok());
        assert_eq!(received.seq, Some(1));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    fn is_uuid_v4(s: &str) -> bool {
        uuid::Uuid::parse_str(s).is_ok()
    }

    async fn collect_done_run_id(
        mut rx: tokio::sync::mpsc::Receiver<AgentEvent>,
    ) -> Option<String> {
        while let Some(ev) = rx.recv().await {
            if let AgentEvent::Done { run_id, .. } = ev {
                return run_id;
            }
            if let AgentEvent::Error(e) = ev {
                panic!("unexpected error: {e}");
            }
        }
        None
    }

    #[tokio::test]
    async fn run_id_unique_per_turn() {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        let providers = ctx.providers.clone();
        struct ImmediateDone;
        #[async_trait]
        impl crate::module::ModelProvider for ImmediateDone {
            async fn chat_stream(
                &self,
                _m: Vec<ChatMessage>,
                _t: Vec<ToolDefinition>,
                _o: ModelOptions,
            ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError>
            {
                Ok(Box::pin(stream::iter(vec![Ok(ModelEvent::Done)])))
            }
            async fn chat(
                &self,
                _m: Vec<ChatMessage>,
                _o: ModelOptions,
            ) -> Result<crate::types::ChatResponse, ModelError> {
                Ok(crate::types::ChatResponse {
                    content: "ok".into(),
                    tool_calls: vec![],
                })
            }
        }
        providers.register("mock".into(), Arc::new(ImmediateDone));
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx1, rx1) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(
                sid.clone(),
                "hi1".into(),
                None,
                RunOverrides::default(),
                tx1,
            )
            .unwrap();
        let id1 = collect_done_run_id(rx1).await.expect("done 1");
        assert!(is_uuid_v4(&id1));
        let (tx2, rx2) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(
                sid.clone(),
                "hi2".into(),
                None,
                RunOverrides::default(),
                tx2,
            )
            .unwrap();
        let id2 = collect_done_run_id(rx2).await.expect("done 2");
        assert!(is_uuid_v4(&id2));
        assert_ne!(id1, id2);
        // revision reset per run is covered in todo_revision_per_run
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    #[tokio::test]
    async fn todo_revision_per_run() {
        let snap_a = todo_snapshot("a");
        let snap_b = todo_snapshot("b");
        // First run emits two todos — revisions 1,2 within same run
        let (kernel, sender) = spy_kernel_with(ArtifactSpy {
            artifacts: vec![
                TurnArtifact::Todo(snap_a.clone()),
                TurnArtifact::Todo(snap_b.clone()),
            ],
        });
        let ctx = loop_test_ctx();
        ctx.providers.register(
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        let mut revs = vec![];
        let mut run_ids = vec![];
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Todo {
                    revision, run_id, ..
                } => {
                    revs.push(revision);
                    run_ids.push(run_id);
                }
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert_eq!(revs, vec![1, 2]);
        assert!(run_ids.iter().all(|id| id.is_some()));
        assert_eq!(run_ids[0], run_ids[1], "same run must share run_id");
        let first_run_id = run_ids[0].clone().unwrap();
        // Second run on same runner/session must reset revision to 1 and have distinct run_id
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(
                sid.clone(),
                "hi2".into(),
                None,
                RunOverrides::default(),
                tx2,
            )
            .unwrap();
        let mut revs2 = vec![];
        let mut run_ids2 = vec![];
        while let Some(ev) = rx2.recv().await {
            match ev {
                AgentEvent::Todo {
                    revision, run_id, ..
                } => {
                    revs2.push(revision);
                    run_ids2.push(run_id);
                }
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert!(!revs2.is_empty());
        assert_eq!(revs2[0], 1, "revision must reset to 1 on next run");
        assert_ne!(run_ids2[0].as_deref(), Some(first_run_id.as_str()));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    #[tokio::test]
    async fn file_change_seq_per_run() {
        let c1 = write_change("a.txt", None, "x", Some("c1"));
        let c2 = write_change("b.txt", None, "y", Some("c1"));
        let c3 = write_change("a.txt", None, "z", Some("c1"));
        // Three file changes inside one run — seq 1,2,3
        // ArtifactSpy will return all three for the single tool call
        let (kernel, sender) = spy_kernel_with(ArtifactSpy {
            artifacts: vec![
                TurnArtifact::FileChange(c1),
                TurnArtifact::FileChange(c2),
                TurnArtifact::FileChange(c3),
            ],
        });
        let ctx = loop_test_ctx();
        ctx.providers.register(
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
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        let mut seqs = vec![];
        let mut run_ids = vec![];
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::FileChange(fc) => {
                    seqs.push(fc.seq);
                    run_ids.push(fc.run_id.clone());
                }
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert_eq!(seqs, vec![Some(1), Some(2), Some(3)]);
        assert!(run_ids.iter().all(|id| id.is_some()));
        assert!(run_ids.iter().all(|id| id == &run_ids[0]));
        // Next run must reset seq to 1
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(
                sid.clone(),
                "hi2".into(),
                None,
                RunOverrides::default(),
                tx2,
            )
            .unwrap();
        let mut seqs2 = vec![];
        while let Some(ev) = rx2.recv().await {
            match ev {
                AgentEvent::FileChange(fc) => seqs2.push(fc.seq),
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        if !seqs2.is_empty() {
            assert_eq!(seqs2[0], Some(1), "seq must reset to 1 on next run");
        }
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    struct RecordingProvider(Arc<Mutex<Option<ModelOptions>>>);

    #[async_trait]
    impl crate::module::ModelProvider for RecordingProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            options: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            *self.0.lock().unwrap() = Some(options);
            Ok(Box::pin(stream::iter(vec![
                Ok(ModelEvent::Delta("ok".into())),
                Ok(ModelEvent::Done),
            ])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<crate::types::ChatResponse, ModelError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn run_turn_injects_session_id_into_model_options() {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        let seen: Arc<Mutex<Option<ModelOptions>>> = Arc::new(Mutex::new(None));
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        ctx.providers
            .register("mock".into(), Arc::new(RecordingProvider(seen.clone())));
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("turn error: {e}"),
                _ => {}
            }
        }
        let seen = seen
            .lock()
            .unwrap()
            .clone()
            .expect("chat_stream must be called");
        assert_eq!(seen.session_id.as_deref(), Some(sid.as_str()));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    /// Config double that answers only `model.default_model` (MockConfig returns None for all).
    struct FixedConfig(&'static str);
    impl ConfigProvider for FixedConfig {
        fn get_str(&self, key: &str) -> Option<String> {
            (key == "model.default_model").then(|| self.0.to_string())
        }
        fn get_int(&self, _key: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    /// Records `"<name>:<model>"` for every model call it serves.
    struct NamedRecordingProvider {
        name: &'static str,
        seen: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl crate::module::ModelProvider for NamedRecordingProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            options: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            self.seen
                .lock()
                .unwrap()
                .push(format!("{}:{}", self.name, options.model));
            Ok(Box::pin(stream::iter(vec![
                Ok(ModelEvent::Delta("ok".into())),
                Ok(ModelEvent::Done),
            ])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            _o: ModelOptions,
        ) -> Result<crate::types::ChatResponse, ModelError> {
            unimplemented!()
        }
    }

    /// One turn with two providers registered: "mock" first (thus the default) and "alt".
    async fn run_with(
        overrides: RunOverrides,
        default_model: Option<&'static str>,
    ) -> (Vec<String>, Option<String>) {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let mut ctx = loop_test_ctx();
        if let Some(dm) = default_model {
            ctx.config = Arc::new(FixedConfig(dm));
        }
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        ctx.providers.register(
            "mock".into(),
            Arc::new(NamedRecordingProvider {
                name: "mock",
                seen: seen.clone(),
            }),
        );
        ctx.providers.register(
            "alt".into(),
            Arc::new(NamedRecordingProvider {
                name: "alt",
                seen: seen.clone(),
            }),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid, "hi".into(), None, overrides, tx)
            .unwrap();
        let mut error = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => {
                    error = Some(e);
                    break;
                }
                _ => {}
            }
        }
        let out = seen.lock().unwrap().clone();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        (out, error)
    }

    #[tokio::test]
    async fn run_turn_applies_model_override() {
        let (seen, err) = run_with(
            RunOverrides {
                provider: None,
                model: Some("custom-model".into()),
            },
            None,
        )
        .await;
        assert!(err.is_none(), "{err:?}");
        assert_eq!(seen, vec!["mock:custom-model".to_string()]);
    }

    #[tokio::test]
    async fn run_turn_routes_to_explicit_non_default_provider() {
        let (seen, err) = run_with(
            RunOverrides {
                provider: Some("alt".into()),
                model: Some("m1".into()),
            },
            None,
        )
        .await;
        assert!(err.is_none(), "{err:?}");
        // the default provider "mock" must not be called
        assert_eq!(seen, vec!["alt:m1".to_string()]);
    }

    #[tokio::test]
    async fn run_turn_provider_only_keeps_config_model() {
        let (seen, err) = run_with(
            RunOverrides {
                provider: Some("alt".into()),
                model: None,
            },
            Some("cfg-model"),
        )
        .await;
        assert!(err.is_none(), "{err:?}");
        assert_eq!(seen, vec!["alt:cfg-model".to_string()]);
    }

    #[tokio::test]
    async fn run_turn_unknown_provider_fails_without_fallback() {
        let (seen, err) = run_with(
            RunOverrides {
                provider: Some("nope".into()),
                model: None,
            },
            None,
        )
        .await;
        let err = err.expect("unknown provider must fail the turn");
        assert!(err.contains("unknown provider: nope"), "{err}");
        assert!(err.contains("alt") && err.contains("mock"), "{err}");
        assert!(seen.is_empty(), "no provider may serve the turn");
    }

    #[tokio::test]
    async fn run_turn_without_overrides_uses_config_default() {
        let (seen, err) = run_with(RunOverrides::default(), Some("cfg-model")).await;
        assert!(err.is_none(), "{err:?}");
        assert_eq!(seen, vec!["mock:cfg-model".to_string()]);
    }

    #[tokio::test]
    async fn run_turn_blank_overrides_are_absent() {
        let (seen, err) = run_with(
            RunOverrides {
                provider: Some("  ".into()),
                model: Some("".into()),
            },
            Some("cfg-model"),
        )
        .await;
        assert!(err.is_none(), "{err:?}");
        assert_eq!(seen, vec!["mock:cfg-model".to_string()]);
    }
}
