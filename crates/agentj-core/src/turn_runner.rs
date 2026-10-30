use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::sync::mpsc;

use crate::artifacts::TurnArtifact;
use crate::budget::{self, MAX_OVERFLOW_RETRIES};
use crate::error::{ModelError, ModuleError, ToolError};
use crate::kernel::KernelSender;
use crate::module::{
    ContextPipeline, KernelContext, KernelMessage, ModelProvider, ToolExecutionOutcome,
};
use crate::sync::lock_or_recover;
use crate::text_tool_call;
use crate::types::{
    AgentEvent, ChatMessage, ModelEvent, ModelOptions, ModuleId, OnTurnComplete, Role,
    RunOverrides, SessionId, TokenUsage, ToolCall, ToolDefinition,
};
use tokio::sync::oneshot::error as oneshot_error;

/// Retries allowed when a model round never produced a usable conclusion — the stream
/// ended before `Done`, or the model answered with nothing. @since 0.29.8
const MAX_INCOMPLETE_RETRIES: usize = 2;

/// 工具轮次用尽后的收尾指令：不再提供任何工具，只要求模型基于已获得的信息作答。
/// 没有它，用户看到的是模型最后一句「让我再看看…」加一条内部错误。@since 0.29.9
const BUDGET_EXHAUSTED_NUDGE: &str =
    "工具调用轮次已用尽。请立刻基于你已经获得的信息给出最终结论，不要再请求任何工具。";

/// 纠偏指令：模型把工具调用写成了纯文本（`<rfl:parameter …>` 之类）而没真的调用工具。
/// 这类文本既不能当结论交给用户，也要告诉模型改走工具通道。@since 0.29.10
const TOOL_CALL_AS_TEXT_NUDGE: &str = "你上一条回复把工具调用写成了纯文本标记（例如 <rfl:parameter>），并没有真的调用工具。\
     请改用工具调用通道重新发起；若不再需要工具，就直接给出最终结论。";

/// Id for a tool call recovered from text (the gateway never issued one). Shaped like the
/// gateway's own `call-…` ids so the upstream sees a familiar field. @since 0.29.10
fn recovered_call_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("call-text-{nanos}")
}

pub struct TurnHandle {
    cancel_tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// How the model phase continues after one round.
enum RoundFlow {
    Next,
    Finished,
    Cancelled,
}

/// Accumulated output of one model stream.
struct Collected {
    content: String,
    tool_calls: Vec<ToolCall>,
    saw_done: bool,
}

/// How one model stream ended.
enum StreamEnd {
    Collected(Collected),
    Cancelled,
    ClientGone,
}

/// How one round of tool calls ended.
enum ToolRound {
    Completed(Vec<(ToolCall, String)>),
    Cancelled,
    ClientGone,
}

/// Why a tool round had to stop early.
enum ToolAbort {
    Cancelled,
    ClientGone,
}

/// The per-round and cross-round lane of one turn: request context, round counters, and
/// the tool/usage accumulators. Grouped so the helpers below stay inside the function
/// length budget without changing any state the turn keeps.
#[derive(Default)]
struct TurnLane {
    tool_defs: Vec<ToolDefinition>,
    options: ModelOptions,
    messages: Vec<ChatMessage>,
    all_tool_calls: Vec<ToolCall>,
    usage: Option<TokenUsage>,
    context_budget: usize,
    cancelled: bool,
    overflow_retries: usize,
    incomplete_retries: usize,
}

/// One turn's state, so the round helpers can take `&mut self` instead of threading a
/// dozen locals through every call.
struct TurnState {
    ctx: KernelContext,
    pipeline: Arc<dyn ContextPipeline>,
    session_id: SessionId,
    run_id: String,
    reply: mpsc::Sender<AgentEvent>,
    sender: KernelSender,
    tool_executor_id: ModuleId,
    todo_revisions: Arc<Mutex<HashMap<String, u64>>>,
    file_change_seqs: Arc<Mutex<HashMap<String, u64>>>,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
    on_done: Option<OnTurnComplete>,
    provider_name: String,
    provider: Arc<dyn ModelProvider>,
    max_iter: usize,
    max_tool_result_chars: usize,
    lane: TurnLane,
}

#[allow(clippy::type_complexity)]
#[derive(Clone)]
pub struct TurnRunner {
    ctx: KernelContext,
    pipeline: Arc<dyn ContextPipeline>,
    default_system_prompt: String,
    max_tool_iterations: usize,
    max_tool_result_chars: usize,
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
        let max_tool_result_chars = ctx
            .config
            .get_int("agent.max_tool_result_chars")
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(100_000);
        Self {
            ctx,
            pipeline,
            default_system_prompt,
            max_tool_iterations,
            max_tool_result_chars,
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
                && let Some(tx) = lock_or_recover(&handle.cancel_tx).take()
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
                if let Some(tx) = lock_or_recover(&handle.cancel_tx).take() {
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
        self.register_turn(&session_id, cancel_tx)?;
        let turns = Arc::clone(&self.turns);
        let turn_reply = reply.clone();
        let turn = self.turn_future(
            session_id.clone(),
            content,
            system_prompt,
            overrides,
            turn_reply,
            cancel_rx,
        );
        tokio::spawn(async move { Self::spawn_guarded(session_id, reply, turns, turn).await });
        Ok(())
    }

    /// Remember the turn slot before the task starts: the handle owns the cancel sender.
    fn register_turn(
        &self,
        session_id: &SessionId,
        cancel_tx: tokio::sync::oneshot::Sender<()>,
    ) -> Result<(), ModuleError> {
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
        Ok(())
    }

    /// The owned turn future: every piece the turn needs is cloned here so the task can
    /// outlive `&self` and the panic guard has one value to wrap.
    fn turn_future(
        &self,
        session_id: SessionId,
        content: String,
        system_prompt: Option<String>,
        overrides: RunOverrides,
        reply: mpsc::Sender<AgentEvent>,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let ctx = self.ctx.clone();
        let pipeline = Arc::clone(&self.pipeline);
        let system_prompt = system_prompt.unwrap_or_else(|| self.default_system_prompt.clone());
        let max_iter = self.max_tool_iterations;
        let max_tool_result_chars = self.max_tool_result_chars;
        let on_done = self.on_turn_complete.clone();
        let sender = self.sender.clone();
        let tool_executor_id = self.tool_executor_id.clone();
        let todo_revisions = Arc::clone(&self.todo_revisions);
        let file_change_seqs = Arc::clone(&self.file_change_seqs);
        let run_id = uuid::Uuid::new_v4().to_string();
        async move {
            Self::run_turn(
                ctx,
                pipeline,
                session_id,
                run_id,
                content,
                system_prompt,
                overrides,
                max_iter,
                max_tool_result_chars,
                reply,
                sender,
                tool_executor_id,
                todo_revisions,
                file_change_seqs,
                cancel_rx,
                on_done,
            )
            .await
        }
    }

    /// Await the turn behind a panic guard, then always deliver its terminal error event
    /// and release the session's turn slot.
    ///
    /// A panic anywhere in the turn (provider adapter, pipeline, tool plumbing) must still
    /// terminate the client's stream and release the slot — otherwise the client waits
    /// forever with no terminal event.
    async fn spawn_guarded<F>(
        session_id: SessionId,
        reply: mpsc::Sender<AgentEvent>,
        turns: Arc<Mutex<HashMap<SessionId, TurnHandle>>>,
        future: F,
    ) where
        F: Future<Output = Result<(), String>>,
    {
        let result =
            match futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(future)).await {
                Ok(result) => result,
                Err(payload) => Err(Self::on_panic(payload.as_ref())),
            };
        if let Err(e) = &result {
            let _ = reply.send(AgentEvent::Error(e.clone())).await;
        }
        if let Ok(mut map) = turns.lock() {
            map.remove(&session_id);
        }
    }

