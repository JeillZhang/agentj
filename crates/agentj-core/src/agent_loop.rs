//! Agent loop — orchestrates a single turn: chat stream, tool calls, and
//! history persistence.

use crate::error::{KernelError, ModuleError};
use crate::module::{ContextPipeline, KernelContext, ToolSource};
use crate::types::*;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Incremental event emitted during agent loop execution.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta(String),
    ToolCall(ToolCall),
    ToolResult {
        call_id: String,
        result: String,
    },
    Done {
        final_response: String,
        tool_calls: Vec<ToolCall>,
    },
    Error(String),
}

type OnTurnComplete = Arc<dyn Fn(&SessionId) + Send + Sync>;

#[derive(Clone)]
pub struct AgentLoop {
    ctx: KernelContext,
    pipeline: Arc<dyn ContextPipeline>,
    system_prompt: String,
    max_tool_iterations: usize,
    on_turn_complete: Option<OnTurnComplete>,
}

impl AgentLoop {
    pub fn new(
        ctx: KernelContext,
        pipeline: Arc<dyn ContextPipeline>,
        system_prompt: String,
    ) -> Self {
        Self {
            ctx,
            pipeline,
            system_prompt,
            max_tool_iterations: 10,
            on_turn_complete: None,
        }
    }

    /// Registers a callback invoked once per successfully completed turn
    /// (usage accounting hook; see agentj-stats UsageCollector).
    pub fn set_on_turn_complete(&mut self, f: Arc<dyn Fn(&SessionId) + Send + Sync>) {
        self.on_turn_complete = Some(f);
    }

    pub fn create_session(&self) -> Result<SessionId, ModuleError> {
        self.ctx.session.create()
    }