    /// Render a panicked turn as the error string the client sees (unchanged text).
    fn on_panic(payload: &(dyn std::any::Any + Send)) -> String {
        let reason = crate::sync::panic_payload_message(payload);
        tracing::error!(%reason, "turn panicked");
        format!("turn panicked: {reason}")
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
        max_tool_result_chars: usize,
        reply: mpsc::Sender<AgentEvent>,
        sender: KernelSender,
        tool_executor_id: ModuleId,
        todo_revisions: Arc<Mutex<HashMap<String, u64>>>,
        file_change_seqs: Arc<Mutex<HashMap<String, u64>>>,
        cancel_rx: tokio::sync::oneshot::Receiver<()>,
        on_done: Option<OnTurnComplete>,
    ) -> Result<(), String> {
        let (provider_name, provider) = TurnState::resolve_provider(&ctx, &overrides)?;
        let mut turn = TurnState {
            ctx,
            pipeline,
            session_id,
            run_id,
            reply,
            sender,
            tool_executor_id,
            todo_revisions,
            file_change_seqs,
            cancel_rx,
            on_done,
            provider_name,
            provider,
            max_iter,
            max_tool_result_chars,
            lane: TurnLane::default(),
        };
        turn.prepare_turn(user_input, &overrides, system_prompt)
            .await?;
        if turn.run_tool_rounds().await? {
            return Ok(());
        }
        turn.finish_turn().await
    }

    /// 工具轮次用尽后的收尾一趟：不提供任何工具，把流里的文本收集起来。返回 `None`
    /// 表示本轮被取消（或客户端已断开），调用方不再结算。@since 0.29.9
    #[allow(clippy::too_many_arguments)]
    async fn conclude_without_tools(
        provider: &dyn crate::module::ModelProvider,
        pipeline: &Arc<dyn ContextPipeline>,
        messages: Vec<ChatMessage>,
        options: &ModelOptions,
        context_budget: usize,
        reply: &mpsc::Sender<AgentEvent>,
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
        usage: &mut Option<TokenUsage>,
    ) -> Result<Option<String>, ModelError> {
        let request = pipeline.fit(messages, context_budget);
        let stream = provider
            .chat_stream(request, Vec::new(), options.clone())
            .await?;
        futures::pin_mut!(stream);
        let mut text = String::new();
        loop {
            let next = tokio::select! {
                item = stream.next() => item,
                _ = &mut *cancel_rx => return Ok(None),
            };
            match next {
                Some(Ok(ModelEvent::Delta(delta))) => {
                    text.push_str(&delta);
                    if reply.send(AgentEvent::Delta(delta)).await.is_err() {
                        return Ok(None);
                    }
                }
                Some(Ok(ModelEvent::Usage(tu))) => match usage {
                    Some(acc) => acc.accumulate(tu),
                    None => *usage = Some(tu),
                },
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e),
                None => return Ok(Some(text)),
            }
        }
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

impl TurnState {
    /// Explicit `provider` override wins over the kernel default; an unknown name is
    /// reported with the registered list instead of silently falling back.
    fn resolve_provider(
        ctx: &KernelContext,
        overrides: &RunOverrides,
    ) -> Result<(String, Arc<dyn ModelProvider>), String> {
        let route = |reason: String| format!("agent error: {reason}");
        match overrides
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            Some(name) => {
                let provider = ctx.providers.get(name).ok_or_else(|| {
                    route(format!(
                        "unknown provider: {name} (registered: {})",
                        ctx.providers.list().join(", ")
                    ))
                })?;
                Ok((name.to_string(), provider))
            }
            None => ctx
                .providers
                .get_default_with_name()
                .ok_or_else(|| route("no default provider".into())),
        }
    }

    /// Model id for this turn: explicit override > configured default > `deepseek-chat`.
    fn resolve_model(ctx: &KernelContext, overrides: &RunOverrides) -> String {
        overrides
            .model
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                ctx.config
                    .get_str("model.default_model")
                    .unwrap_or_else(|| "deepseek-chat".into())
            })
    }

    /// Append the user message, assemble the request context and set the token budget.
    async fn prepare_turn(
        &mut self,
        user_input: String,
        overrides: &RunOverrides,
        system_prompt: String,
    ) -> Result<(), String> {
        let route = |reason: String| format!("agent error: {reason}");
        self.ctx
            .session
            .append(&self.session_id, ChatMessage::user(user_input))
            .map_err(|e| route(e.to_string()))?;
        let history = self
            .ctx
            .session
            .get(&self.session_id)
            .map_err(|e| route(e.to_string()))?;
        self.lane.tool_defs = self.ctx.tools.definitions();
        self.lane.messages = self.pipeline.assemble(&system_prompt, &history, &[]);
        self.lane.options = ModelOptions {
            model: Self::resolve_model(&self.ctx, overrides),
            session_id: Some(self.session_id.clone()),
            ..Default::default()
        };
        // 上下文预算（design §上下文预算）：发送前把装配好的请求裁到预算内，上游报
        // context 超限时收紧预算重试，而不是把 400 直接抛给用户。预算按本次请求的模型
        // 重新计算——显式上限与模型窗口取较小者，窗口只信 90%。@since 0.29.7
        let cap = self
            .ctx
            .config
            .get_int("model.max_context_tokens")
            .and_then(|budget| usize::try_from(budget).ok());
        let window = self.ctx.providers.context_window(&self.lane.options.model);
        self.lane.context_budget = budget::effective_budget(cap, window);
        Ok(())
    }

    /// Drain up to `max_iter` rounds. `Ok(true)` means the turn already produced its
    /// terminal event inside a round (final answer, or the client went away).
    async fn run_tool_rounds(&mut self) -> Result<bool, String> {
        for _iter in 0..self.max_iter {
            match self.run_round().await? {
                RoundFlow::Next => {}
                RoundFlow::Finished => return Ok(true),
                RoundFlow::Cancelled => return Ok(false),
            }
        }
        Ok(false)
    }

    /// One model round: open the stream, drain it, run the guards, then either answer or
    /// execute the round's tool calls.
    async fn run_round(&mut self) -> Result<RoundFlow, String> {
        let Some(stream) = self.open_stream().await? else {
            return Ok(RoundFlow::Next); // budget shrank: replay the round
        };
        let mut collected = match self.collect_stream(stream).await? {
            StreamEnd::Collected(collected) => collected,
            StreamEnd::Cancelled => return Ok(RoundFlow::Cancelled),
            StreamEnd::ClientGone => return Ok(RoundFlow::Finished),
        };
        let retry = self.guard_round(
            &mut collected.content,
            &mut collected.tool_calls,
            collected.saw_done,
        )?;
        if retry {
            return Ok(RoundFlow::Next);
        }
        if collected.tool_calls.is_empty() {
            return self.emit_final_answer(collected.content).await;
        }
        match self.execute_tool_calls(collected.tool_calls).await {
            ToolRound::Cancelled => Ok(RoundFlow::Cancelled),
            ToolRound::ClientGone => Ok(RoundFlow::Finished),
            ToolRound::Completed(round) => self.commit_tool_round(collected.content, round).await,
        }
    }

    /// Send the next request and open its stream. `Ok(None)` means upstream reported an
    /// overflow and the budget was tightened, so the round has to be replayed.
    async fn open_stream(
        &mut self,
    ) -> Result<Option<BoxStream<'static, Result<ModelEvent, ModelError>>>, String> {
        let route = |reason: String| format!("agent error: {reason}");
        // The definition set may not claim more than 30% of the budget (design ㊶): the tail is
        // dropped so the body keeps room, and the provider is sent exactly what was charged.
        let tools = budget::bounded_definitions(&self.lane.tool_defs, self.lane.context_budget);
        let request = self.pipeline.fit_with_tools(
            self.lane.messages.clone(),
            &tools,
            self.lane.context_budget,
        );
        let options = self.lane.options.clone();
        match self.provider.chat_stream(request, tools, options).await {
            Ok(stream) => Ok(Some(stream)),
            Err(err @ ModelError::TokenLimitExceeded { max, .. }) => {
                // 上游报超限：收紧预算重发（预算严格递减；无法再减时如实上抛）。
                match budget::retry_budget(self.lane.context_budget, max) {
                    Some(next) if self.lane.overflow_retries < MAX_OVERFLOW_RETRIES => {
                        self.lane.overflow_retries += 1;
                        self.lane.context_budget = next;
                        Ok(None)
                    }
                    _ => Err(route(err.to_string())),
                }
            }
            Err(e) => Err(route(e.to_string())),
        }
    }

    /// Drain one model stream, forwarding deltas to the client as they arrive.
    async fn collect_stream(
        &mut self,
        stream: BoxStream<'static, Result<ModelEvent, ModelError>>,
    ) -> Result<StreamEnd, String> {
        let route = |reason: String| format!("agent error: {reason}");
        futures::pin_mut!(stream);
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = vec![];
        let mut saw_done = false;
        loop {
            let next = tokio::select! {
                r = stream.next() => r,
                _ = &mut self.cancel_rx => { self.lane.cancelled = true; None },
            };
            match next {
                Some(result) => match result.map_err(|e| route(e.to_string()))? {
                    ModelEvent::Delta(text) => {
                        content.push_str(&text);
                        if self.reply.send(AgentEvent::Delta(text)).await.is_err() {
                            return Ok(StreamEnd::ClientGone);
                        }
                    }
                    ModelEvent::ToolCall(tc) => tool_calls.push(tc),
                    ModelEvent::Usage(tu) => accumulate_usage(&mut self.lane.usage, tu),
                    ModelEvent::Done => {
                        saw_done = true;
                        break;
                    }
                },
                None => break,
            }
        }
        if self.lane.cancelled {
            return Ok(StreamEnd::Cancelled);
        }
        Ok(StreamEnd::Collected(Collected {
            content,
            tool_calls,
            saw_done,
        }))
    }

    /// 落库/执行前的三道守卫（逐条照搬拆分前的判定顺序）。`Ok(true)` = 本轮要重发。
    fn guard_round(
        &mut self,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        saw_done: bool,
    ) -> Result<bool, String> {
        if !saw_done && tool_calls.is_empty() && !content.trim().is_empty() {
            return self.guard_truncated_stream(content);
        }
        if tool_calls.is_empty() {
            self.recover_text_tool_call(content, tool_calls);
        }
        if tool_calls.is_empty() && text_tool_call::looks_like_tool_call_text(content) {
            return self.guard_unparsable_markup(content);
        }
        Ok(false)
    }

    /// 纯文本回复却没以 Done 收尾 = 上游/连接中途截断：半截句子不能被当成「最终结论」
    /// 落库（用户只看到一句残句，「继续」也接不上）。先重发，仍不行就如实报错。带工具
    /// 调用的轮次不受影响：openai-compat 只在收到 finish_reason=tool_calls 后才吐工具
    /// 调用，那本身就是上游已完成的信号。@since 0.29.8
    fn guard_truncated_stream(&mut self, content: &str) -> Result<bool, String> {
        let route = |reason: String| format!("agent error: {reason}");
        if self.lane.incomplete_retries < MAX_INCOMPLETE_RETRIES {
            self.lane.incomplete_retries += 1;
            if text_tool_call::looks_like_tool_call_text(content) {
                self.push_nudge();
            }
            return Ok(true);
        }
        Err(route("model stream ended before completion".into()))
    }

    /// 有些模型把工具调用写成**纯文本方言**
    /// （`analysis <|constrain|>xml<|message|><rfl:parameter name="command">ls -la</rfl:parameter>`，
    /// 实测 opencode-go 部分模型），网关没把它翻译成结构化 tool_calls。当成「最终
    /// 结论」收尾就会让整轮任务停在半路，所以按文本里写明的参数还原成真正的工具调用，
    /// 继续正常执行（权限门与落库都走原有路径）。@since 0.29.10
    fn recover_text_tool_call(&mut self, content: &mut String, tool_calls: &mut Vec<ToolCall>) {
        let Some(call) = text_tool_call::recover_tool_call(content) else {
            return;
        };
        tracing::warn!(
            tool = %call.tool,
            chars = content.len(),
            "model wrote its tool call as text; recovered it and executing the tool"
        );
        tool_calls.push(ToolCall {
            id: recovered_call_id(),
            name: call.tool,
            arguments: call.arguments,
        });
        content.clear();
    }

    /// 认得出是文本工具调用、但参数无法无歧义还原：同样不能当结论。重发一次并要求模型改走
    /// 工具通道，仍不改就如实报错（而不是把标记当答复交给用户）。@since 0.29.10
    fn guard_unparsable_markup(&mut self, content: &str) -> Result<bool, String> {
        let route = |reason: String| format!("agent error: {reason}");
        if self.lane.incomplete_retries < MAX_INCOMPLETE_RETRIES {
            self.lane.incomplete_retries += 1;
            tracing::warn!(
                chars = content.len(),
                "model wrote unparsable tool-call markup; replaying with a corrective note"
            );
            self.push_nudge();
            return Ok(true);
        }
        Err(route(
            "model kept writing its tool call as text instead of calling the tool".into(),
        ))
    }

    fn push_nudge(&mut self) {
        self.lane
            .messages
            .push(ChatMessage::user(TOOL_CALL_AS_TEXT_NUDGE.to_string()));
    }

    /// No tool calls: this round's text IS the answer. Persist it, emit `Done`, notify.
    async fn emit_final_answer(&mut self, content: String) -> Result<RoundFlow, String> {
        let route = |reason: String| format!("agent error: {reason}");
        self.ctx
            .session
            .append(&self.session_id, ChatMessage::assistant(content.clone()))
            .map_err(|e| route(e.to_string()))?;
        if self.emit_done(content).await.is_err() {
            return Ok(RoundFlow::Finished);
        }
        self.notify_done();
        Ok(RoundFlow::Finished)
    }

    /// `AgentEvent::Done` carrying the run id and the provider/model that served it.
    async fn emit_done(&mut self, final_response: String) -> Result<(), ()> {
        let event = AgentEvent::Done {
            run_id: Some(self.run_id.clone()),
            final_response,
            tool_calls: std::mem::take(&mut self.lane.all_tool_calls),
            usage: self.lane.usage,
            provider: Some(self.provider_name.clone()),
            model: Some(self.lane.options.model.clone()),
        };
        self.reply.send(event).await.map_err(|_| ())
    }

    fn notify_done(&self) {
        if let Some(f) = &self.on_done {
            f(&self.session_id, self.lane.usage);
        }
    }

    /// Run every tool call of this round in order and collect their results.
    async fn execute_tool_calls(&mut self, tool_calls: Vec<ToolCall>) -> ToolRound {
        let mut round: Vec<(ToolCall, String)> = Vec::with_capacity(tool_calls.len());
        for tc in tool_calls {
            if let Err(abort) = self.run_one_tool(tc, &mut round).await {
                return match abort {
                    ToolAbort::Cancelled => ToolRound::Cancelled,
                    ToolAbort::ClientGone => ToolRound::ClientGone,
                };
            }
        }
        ToolRound::Completed(round)
    }

    async fn run_one_tool(
        &mut self,
        tc: ToolCall,
        round: &mut Vec<(ToolCall, String)>,
    ) -> Result<(), ToolAbort> {
        let outcome = self.dispatch_tool(&tc).await?;
        self.finish_tool(&tc, outcome, round).await
    }

    /// Ask the tool-executor module to run one call and wait for its outcome.
    async fn dispatch_tool(
        &mut self,
        tc: &ToolCall,
    ) -> Result<(String, Vec<TurnArtifact>), ToolAbort> {
        if self
            .reply
            .send(AgentEvent::ToolCall(tc.clone()))
            .await
            .is_err()
        {
            return Err(ToolAbort::ClientGone);
        }
        self.lane.all_tool_calls.push(tc.clone());
        let mut tool_rx = self.send_tool_request(tc).await?;
        let picked = tokio::select! {
            r = &mut tool_rx => Some(r),
            _ = &mut self.cancel_rx => None,
        };
        self.tool_outcome(picked).await
    }

    /// Hand one call to the tool-executor module; no receiver means the client is gone.
    async fn send_tool_request(
        &mut self,
        tc: &ToolCall,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<ToolExecutionOutcome, ToolError>>, ToolAbort>
    {
        let (tx, tool_rx) = tokio::sync::oneshot::channel();
        let request = KernelMessage::ToolCallRequest {
            session_id: self.session_id.clone(),
            tool_call: tc.clone(),
            target: self.tool_executor_id.clone(),
            reply: Arc::new(std::sync::Mutex::new(Some(tx))),
        };
        if self.sender.send(request).is_err() {
            let _ = self
                .reply
                .send(AgentEvent::Error("bus send failed".into()))
                .await;
            return Err(ToolAbort::ClientGone);
        }
        Ok(tool_rx)
    }

    /// Normalize whatever the executor reported into the text the model sees next round.
    async fn tool_outcome(
        &mut self,
        picked: Option<Result<Result<ToolExecutionOutcome, ToolError>, oneshot_error::RecvError>>,
    ) -> Result<(String, Vec<TurnArtifact>), ToolAbort> {
        match picked {
            Some(Ok(Ok(ToolExecutionOutcome { result, artifacts }))) => {
                let text = result
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| result.to_string());
                Ok((text, artifacts))
            }
            Some(Ok(Err(e))) => Ok((format!("Error: {e}"), Vec::new())),
            Some(Err(_)) => {
                let _ = self
                    .reply
                    .send(AgentEvent::Error("tool executor dropped".into()))
                    .await;
                Err(ToolAbort::ClientGone)
            }
            None => {
                self.lane.cancelled = true;
                Err(ToolAbort::Cancelled)
            }
        }
    }

    /// Emit the tool result (truncated exactly once, at this single conversion point),
    /// forward its artifacts, and fold the result back into the request context.
    async fn finish_tool(
        &mut self,
        tc: &ToolCall,
        outcome: (String, Vec<TurnArtifact>),
        round: &mut Vec<(ToolCall, String)>,
    ) -> Result<(), ToolAbort> {
        let (tool_result, artifacts) = outcome;
        let (tool_result, dropped) =
            crate::budget::truncate_chars(&tool_result, self.max_tool_result_chars);
        if !self.send_tool_result(tc, &tool_result, dropped).await {
            return Err(ToolAbort::ClientGone);
        }
        let forwarded = TurnRunner::forward_artifacts(
            &self.reply,
            artifacts,
            &self.run_id,
            &self.todo_revisions,
            &self.file_change_seqs,
        )
        .await;
        if !forwarded {
            return Err(ToolAbort::ClientGone);
        }
        self.pipeline
            .reflux(&mut self.lane.messages, tc, tool_result.clone());
        round.push((tc.clone(), tool_result));
        Ok(())
    }

    /// Broadcast the (already truncated) result of one call; `false` = client gone.
    async fn send_tool_result(&mut self, tc: &ToolCall, result: &str, dropped: usize) -> bool {
        if dropped > 0 {
            let truncated = AgentEvent::ToolResultTruncated {
                tool_call_id: tc.id.clone(),
                dropped_chars: dropped,
            };
            if self.reply.send(truncated).await.is_err() {
                return false;
            }
        }
        let event = AgentEvent::ToolResult {
            call_id: tc.id.clone(),
            result: result.to_string(),
        };
        self.reply.send(event).await.is_ok()
    }

    /// 工具往返必须落库：若只有最终一句话留在快照里，下一轮（尤其用户「继续」）模型
    /// 看不到任何工具调用与结果，等于从零重来。落库延到整轮结束后：取消或中途失败时
    /// 整轮不写，绝不会留下没有结果的悬空调用。@since 0.29.8
    async fn commit_tool_round(
        &mut self,
        content: String,
        round: Vec<(ToolCall, String)>,
    ) -> Result<RoundFlow, String> {
        let route = |reason: String| format!("agent error: {reason}");
        let assistant = ChatMessage {
            role: Role::Assistant,
            content,
            tool_call_id: None,
            tool_calls: round.iter().map(|(tc, _)| tc.clone()).collect(),
        };
        self.ctx
            .session
            .append(&self.session_id, assistant)
            .map_err(|e| route(e.to_string()))?;
        for (tc, tool_result) in round {
            self.ctx
                .session
                .append(
                    &self.session_id,
                    ChatMessage::tool_result(tc.id, tool_result),
                )
                .map_err(|e| route(e.to_string()))?;
        }
        Ok(RoundFlow::Next)
    }

    /// Cancel-aware epilogue: the tool-free conclusion pass, then persist + emit `Done`.
    async fn finish_turn(&mut self) -> Result<(), String> {
        let route = |reason: String| format!("agent error: {reason}");
        let Some(conclusion) = self.conclude_turn().await? else {
            let _ = self.reply.send(AgentEvent::Error("cancelled".into())).await;
            return Ok(());
        };
        // 工具轮次用尽：此刻模型最后一句往往还是「让我再看看…」，直接报内部错误会把整轮
        // 成果丢掉，用户也接不下去。收尾也拿不到内容才如实报错。@since 0.29.9
        if conclusion.trim().is_empty() {
            return Err(route(format!(
                "max tool iterations exceeded ({}) and the model returned no conclusion",
                self.max_iter
            )));
        }
        self.ctx
            .session
            .append(&self.session_id, ChatMessage::assistant(conclusion.clone()))
            .map_err(|e| route(e.to_string()))?;
        if self.emit_done(conclusion).await.is_err() {
            return Ok(());
        }
        self.notify_done();
        Ok(())
    }

    /// 工具轮次用尽后的收尾一趟：不再提供任何工具，只允许模型基于已获得的信息作答。
    /// `Ok(None)` 表示本轮被取消（或客户端已断开），调用方不再结算。@since 0.29.9
    async fn conclude_turn(&mut self) -> Result<Option<String>, String> {
        if self.lane.cancelled {
            return Ok(None);
        }
        let route = |reason: String| format!("agent error: {reason}");
        self.lane
            .messages
            .push(ChatMessage::user(BUDGET_EXHAUSTED_NUDGE.to_string()));
        let messages = std::mem::take(&mut self.lane.messages);
        TurnRunner::conclude_without_tools(
            self.provider.as_ref(),
            &self.pipeline,
            messages,
            &self.lane.options,
            self.lane.context_budget,
            &self.reply,
            &mut self.cancel_rx,
            &mut self.lane.usage,
        )
        .await
        .map_err(|e| route(e.to_string()))
    }
}