    /// Runs one turn and returns a stream of events for incremental output.
    pub fn run_turn_stream(
        &self,
        session_id: &SessionId,
        user_input: &str,
    ) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(256);
        let sid = session_id.clone();
        let input = user_input.to_string();
        let ctx = self.ctx.clone();
        let pipeline = Arc::clone(&self.pipeline);
        let system_prompt = self.system_prompt.clone();
        let max_iter = self.max_tool_iterations;
        let on_done = self.on_turn_complete.clone();
        tokio::spawn(async move {
            let mut completed = false;
            if let Err(e) = Self::run_inner(
                ctx,
                pipeline,
                sid.clone(),
                input,
                system_prompt,
                max_iter,
                tx.clone(),
            )
            .await
            {
                let _ = tx.send(AgentEvent::Error(e.to_string())).await;
            } else {
                completed = true;
            }
            if completed && let Some(f) = &on_done {
                f(&sid);
            }
        });
        rx
    }

    /// Convenience: run one turn and collect the final text (non-streaming callers).
    pub async fn run_turn(
        &self,
        session_id: &SessionId,
        user_input: &str,
    ) -> Result<String, KernelError> {
        let mut rx = self.run_turn_stream(session_id, user_input);
        let mut text = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(d) => text.push_str(&d),
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => {
                    return Err(KernelError::MessageRouteFailed {
                        msg: "agent".into(),
                        reason: e,
                    });
                }
                _ => {}
            }
        }
        Ok(text)
    }

    async fn run_inner(
        ctx: KernelContext,
        pipeline: Arc<dyn ContextPipeline>,
        session_id: SessionId,
        user_input: String,
        system_prompt: String,
        max_iter: usize,
        tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(), KernelError> {
        let route = |msg: &str, reason: String| KernelError::MessageRouteFailed {
            msg: msg.into(),
            reason,
        };

        ctx.session
            .append(&session_id, ChatMessage::user(user_input))
            .map_err(|e| route("append", e.to_string()))?;
        let history = ctx
            .session
            .get(&session_id)
            .map_err(|e| route("get", e.to_string()))?;
        let provider = ctx
            .providers
            .get_default()
            .ok_or_else(|| route("no provider", "no default provider".into()))?;
        let tool_defs = ctx.tools.definitions();

        let mut messages = pipeline.assemble(&system_prompt, &history, &[]);
        let mut all_tool_calls: Vec<ToolCall> = vec![];
        // design 4.3: model comes from config (default_model), not hardcoded default
        let model = ctx
            .config
            .get_str("model.default_model")
            .unwrap_or_else(|| "deepseek-chat".into());
        let default_options = ModelOptions {
            model,
            ..Default::default()
        };
        let workspace_root = std::env::current_dir().map_err(|e| route("cwd", e.to_string()))?;

        for _iter in 0..max_iter {
            let mut stream = provider
                .chat_stream(messages.clone(), tool_defs.clone(), default_options.clone())
                .await
                .map_err(|e| route("chat_stream", e.to_string()))?;
            let mut content = String::new();
            let mut tc_list: Vec<ToolCall> = vec![];
            while let Some(result) = stream.next().await {
                match result.map_err(|e| route("stream", e.to_string()))? {
                    ModelEvent::Delta(text) => {
                        content.push_str(&text);
                        let _ = tx.send(AgentEvent::Delta(text)).await;
                    }
                    ModelEvent::ToolCall(tc) => tc_list.push(tc),
                    ModelEvent::Done => break,
                }
            }

            if tc_list.is_empty() {
                ctx.session
                    .append(&session_id, ChatMessage::assistant(content.clone()))
                    .map_err(|e| route("append response", e.to_string()))?;
                let _ = tx
                    .send(AgentEvent::Done {
                        final_response: content,
                        tool_calls: all_tool_calls,
                    })
                    .await;
                return Ok(());
            }

            Self::execute_tool_calls(
                &ctx,
                pipeline.as_ref(),
                &mut messages,
                &tc_list,
                &mut all_tool_calls,
                &tx,
                &session_id,
                &workspace_root,
            )
            .await?;
        }
        let _ = tx
            .send(AgentEvent::Error("max tool iterations exceeded".into()))
            .await;
        Ok(())
    }

    // 8 params are mandated by the permission-system plan (session_id +
    // workspace_root are part of the execute_async routing contract).
    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_calls(
        ctx: &KernelContext,
        pipeline: &dyn ContextPipeline,
        messages: &mut Vec<ChatMessage>,
        tc_list: &[ToolCall],
        all_tool_calls: &mut Vec<ToolCall>,
        tx: &mpsc::Sender<AgentEvent>,
        session_id: &str,
        workspace_root: &std::path::Path,
    ) -> Result<(), KernelError> {
        for tc in tc_list {
            let _ = tx.send(AgentEvent::ToolCall(tc.clone())).await;
            all_tool_calls.push(tc.clone());
            let source = ctx.tools.source_of(&tc.name).unwrap_or(ToolSource::Builtin);
            let perm_ctx = crate::permission::PermissionContext {
                session_id: session_id.to_string(),
                tool: tc.name.clone(),
                source,
                args: tc.arguments.clone(),
                workspace_root: workspace_root.to_path_buf(),
            };
            let result = ctx
                .tools
                .execute_async(&tc.name, tc.arguments.clone(), &perm_ctx)
                .await;
            let result_str = match result {
                Ok(val) => val
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| val.to_string()),
                Err(e) => format!("Error: {e}"),
            };
            let _ = tx
                .send(AgentEvent::ToolResult {
                    call_id: tc.id.clone(),
                    result: result_str.clone(),
                })
                .await;
            pipeline.reflux(messages, tc, result_str);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::*;
    use crate::module::*;
    use crate::permission::{
        PermissionChecker, PermissionContext, PermissionDecision, PermissionRequestId,
    };
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
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
        ) -> Result<ChatResponse, ModelError> {
            Ok(ChatResponse {
                content: "mock".into(),
                tool_calls: vec![],
            })
        }
    }

    // design 4.3: records the model string it receives so tests can assert the
    // config-driven default_model reached the provider.
    struct RecordingProvider {
        models: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait]
    impl ModelProvider for RecordingProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            _t: Vec<ToolDefinition>,
            o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            self.models.lock().unwrap().push(o.model.clone());
            Ok(Box::pin(stream::iter(vec![Ok(ModelEvent::Done)])))
        }
        async fn chat(
            &self,
            _m: Vec<ChatMessage>,
            o: ModelOptions,
        ) -> Result<ChatResponse, ModelError> {
            self.models.lock().unwrap().push(o.model.clone());
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    struct MockSession {
        history: Mutex<Vec<ChatMessage>>,
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
        fn get_str(&self, _key: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _key: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    // design 4.3: config that serves a custom model.default_model.
    struct ModelConfig(&'static str);
    impl ConfigProvider for ModelConfig {
        fn get_str(&self, key: &str) -> Option<String> {
            if key == "model.default_model" {
                Some(self.0.to_string())
            } else {
                None
            }
        }
        fn get_int(&self, _key: &str) -> Option<i64> {
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

    #[tokio::test]
    async fn on_turn_complete_fires_once_per_completed_turn() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let session = Arc::new(MockSession {
            history: Mutex::new(vec![]),
        });
        let config: Arc<dyn ConfigProvider> = Arc::new(MockConfig);
        let mock = Arc::new(MockProvider {
            chunks: vec!["Hello".into()],
        });
        providers.register("mock".into(), mock);
        let ctx = KernelContext {
            session,
            tools,
            providers,
            config,
        };
        let mut agent_loop = AgentLoop::new(ctx, Arc::new(MockPipeline), "sys".into());
        let sid = agent_loop.create_session().unwrap();

        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        agent_loop.set_on_turn_complete(Arc::new(move |_sid| {
            h.fetch_add(1, Ordering::SeqCst);
        }));

        agent_loop.run_turn(&sid, "hello").await.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while hits.load(Ordering::SeqCst) < 1 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        agent_loop.run_turn(&sid, "again").await.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while hits.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_turn_stream_yields_deltas() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let session = Arc::new(MockSession {
            history: Mutex::new(vec![]),
        });
        let config: Arc<dyn ConfigProvider> = Arc::new(MockConfig);
        let mock = Arc::new(MockProvider {
            chunks: vec!["Hello".into(), " world".into()],
        });
        providers.register("mock".into(), mock);

        let ctx = KernelContext {
            session,
            tools,
            providers,
            config,
        };
        let agent_loop = AgentLoop::new(
            ctx,
            Arc::new(MockPipeline),
            "You are a helpful assistant.".into(),
        );
        let sid = agent_loop.create_session().unwrap();

        let mut rx = agent_loop.run_turn_stream(&sid, "hello");
        let mut collected = vec![];
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(text) => collected.push(text),
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("agent error: {e}"),
                _ => {}
            }
        }
        assert_eq!(collected.join(""), "Hello world");
    }

    #[tokio::test]
    async fn run_turn_uses_config_default_model() {
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        let models = Arc::new(Mutex::new(Vec::new()));
        providers.register(
            "mock".into(),
            Arc::new(RecordingProvider {
                models: models.clone(),
            }),
        );
        let session = Arc::new(MockSession {
            history: Mutex::new(vec![]),
        });
        let config: Arc<dyn ConfigProvider> = Arc::new(ModelConfig("opencode-go/kimi-k3"));

        let ctx = KernelContext {
            session,
            tools,
            providers,
            config,
        };
        let agent_loop = AgentLoop::new(
            ctx,
            Arc::new(MockPipeline),
            "You are a helpful assistant.".into(),
        );
        let sid = agent_loop.create_session().unwrap();

        let mut rx = agent_loop.run_turn_stream(&sid, "hello");
        while let Some(event) = rx.recv().await {
            if let AgentEvent::Done { .. } = event {
                break;
            }
        }
        let recorded = models.lock().unwrap().clone();
        assert_eq!(recorded, vec!["opencode-go/kimi-k3".to_string()]);
    }

    // Minimal tool for the loop tests (agent_loop.rs tests cannot see module.rs's
    // private DummyTool — define a local one).
    struct LoopTool;
    impl crate::module::Tool for LoopTool {
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
            _input: serde_json::Value,
        ) -> Result<serde_json::Value, crate::error::ToolError> {
            Ok(serde_json::Value::String("ok".into()))
        }
    }

    struct AllowAll;
    #[async_trait]
    impl PermissionChecker for AllowAll {
        fn check(&self, _ctx: &PermissionContext) -> PermissionDecision {
            PermissionDecision::Allow
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    fn loop_test_ctx(tools: ToolRegistry) -> KernelContext {
        KernelContext {
            session: Arc::new(MockSession {
                history: Mutex::new(vec![]),
            }),
            tools: Arc::new(tools),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig), // local mock — v0.1 T11 pattern
        }
    }

    #[tokio::test]
    async fn tool_loop_goes_through_execute_async() {
        let tools = ToolRegistry::new();
        tools.set_permission_checker(Arc::new(AllowAll));
        tools.register(Arc::new(LoopTool));
        let ctx = loop_test_ctx(tools);
        let pipeline = MockPipeline;
        let mut messages = vec![];
        let mut all_tool_calls = vec![];
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let tc = ToolCall {
            id: "c1".into(),
            name: "dummy".into(),
            arguments: serde_json::json!({}),
        };
        AgentLoop::execute_tool_calls(
            &ctx,
            &pipeline,
            &mut messages,
            &[tc],
            &mut all_tool_calls,
            &tx,
            "session-1", // real session id — never ""
            std::path::Path::new("/ws"),
        )
        .await
        .unwrap();
        assert!(matches!(rx.recv().await.unwrap(), AgentEvent::ToolCall(_)));
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, AgentEvent::ToolResult { call_id, .. } if call_id == "c1"));
    }

    #[tokio::test]
    async fn tool_loop_denied_returns_denied_result() {
        struct DenyAll;
        #[async_trait]
        impl PermissionChecker for DenyAll {
            fn check(&self, _ctx: &PermissionContext) -> PermissionDecision {
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
        let tools = ToolRegistry::new();
        tools.set_permission_checker(Arc::new(DenyAll));
        tools.register(Arc::new(LoopTool));
        let ctx = loop_test_ctx(tools);
        let pipeline = MockPipeline;
        let mut messages = vec![];
        let mut all_tool_calls = vec![];
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let tc = ToolCall {
            id: "c1".into(),
            name: "dummy".into(),
            arguments: serde_json::json!({}),
        };
        AgentLoop::execute_tool_calls(
            &ctx,
            &pipeline,
            &mut messages,
            &[tc],
            &mut all_tool_calls,
            &tx,
            "session-1",
            std::path::Path::new("/ws"),
        )
        .await
        .unwrap();
        assert!(matches!(rx.recv().await.unwrap(), AgentEvent::ToolCall(_)));
        let event = rx.recv().await.unwrap();
        match event {
            AgentEvent::ToolResult { call_id, result } => {
                assert_eq!(call_id, "c1");
                assert!(
                    result.contains("permission denied") || result.contains("denied"),
                    "got: {result}"
                );
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