/// Fold one usage event into the turn's running total.
fn accumulate_usage(usage: &mut Option<TokenUsage>, incoming: TokenUsage) {
    match usage {
        Some(acc) => acc.accumulate(incoming),
        None => *usage = Some(incoming),
    }
}

fn next_todo_revision(revisions: &Mutex<HashMap<String, u64>>, run_id: &str) -> u64 {
    let mut map = crate::sync::lock_or_recover(revisions);
    let next = map.entry(run_id.to_string()).or_insert(0);
    *next += 1;
    *next
}

fn next_file_change_seq(seqs: &Mutex<HashMap<String, u64>>, run_id: &str) -> u64 {
    let mut map = lock_or_recover(seqs);
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
            lock_or_recover(&self.history).push(msg);
            Ok(())
        }
        fn get(&self, _sid: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
            Ok(lock_or_recover(&self.history).clone())
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

    /// Config that only answers the context-budget key.
    struct BudgetConfig(i64);
    impl ConfigProvider for BudgetConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, k: &str) -> Option<i64> {
            (k == "model.max_context_tokens").then_some(self.0)
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    /// Context whose session already holds `history`, with `config` answering the budget key.
    fn budget_test_ctx_with(
        history: Vec<ChatMessage>,
        config: Arc<dyn ConfigProvider>,
    ) -> KernelContext {
        KernelContext {
            session: Arc::new(MockSession {
                history: StdMutex::new(history),
            }),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config,
        }
    }

    /// Context whose session already holds `history` and whose budget is `budget` tokens.
    fn budget_test_ctx(history: Vec<ChatMessage>, budget: usize) -> KernelContext {
        budget_test_ctx_with(history, Arc::new(BudgetConfig(budget as i64)))
    }

    /// Records the estimated token size of every request it is asked to serve. Fails with
    /// `overflow` when set — on every call when `always_overflow` is set, else only on the
    /// first — mimicking an upstream context-window rejection. `window` is the context
    /// window it publishes for every model (`None` = unknown, the legacy behaviour).
    struct CapturingProvider {
        seen: Arc<StdMutex<Vec<usize>>>,
        /// Estimated size of the tool definitions each request carried.
        seen_defs: Arc<StdMutex<Vec<usize>>>,
        overflow: Option<ModelError>,
        always_overflow: bool,
        calls: Arc<AtomicUsize>,
        window: Option<usize>,
    }

    #[async_trait]
    impl crate::module::ModelProvider for CapturingProvider {
        fn context_window(&self, _model: &str) -> Option<usize> {
            self.window
        }

        async fn chat_stream(
            &self,
            m: Vec<ChatMessage>,
            t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            crate::sync::lock_or_recover(&self.seen).push(crate::budget::request_tokens(&m));
            crate::sync::lock_or_recover(&self.seen_defs)
                .push(crate::budget::definitions_tokens(&t));
            if let Some(err) = &self.overflow
                && (self.always_overflow || call == 0)
            {
                return Err(match err {
                    ModelError::TokenLimitExceeded { used, max } => {
                        ModelError::TokenLimitExceeded {
                            used: *used,
                            max: *max,
                        }
                    }
                    other => ModelError::Api {
                        status: 400,
                        message: other.to_string(),
                    },
                });
            }
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
            Ok(crate::types::ChatResponse {
                content: String::new(),
                tool_calls: vec![],
            })
        }
    }

    /// Drive one turn to completion and return what the provider saw.
    async fn run_capturing_turn(
        history: Vec<ChatMessage>,
        budget: usize,
        overflow: Option<ModelError>,
    ) -> Vec<usize> {
        run_capturing_turn_with(history, budget, overflow, false).await
    }

    /// As [`run_capturing_turn`], but `always_overflow` keeps rejecting every call so a turn
    /// can run out of retries.
    async fn run_capturing_turn_with(
        history: Vec<ChatMessage>,
        budget: usize,
        overflow: Option<ModelError>,
        always_overflow: bool,
    ) -> Vec<usize> {
        let ctx = budget_test_ctx(history, budget);
        run_capturing_turn_in(ctx, overflow, always_overflow, None).await
    }

    /// Drive one turn over `ctx` with a provider publishing `window`, and return the
    /// estimated token size of every request it saw.
    async fn run_capturing_turn_in(
        ctx: KernelContext,
        overflow: Option<ModelError>,
        always_overflow: bool,
        window: Option<usize>,
    ) -> Vec<usize> {
        run_capturing_turn_in_recording_defs(ctx, overflow, always_overflow, window)
            .await
            .0
    }

    /// As [`run_capturing_turn_in`], but also reports the estimated size of the tool
    /// definitions each request carried: `(messages_tokens, definitions_tokens)`.
    async fn run_capturing_turn_in_recording_defs(
        ctx: KernelContext,
        overflow: Option<ModelError>,
        always_overflow: bool,
        window: Option<usize>,
    ) -> (Vec<usize>, Vec<usize>) {
        let (_kernel, sender) = spy_kernel_with(ToolSpy);
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(MockPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen_defs = Arc::new(StdMutex::new(Vec::new()));
        ctx.providers.register(
            "mock".into(),
            Arc::new(CapturingProvider {
                seen: Arc::clone(&seen),
                seen_defs: Arc::clone(&seen_defs),
                overflow,
                always_overflow,
                calls: Arc::new(AtomicUsize::new(0)),
                window,
            }),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid, "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        while let Some(ev) = rx.recv().await {
            if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
                break;
            }
        }
        lock_or_recover(&seen)
            .iter()
            .copied()
            .zip(lock_or_recover(&seen_defs).iter().copied())
            .unzip()
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
                let mut guard = lock_or_recover(&reply);
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
                if let Some(tx) = lock_or_recover(reply).take() {
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
                AgentEvent::ToolResultTruncated { .. } => {}
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

    /// Serves one scripted event list per call; the last script repeats. Records how many
    /// tools each call was offered, so the tool-free conclusion pass is observable.
    struct ScriptProvider {
        calls: Arc<AtomicUsize>,
        scripts: Vec<Vec<ModelEvent>>,
        tools_seen: Arc<StdMutex<Vec<usize>>>,
    }

    #[async_trait]
    impl crate::module::ModelProvider for ScriptProvider {
        async fn chat_stream(
            &self,
            _m: Vec<ChatMessage>,
            t: Vec<ToolDefinition>,
            _o: ModelOptions,
        ) -> Result<BoxStream<'static, Result<ModelEvent, ModelError>>, ModelError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            lock_or_recover(&self.tools_seen).push(t.len());
            let index = n.min(self.scripts.len().saturating_sub(1));
            let events = self.scripts[index].clone();
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
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

    /// Drive one turn against `scripts`, returning every emitted event, the session history
    /// left behind and the provider call count.
    async fn run_scripted_turn(
        scripts: Vec<Vec<ModelEvent>>,
    ) -> (Vec<AgentEvent>, Vec<ChatMessage>, Arc<AtomicUsize>) {
        let (events, history, calls, _tools) =
            run_scripted_turn_with(Arc::new(MockConfig), scripts).await;
        (events, history, calls)
    }

    /// Config answering the tool-iteration budget key.
    struct TurnLimitConfig(i64);
    impl ConfigProvider for TurnLimitConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, k: &str) -> Option<i64> {
            (k == "agent.max_tool_iterations").then_some(self.0)
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    /// A round that asks for one tool and terminates properly.
    fn tool_round() -> Vec<ModelEvent> {
        vec![ModelEvent::ToolCall(tc()), ModelEvent::Done]
    }

    /// Drive one turn against `scripts` with a small tool-iteration budget, also returning the
    /// per-call tool-definition counts.
    async fn run_budgeted_scripted_turn(
        max_iter: i64,
        scripts: Vec<Vec<ModelEvent>>,
    ) -> (
        Vec<AgentEvent>,
        Vec<ChatMessage>,
        Arc<AtomicUsize>,
        Arc<StdMutex<Vec<usize>>>,
    ) {
        run_scripted_turn_with(Arc::new(TurnLimitConfig(max_iter)), scripts).await
    }

    #[allow(clippy::type_complexity)]
    async fn run_scripted_turn_with(
        config: Arc<dyn ConfigProvider>,
        scripts: Vec<Vec<ModelEvent>>,
    ) -> (
        Vec<AgentEvent>,
        Vec<ChatMessage>,
        Arc<AtomicUsize>,
        Arc<StdMutex<Vec<usize>>>,
    ) {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let session = Arc::new(MockSession {
            history: StdMutex::new(vec![]),
        });
        let ctx = KernelContext {
            session: session.clone(),
            tools: Arc::new(ToolRegistry::new()),
            providers: Arc::new(ProviderRegistry::new()),
            config,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let tools_seen = Arc::new(StdMutex::new(vec![]));
        ctx.providers.register(
            "mock".into(),
            Arc::new(ScriptProvider {
                calls: Arc::clone(&calls),
                scripts,
                tools_seen: Arc::clone(&tools_seen),
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
            .spawn_turn(sid, "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        let mut events = vec![];
        while let Some(ev) = rx.recv().await {
            let terminal = matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_));
            events.push(ev);
            if terminal {
                break;
            }
        }
        let history = lock_or_recover(&session.history).clone();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        (events, history, calls, tools_seen)
    }

    /// Every tool round-trip is persisted: a follow-up turn (user “继续”) must still see the
    /// tool calls and their results, otherwise it starts from zero. @since 0.29.8
    #[tokio::test]
    async fn turn_persists_tool_round_trips_so_the_next_turn_has_context() {
        let (events, history, _calls) = run_scripted_turn(vec![
            vec![
                ModelEvent::Delta("let me look".into()),
                ModelEvent::ToolCall(tc()),
                ModelEvent::Done,
            ],
            vec![ModelEvent::Delta(" final".into()), ModelEvent::Done],
        ])
        .await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { final_response, .. }) if final_response == " final"),
            "unexpected terminal event: {:?}",
            events.last()
        );
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].content, "let me look");
        assert_eq!(history[1].tool_calls.len(), 1);
        assert_eq!(history[1].tool_calls[0].id, "c1");
        assert_eq!(history[2].role, Role::Tool);
        assert_eq!(history[2].tool_call_id.as_deref(), Some("c1"));
        assert!(history[2].content.contains("ok"), "{}", history[2].content);
        assert_eq!(history[3].content, " final");
    }

    /// A stream that ends without `Done` is truncated output, not a conclusion: the round is
    /// replayed and only the complete reply reaches the session and the Done event. @since 0.29.8
    #[tokio::test]
    async fn turn_replays_a_round_whose_stream_never_finished() {
        let (events, history, calls) = run_scripted_turn(vec![
            vec![ModelEvent::Delta("passed total: 922".into())],
            vec![ModelEvent::Delta("full analysis".into()), ModelEvent::Done],
        ])
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { final_response, .. }) if final_response == "full analysis"),
            "unexpected terminal event: {:?}",
            events.last()
        );
        assert_eq!(
            history.len(),
            2,
            "only the user input and the final reply persist"
        );
        assert_eq!(history[1].content, "full analysis");
    }

    /// Retries are bounded: a provider that never completes a round ends in an error instead
    /// of a bogus Done. @since 0.29.8
    #[tokio::test]
    async fn turn_fails_when_no_round_ever_completes() {
        let (events, history, calls) =
            run_scripted_turn(vec![vec![ModelEvent::Delta("half".into())]]).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1 + MAX_INCOMPLETE_RETRIES);
        match events.last() {
            Some(AgentEvent::Error(message)) => {
                assert!(message.contains("ended before completion"), "{message}")
            }
            other => panic!("expected an error, got {other:?}"),
        }
        assert_eq!(history.len(), 1, "no assistant message may be persisted");
    }

    /// A round that terminates properly with no text is not a truncation: the stream
    /// closed itself, so it must not be retried (only a missing `Done` counts).
    #[tokio::test]
    async fn turn_accepts_an_empty_but_terminated_round() {
        let (events, history, calls) = run_scripted_turn(vec![vec![ModelEvent::Done]]).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no retry for a terminated round"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { .. })),
            "the turn ends normally, got {:?}",
            events.last()
        );
        assert_eq!(
            history.len(),
            2,
            "the empty assistant reply is still persisted"
        );
    }

    /// 模型把工具调用写成了纯文本方言（实测 leak）：它是一次真正的工具调用，不是最终结论。
    /// 还原后正常执行并继续，用户不会再拿到一段 `<rfl:parameter>` 标记当答复。@since 0.29.10
    #[tokio::test]
    async fn turn_recovers_a_tool_call_the_model_wrote_as_text() {
        let leak = "analysis <|constrain|>xml<|message|><rfl:parameter name=\"command\">\
                    ls docs/changelog</rfl:parameter>";
        let (events, history, calls) = run_scripted_turn(vec![
            vec![ModelEvent::Delta(leak.into()), ModelEvent::Done],
            vec![ModelEvent::Delta("最终结论".into()), ModelEvent::Done],
        ])
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the leaked round must not be treated as the answer"
        );
        assert!(
            matches!(
                events.last(),
                Some(AgentEvent::Done { final_response, tool_calls, .. })
                    if final_response == "最终结论" && tool_calls.len() == 1
            ),
            "unexpected terminal event: {:?}",
            events.last()
        );
        assert_eq!(history.len(), 4);
        assert_eq!(
            history[1].tool_calls.len(),
            1,
            "the recovered call is persisted"
        );
        assert_eq!(history[1].tool_calls[0].name, "bash");
        assert_eq!(
            history[1].tool_calls[0].arguments["command"],
            "ls docs/changelog"
        );
        assert!(history[1].tool_calls[0].id.starts_with("call-text-"));
        assert_eq!(
            history[1].content, "",
            "markup must not become the model's words"
        );
        assert_eq!(history[3].content, "最终结论");
    }

    /// 认得出是文本工具调用却无法还原（只剩半截标记）时，既不能当结论，也不能假装成功：
    /// 重发并纠偏，仍不改就如实报错，且不落任何助手消息。@since 0.29.10
    #[tokio::test]
    async fn turn_refuses_to_present_tool_call_markup_as_an_answer() {
        let fragment = "passed total: 922\n</rfl:parameter>";
        let (events, history, calls) = run_scripted_turn(vec![vec![
            ModelEvent::Delta(fragment.into()),
            ModelEvent::Done,
        ]])
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1 + MAX_INCOMPLETE_RETRIES);
        assert!(
            matches!(events.last(), Some(AgentEvent::Error(e)) if e.contains("as text")),
            "unexpected terminal event: {:?}",
            events.last()
        );
        assert_eq!(history.len(), 1, "no assistant message may be persisted");
    }

    /// 工具轮次用尽时不能把模型的最后一句「让我再看看…」当成结论，也不能只报一条内部
    /// 错误：要再走一趟**不带工具**的收尾，把已经查到的信息落成最终答复。@since 0.29.9
    #[tokio::test]
    async fn turn_concludes_with_a_tool_free_pass_when_the_budget_runs_out() {
        let (events, history, calls, tools_seen) = run_budgeted_scripted_turn(
            2,
            vec![
                tool_round(),
                tool_round(),
                vec![ModelEvent::Delta("最终结论".into()), ModelEvent::Done],
            ],
        )
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "two tool rounds plus one conclusion pass"
        );
        assert_eq!(
            lock_or_recover(&tools_seen).last().copied(),
            Some(0),
            "the conclusion pass must offer no tools: left as-is the model asks for more work"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::Done { final_response, .. }) if final_response == "最终结论"),
            "unexpected terminal event: {:?}",
            events.last()
        );
        assert_eq!(history.last().map(|m| m.content.as_str()), Some("最终结论"));
    }

    /// 收尾那一趟也拿不到结论时，如实报错——不能把「让我再看看…」回落成答复，
    /// 也不写入一条空助手消息。@since 0.29.9
    #[tokio::test]
    async fn turn_reports_an_error_when_the_conclusion_pass_is_empty() {
        let (events, history, calls, _tools) =
            run_budgeted_scripted_turn(1, vec![tool_round(), vec![ModelEvent::Done]]).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "one tool round + one pass");
        match events.last() {
            Some(AgentEvent::Error(message)) => assert!(
                message.contains("no conclusion"),
                "the error must say the conclusion pass came back empty: {message}"
            ),
            other => panic!("expected an error, got {other:?}"),
        }
        assert_ne!(
            history.last().map(|m| m.role),
            Some(Role::Assistant),
            "nothing may be persisted as the turn's conclusion"
        );
    }

    /// A long session must not be forwarded whole: the request is trimmed to the budget.
    #[tokio::test]
    async fn provider_request_stays_within_the_configured_context_budget() {
        const BUDGET: usize = 2_000;
        let history: Vec<ChatMessage> = (0..40)
            .map(|i| ChatMessage::user(format!("message {i}: {}", "lorem ipsum ".repeat(150))))
            .collect();
        let sizes = run_capturing_turn(history, BUDGET, None).await;
        assert_eq!(sizes.len(), 1, "one model call expected");
        assert!(
            sizes[0] <= BUDGET,
            "request estimated at {} tokens, budget is {BUDGET}",
            sizes[0]
        );
    }

    /// A model that publishes a context window sizes the budget even when no explicit cap
    /// is configured: the request must fit 90% of that window, not the 128k default.
    #[tokio::test]
    async fn budget_uses_the_model_window_when_the_cap_is_unset() {
        const WINDOW: usize = 4_000;
        let history: Vec<ChatMessage> = (0..40)
            .map(|i| ChatMessage::user(format!("message {i}: {}", "lorem ipsum ".repeat(150))))
            .collect();
        let ctx = budget_test_ctx_with(history, Arc::new(MockConfig));
        let sizes = run_capturing_turn_in(ctx, None, false, Some(WINDOW)).await;
        assert_eq!(sizes.len(), 1, "one model call expected");
        assert!(
            sizes[0] <= WINDOW * 9 / 10,
            "request estimated at {} tokens, 90% of the {WINDOW} window is {}",
            sizes[0],
            WINDOW * 9 / 10
        );
    }

    /// An upstream context-window rejection must be retried with a smaller request.
    #[tokio::test]
    async fn context_overflow_retries_with_a_smaller_request() {
        let history: Vec<ChatMessage> = (0..40)
            .map(|i| ChatMessage::user(format!("message {i}: {}", "lorem ipsum ".repeat(150))))
            .collect();
        let overflow = ModelError::TokenLimitExceeded {
            used: 60_000,
            max: 20_000,
        };
        let sizes = run_capturing_turn(history, 100_000, Some(overflow)).await;
        assert_eq!(
            sizes.len(),
            2,
            "the overflow must trigger exactly one retry"
        );
        assert!(sizes[1] < sizes[0], "retry must be smaller: {:?}", sizes);
        assert!(
            sizes[1] <= 18_000,
            "the upstream window (20k) must drive the retry, got {} tokens",
            sizes[1]
        );
    }

    /// Retries are bounded, and every retry must shrink the request (a no-op retry would
    /// resend the identical request): three rejections end the turn with an error.
    #[tokio::test]
    async fn context_overflow_gives_up_after_the_retry_budget() {
        let history: Vec<ChatMessage> = (0..40)
            .map(|i| ChatMessage::user(format!("message {i}: {}", "lorem ipsum ".repeat(150))))
            .collect();
        let overflow = ModelError::TokenLimitExceeded {
            used: 60_000,
            max: 20_000,
        };
        let sizes = run_capturing_turn_with(history, 100_000, Some(overflow), true).await;
        assert_eq!(
            sizes.len(),
            MAX_OVERFLOW_RETRIES + 1,
            "one call plus the bounded retries"
        );
        assert!(
            sizes.windows(2).all(|pair| pair[1] < pair[0]),
            "every retry must strictly shrink the request: {sizes:?}"
        );
    }

    /// Description long enough that the definition budget is a real chunk of a 2000-token
    /// request — the point of the fixture is that skipping it is visible.
    const BUDGETED_DESC: &str = "runs a command inside the sandboxed project root; prefer \
        read/grep/glob when the file is already known, keep commands idempotent, never write \
        outside the project root because the kernel guard rejects it and wastes a round trip, \
        prefer one command over a pipeline of throwaway greps, and report failures verbatim";

    /// A tool whose only job is to own a definition worth budgeting for.
    struct BudgetedTool(String);

    impl crate::module::Tool for BudgetedTool {
        fn name(&self) -> &str {
            &self.0
        }
        fn description(&self) -> &str {
            BUDGETED_DESC
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
                "required": ["cmd"]
            })
        }
        fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<serde_json::Value, crate::error::ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
    }

    /// The tool definitions shipped with every request are part of what the model has to
    /// read, so they must be reserved out of the context budget before the body is fitted.
    #[tokio::test]
    async fn provider_request_keeps_room_for_the_tool_definitions() {
        const BUDGET: usize = 2_000;
        let history: Vec<ChatMessage> = (0..20)
            .map(|i| ChatMessage::user(format!("msg {i}: {}", "x".repeat(1_000))))
            .collect();
        let ctx = budget_test_ctx(history, BUDGET);
        for name in ["budgeted-a", "budgeted-b", "budgeted-c", "budgeted-d"] {
            ctx.tools.register(Arc::new(BudgetedTool(name.into())));
        }
        let (messages, defs) = run_capturing_turn_in_recording_defs(ctx, None, false, None).await;
        let messages_tokens = messages[0];
        let defs_tokens = defs[0];
        assert!(
            defs_tokens > 300,
            "fixture definitions must be a real chunk: {defs_tokens}"
        );
        assert!(
            messages_tokens > 800,
            "fixture must still send a body: {messages_tokens}"
        );
        assert!(
            messages_tokens + defs_tokens <= BUDGET,
            "request = {} messages + {} definitions exceeds the {BUDGET} token budget",
            messages_tokens,
            defs_tokens
        );
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
            *lock_or_recover(&cap) = usage;
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
        while lock_or_recover(&captured).is_none() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            *lock_or_recover(&captured),
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
                let mut guard = lock_or_recover(&reply);
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
                AgentEvent::ToolResultTruncated { .. } => {}
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
                AgentEvent::ToolResultTruncated { .. } => {}
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
            *lock_or_recover(&self.0) = Some(options);
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
        let seen = crate::sync::lock_or_recover(&seen)
            .clone()
            .expect("chat_stream must be called");
        assert_eq!(seen.session_id.as_deref(), Some(sid.as_str()));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
    }

    /// Model override must not lose the injected `session_id` (subscription transports
    /// key on it), so this asserts both fields on the override path.
    #[tokio::test]
    async fn run_turn_model_override_still_injects_session_id() {
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
            .spawn_turn(
                sid.clone(),
                "hi".into(),
                None,
                RunOverrides {
                    provider: None,
                    model: Some("custom-model".into()),
                },
                tx,
            )
            .unwrap();
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Done { .. } => break,
                AgentEvent::Error(e) => panic!("turn error: {e}"),
                _ => {}
            }
        }
        let captured = crate::sync::lock_or_recover(&seen)
            .clone()
            .expect("chat_stream must be called");
        assert_eq!(captured.model, "custom-model");
        assert_eq!(captured.session_id, Some(sid.clone()));
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
            crate::sync::lock_or_recover(&self.seen)
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
        let out = lock_or_recover(&seen).clone();
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

    /// One turn against a single registered default provider with an optional model override;
    /// returns the full event stream plus the persisted session history.
    async fn run_turn_with(model: Option<&'static str>) -> (Vec<AgentEvent>, Vec<ChatMessage>) {
        let (kernel, sender) = spy_kernel_with(ToolSpy);
        let ctx = loop_test_ctx();
        ctx.providers.register(
            "default-provider".into(),
            Arc::new(NamedRecordingProvider {
                name: "default-provider",
                seen: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        let session = ctx.session.clone();
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
            .spawn_turn(
                sid.clone(),
                "hi".into(),
                None,
                RunOverrides {
                    provider: None,
                    model: model.map(str::to_string),
                },
                tx,
            )
            .unwrap();
        let mut events = vec![];
        while let Some(ev) = rx.recv().await {
            let terminal = matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_));
            events.push(ev);
            if terminal {
                break;
            }
        }
        let stored = session.get(&sid).unwrap_or_default();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        (events, stored)
    }

    #[tokio::test]
    async fn done_reports_the_effective_model() {
        let (events, _) = run_turn_with(Some("m-b")).await;
        let (provider, model) = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::Done {
                    provider, model, ..
                } => Some((provider.clone(), model.clone())),
                _ => None,
            })
            .expect("Done");
        assert_eq!(provider.as_deref(), Some("default-provider"));
        assert_eq!(model.as_deref(), Some("m-b"));
    }

    /// Tool whose result is a fixed-size string — the T6 truncation fixture.
    struct SizedOutputTool(usize);
    impl crate::module::Tool for SizedOutputTool {
        fn name(&self) -> &str {
            "big"
        }
        fn description(&self) -> &str {
            "returns a fixed-size output"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object", "properties": {} })
        }
        fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<serde_json::Value, crate::error::ToolError> {
            Ok(serde_json::Value::String("x".repeat(self.0)))
        }
    }

    /// Executes tools registered in a shared [`ToolRegistry`] — the in-test stand-in for
    /// the real tool-executor module.
    struct BigToolExecutor(Arc<ToolRegistry>);
    impl Module for BigToolExecutor {
        fn id(&self) -> ModuleId {
            "tool_exec".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "tool_exec".into(),
                name: "exec".into(),
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
    impl KernelModule for BigToolExecutor {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<crate::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest {
                tool_call, reply, ..
            } = msg
            {
                let reply = reply.clone();
                let result = match self.0.get(&tool_call.name) {
                    Some(tool) => tool
                        .execute(tool_call.arguments.clone())
                        .unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                };
                let mut guard = lock_or_recover(&reply);
                if let Some(tx) = guard.take() {
                    let _ = tx.send(Ok(ToolExecutionOutcome {
                        result,
                        artifacts: vec![],
                    }));
                }
            }
            Ok(crate::module::KernelResponse::Handled)
        }
    }

    /// Config that answers only the tool-result cap key.
    struct ToolResultCapConfig(i64);
    impl ConfigProvider for ToolResultCapConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, k: &str) -> Option<i64> {
            (k == "agent.max_tool_result_chars").then_some(self.0)
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    /// Pipeline that records every string handed to `reflux`, so the test can pin that the
    /// refluxed tool result is the SAME truncated string the event and store carry.
    struct RecordingPipeline(StdMutex<Vec<String>>);
    impl ContextPipeline for RecordingPipeline {
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
            lock_or_recover(&self.0).push(result.clone());
            history.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![tool_call.clone()],
            });
            history.push(ChatMessage::tool_result(&tool_call.id, result));
        }
    }

    /// One tool round against a 200 000-char output, driven to completion. Returns every
    /// emitted event and the persisted session history.
    async fn run_one_tool_turn(cap: i64) -> (Vec<AgentEvent>, Vec<ChatMessage>) {
        run_one_tool_turn_with(cap, 200_000, Arc::new(MockPipeline)).await
    }

    /// As [`run_one_tool_turn`], with a caller-chosen output length and pipeline, so the
    /// exactly-at-cap boundary and the reflux identity can be pinned against a recording
    /// pipeline instead of relied on structurally.
    async fn run_one_tool_turn_with(
        cap: i64,
        output_len: usize,
        pipeline: Arc<dyn ContextPipeline>,
    ) -> (Vec<AgentEvent>, Vec<ChatMessage>) {
        let tools = Arc::new(ToolRegistry::new());
        tools.register(Arc::new(SizedOutputTool(output_len)));
        let mut kernel = crate::kernel::Kernel::new();
        let mut register = ModuleRegister::new(tools.clone(), Arc::new(ProviderRegistry::new()));
        kernel
            .register_module(
                Box::new(BigToolExecutor(tools.clone())),
                &loop_test_ctx(),
                &mut register,
            )
            .unwrap();
        let sender = kernel.sender();
        let _handle = kernel.start();
        let session = Arc::new(MockSession {
            history: StdMutex::new(vec![]),
        });
        let ctx = KernelContext {
            session: session.clone(),
            tools,
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(ToolResultCapConfig(cap)),
        };
        ctx.providers.register(
            "mock".into(),
            Arc::new(ScriptedProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                tool: ToolCall {
                    id: "c1".into(),
                    name: "big".into(),
                    arguments: serde_json::json!({}),
                },
            }),
        );
        let runner = TurnRunner::new(
            ctx.clone(),
            pipeline,
            "sys".into(),
            sender,
            "tool_exec".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid, "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();
        let mut events = vec![];
        while let Some(ev) = rx.recv().await {
            let terminal = matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_));
            events.push(ev);
            if terminal {
                break;
            }
        }
        let stored = lock_or_recover(&session.history).clone();
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        (events, stored)
    }

    #[tokio::test]
    async fn big_tool_output_is_truncated_once() {
        let (events, stored) = run_one_tool_turn(100_000).await;
        let tool_msg = stored
            .iter()
            .find(|m| m.role == Role::Tool)
            .expect("tool message");
        assert!(tool_msg.content.chars().count() <= 100_000 + 160);
        assert!(tool_msg.content.ends_with("请缩小查询范围]…\n"));
        let dropped: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolResultTruncated { dropped_chars, .. } => Some(*dropped_chars),
                _ => None,
            })
            .collect();
        assert_eq!(dropped, vec![100_000], "one truncation with exact drop");
        // The ToolResult frame and the stored message must carry the SAME truncated string
        // (single conversion point, no re-truncation downstream).
        let frame = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .expect("tool result frame");
        assert_eq!(frame, &tool_msg.content, "event frame == stored message");
        // Ordering: the truncation notice must precede the ToolResult frame and the
        // terminal Done, so every downstream sink sees the already-capped string.
        let truncated_at = events
            .iter()
            .position(|e| matches!(e, AgentEvent::ToolResultTruncated { .. }))
            .expect("truncation event");
        let result_at = events
            .iter()
            .position(|e| matches!(e, AgentEvent::ToolResult { .. }))
            .expect("tool result frame");
        let done_at = events
            .iter()
            .position(|e| matches!(e, AgentEvent::Done { .. }))
            .expect("done event");
        assert!(
            truncated_at < result_at && result_at < done_at,
            "truncation notice must precede the ToolResult frame and Done"
        );
    }

    #[tokio::test]
    async fn small_tool_output_is_untouched() {
        let (events, stored) = run_one_tool_turn(10_000_000).await;
        let tool_msg = stored
            .iter()
            .find(|m| m.role == Role::Tool)
            .expect("tool message");
        assert_eq!(
            tool_msg.content,
            "x".repeat(200_000),
            "below-limit result must be byte-identical"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::ToolResultTruncated { .. }))
        );
    }

    #[tokio::test]
    async fn exactly_at_cap_tool_output_is_untouched() {
        let (events, stored) =
            run_one_tool_turn_with(100_000, 100_000, Arc::new(MockPipeline)).await;
        let tool_msg = stored
            .iter()
            .find(|m| m.role == Role::Tool)
            .expect("tool message");
        assert_eq!(
            tool_msg.content,
            "x".repeat(100_000),
            "an exactly-at-cap result must be byte-identical"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::ToolResultTruncated { .. })),
            "no truncation event at the exact cap"
        );
    }

    #[tokio::test]
    async fn reflux_receives_the_same_truncated_string() {
        let recording = Arc::new(RecordingPipeline(StdMutex::new(vec![])));
        let (events, _stored) = run_one_tool_turn_with(100_000, 200_000, recording.clone()).await;
        let frame = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .expect("tool result frame");
        let refluxed = lock_or_recover(&recording.0).clone();
        assert_eq!(refluxed.len(), 1, "exactly one refluxed result");
        assert_eq!(
            refluxed[0].as_str(),
            frame.as_str(),
            "reflux must see the same truncated string the event carries"
        );
    }

    /// Pipeline that panics on the first assemble — models a bug in context assembly.
    struct PanicPipeline;
    impl ContextPipeline for PanicPipeline {
        fn assemble(
            &self,
            _system: &str,
            _history: &[ChatMessage],
            _tool_results: &[(ToolCall, String)],
        ) -> Vec<ChatMessage> {
            panic!("pipeline boom")
        }
        fn reflux(&self, _history: &mut Vec<ChatMessage>, _tool_call: &ToolCall, _result: String) {}
    }

    #[tokio::test]
    async fn a_panicking_turn_reports_an_error_and_releases_the_turn_slot() {
        let (_kernel, sender) = spy_kernel_with(ToolSpy);
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
            Arc::new(PanicPipeline),
            "sys".into(),
            sender,
            "tool_spy".into(),
        );
        let sid = runner.create_session().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        runner
            .spawn_turn(sid.clone(), "hi".into(), None, RunOverrides::default(), tx)
            .unwrap();

        let mut error_text = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::Error(e) => {
                    error_text = Some(e);
                    break;
                }
                AgentEvent::Done { .. } => break,
                _ => {}
            }
        }
        let error_text = error_text.expect("a panicking turn must end with an Error event");
        assert!(
            error_text.contains("turn panicked"),
            "unexpected error text: {error_text}"
        );

        // The slot must be released: a stale entry would make cancel/shutdown act on a
        // dead turn and leak one map entry per panicking session.
        for _ in 0..20 {
            if crate::sync::lock_or_recover(&runner.turns).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            crate::sync::lock_or_recover(&runner.turns).is_empty(),
            "the panicked turn must be removed from the active-turn map"
        );
    }
}
