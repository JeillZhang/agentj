//! `session/run` streaming and cancellation handlers.
//!
//! Verbatim move out of `lib.rs`: the run slot guard, progress-frame fan-out and
//! cancel routing are unchanged; only visibility widened to `pub(crate)` where
//! the route table or the crate-root tests reach in.

use crate::{
    DispatchError, IpcServer, JsonRpcRequest, RunGuard, build_progress_frame,
    file_change_into_wire, optional_trimmed, sanitize_run_id, todo_items_into_wire,
};
use agentj_core::artifacts::{FileChangeArtifact, TodoSnapshot};
use agentj_core::module::KernelMessage;
use agentj_core::types::{AgentEvent, RunOverrides, TokenUsage, ToolCall as CoreToolCall};
use agentj_protocol::base::error_codes;
use agentj_protocol::session::{SessionRunResult, UsageInfo};
use agentj_protocol::streaming::CancelParams;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

/// Everything a `session/run` reply needs, accumulated while the stream drains.
#[derive(Default)]
struct RunAccum {
    text: String,
    /// Characters the kernel dropped per call id (design ㊵). The core sends
    /// `ToolResultTruncated` *before* the matching `ToolResult`, so the count is waiting
    /// here when the result is fanned out.
    truncated: std::collections::HashMap<String, usize>,
    tool_calls: Vec<CoreToolCall>,
    usage: Option<UsageInfo>,
    run_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    thinking: String,
}

impl RunAccum {
    /// The wire result: deltas concatenated, tool calls mapped 1:1, identity as carried.
    fn into_result(self) -> SessionRunResult {
        let tool_calls = self
            .tool_calls
            .into_iter()
            .map(|tc| agentj_protocol::types::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect();
        SessionRunResult {
            content: self.text,
            tool_calls: Some(tool_calls),
            usage: self.usage,
            run_id: self.run_id,
            provider: self.provider,
            model: self.model,
        }
    }
}

/// The `Done` event's payload, bundled so the handlers stay under the argument limit.
struct DoneParts {
    run_id: Option<String>,
    tool_calls: Vec<CoreToolCall>,
    usage: Option<TokenUsage>,
    provider: Option<String>,
    model: Option<String>,
}

/// `session/run` params → (session id, user content, per-run overrides). Overrides are
/// parsed up front so an invalid field is rejected without touching registry/owner state.
fn parse_run_params(req: &JsonRpcRequest) -> Result<(&str, &str, RunOverrides), DispatchError> {
    let params = req
        .params
        .as_ref()
        .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
    let sid = params["session_id"]
        .as_str()
        .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
    let content = params["content"].as_str().unwrap_or("");
    let overrides = RunOverrides {
        provider: optional_trimmed(params, "provider")?,
        model: optional_trimmed(params, "model")?,
    };
    Ok((sid, content, overrides))
}

/// §7.2: a `$/progress` `token` must be ProgressToken (integer|string). Echo
/// `progress_token` when provided; otherwise fall back to the request id; a
/// notification (no id) falls back to a valid number.
fn progress_token_for(req: &JsonRpcRequest) -> Value {
    match req.params.as_ref().and_then(|p| p.get("progress_token")) {
        Some(t) if !t.is_null() => t.clone(),
        _ => match &req.id {
            Value::Null => Value::Number(0.into()),
            id => id.clone(),
        },
    }
}

impl IpcServer {
    pub(crate) fn record_trajectory(
        &self,
        sid: &str,
        value: agentj_protocol::trajectory::TrajectoryValue,
    ) {
        let Some(store) = self.trajectory_store.as_ref() else {
            return;
        };
        if let Err(e) = store.record(&sid.to_string(), value) {
            tracing::warn!("trajectory record failed for {sid}: {e}");
        }
    }

    fn flush_thinking(&self, pending: &mut String, sid: &str) {
        // A gateway keeps the stream warm between tool calls by sending a lone
        // zero-width space (U+200B). U+200B is not `char::is_whitespace`, so it
        // used to be persisted as a thinking node with no visible text — which the
        // UI replayed as an empty assistant bubble. Always take the buffer first so
        // the residue cannot leak into the next node either.
        let text = std::mem::take(pending);
        if Self::is_visually_blank(&text) {
            return;
        }
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::Thinking { content: text },
        );
    }

    /// True when `s` renders as nothing: only whitespace and zero-width/format code
    /// points, which `char::is_whitespace` does not cover (U+200B is the common one).
    pub(crate) fn is_visually_blank(s: &str) -> bool {
        !s.chars().any(|c| {
            !c.is_whitespace() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}')
        })
    }

    /// Drain the turn stream, fanning every event out as it arrives; `true` means the
    /// run settled (`Done`) and the loop may stop.
    fn apply_run_event(
        &self,
        sid: &str,
        token: &Value,
        event: AgentEvent,
        acc: &mut RunAccum,
    ) -> Result<bool, DispatchError> {
        match event {
            AgentEvent::Delta(d) => self.emit_text_delta(sid, token, acc, &d),
            AgentEvent::ToolCall(tc) => self.emit_tool_call(sid, token, acc, tc),
            AgentEvent::ToolResult { call_id, result } => {
                self.emit_tool_result(sid, token, acc, call_id, result);
            }
            AgentEvent::ToolResultTruncated {
                tool_call_id,
                dropped_chars,
            } => {
                acc.truncated.insert(tool_call_id, dropped_chars);
            }
            AgentEvent::Done {
                run_id,
                tool_calls,
                usage,
                provider,
                model,
                ..
            } => {
                let done = DoneParts {
                    run_id,
                    tool_calls,
                    usage,
                    provider,
                    model,
                };
                self.emit_done(sid, token, acc, done);
                return Ok(true);
            }
            AgentEvent::Error(e) => return Err(self.run_error(sid, acc, e)),
            AgentEvent::Todo {
                run_id,
                revision,
                snapshot,
            } => self.emit_todo(sid, token, acc, (run_id, revision, snapshot)),
            AgentEvent::FileChange(artifact) => self.emit_file_change(sid, token, acc, artifact),
        }
        Ok(false)
    }

    /// `$/progress` fan-out: every subscriber of `sid` sees the same frame.
    fn broadcast_progress(&self, sid: &str, frame: &str) {
        if let Ok(reg) = self.registry.read() {
            reg.broadcast_to_session(&sid.to_string(), frame);
        }
    }

    fn emit_text_delta(&self, sid: &str, token: &Value, acc: &mut RunAccum, delta: &str) {
        acc.text.push_str(delta);
        acc.thinking.push_str(delta);
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": {
                "token": token.clone(),
                "value": { "kind": "text", "delta": delta },
            },
        })
        .to_string();
        self.broadcast_progress(sid, &frame);
    }

    fn emit_tool_call(&self, sid: &str, token: &Value, acc: &mut RunAccum, tc: CoreToolCall) {
        self.flush_thinking(&mut acc.thinking, sid);
        let wire = agentj_protocol::types::ToolCall {
            id: tc.id,
            name: tc.name,
            arguments: tc.arguments,
        };
        let frame = build_progress_frame(
            token,
            &agentj_protocol::streaming::ProgressValue::ToolCall {
                tool_call: wire.clone(),
            },
        );
        self.broadcast_progress(sid, &frame);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call: wire },
        );
    }

    fn emit_tool_result(
        &self,
        sid: &str,
        token: &Value,
        acc: &mut RunAccum,
        call_id: String,
        result: String,
    ) {
        self.flush_thinking(&mut acc.thinking, sid);
        let frame = build_progress_frame(
            token,
            &agentj_protocol::streaming::ProgressValue::ToolResult {
                tool_call_id: call_id.clone(),
                content: result.clone(),
                truncated_chars: acc.truncated.remove(&call_id),
            },
        );
        self.broadcast_progress(sid, &frame);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::ToolResult {
                tool_call_id: call_id,
                content: result,
            },
        );
    }

    /// Token-usage settlement: one `$/progress` Usage frame plus its trajectory node.
    fn emit_usage(&self, sid: &str, token: &Value, acc: &mut RunAccum, usage: &TokenUsage) {
        let info = UsageInfo {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        };
        let frame = build_progress_frame(
            token,
            &agentj_protocol::streaming::ProgressValue::Usage { usage: info },
        );
        self.broadcast_progress(sid, &frame);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::Usage { usage: info },
        );
        acc.usage = Some(info);
    }

    /// `Done` settlement: run id, usage, tool calls and the effective provider/model.
    fn emit_done(&self, sid: &str, token: &Value, acc: &mut RunAccum, done: DoneParts) {
        if let Some(run_id) = sanitize_run_id(done.run_id) {
            acc.run_id = Some(run_id);
        }
        self.flush_thinking(&mut acc.thinking, sid);
        if let Some(usage) = &done.usage {
            self.emit_usage(sid, token, acc, usage);
        }
        acc.tool_calls = done.tool_calls;
        // Record the effective provider/model once per run, only when the turn
        // carried at least one identity field.
        if done.provider.is_some() || done.model.is_some() {
            self.record_trajectory(
                sid,
                agentj_protocol::trajectory::TrajectoryValue::RunModel {
                    provider: done.provider.clone(),
                    model: done.model.clone(),
                },
            );
        }
        acc.provider = done.provider;
        acc.model = done.model;
    }

    fn emit_todo(
        &self,
        sid: &str,
        token: &Value,
        acc: &mut RunAccum,
        todo: (Option<String>, u64, TodoSnapshot),
    ) {
        let (run_id, revision, snapshot) = todo;
        let sanitized = sanitize_run_id(run_id);
        if let Some(run_id) = &sanitized {
            acc.run_id = Some(run_id.clone());
        }
        self.flush_thinking(&mut acc.thinking, sid);
        let items = todo_items_into_wire(snapshot.items);
        let frame = build_progress_frame(
            token,
            &agentj_protocol::streaming::ProgressValue::Todo {
                session_id: sid.to_string(),
                run_id: sanitized.clone(),
                revision,
                items: items.clone(),
            },
        );
        self.broadcast_progress(sid, &frame);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::Todo {
                revision,
                items,
                run_id: sanitized,
            },
        );
    }

    fn emit_file_change(
        &self,
        sid: &str,
        token: &Value,
        acc: &mut RunAccum,
        artifact: FileChangeArtifact,
    ) {
        self.flush_thinking(&mut acc.thinking, sid);
        // artifact already carries run_id/seq from TurnRunner; sanitize illegal ids to None
        let mut artifact = artifact;
        artifact.run_id = sanitize_run_id(artifact.run_id);
        if let Some(run_id) = &artifact.run_id {
            acc.run_id = Some(run_id.clone());
        }
        let change = file_change_into_wire(artifact.clone());
        let top_run_id = change.run_id.clone();
        let file_change_run_id = change.run_id.clone();
        let frame = build_progress_frame(
            token,
            &agentj_protocol::streaming::ProgressValue::FileChange {
                session_id: sid.to_string(),
                run_id: top_run_id,
                change: change.clone(),
            },
        );
        self.broadcast_progress(sid, &frame);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::FileChange {
                change,
                run_id: file_change_run_id,
            },
        );
    }

    /// A cancelled run is `-32800`; anything else keeps its message on the trajectory
    /// and surfaces as an internal error.
    fn run_error(&self, sid: &str, acc: &mut RunAccum, message: String) -> DispatchError {
        if message == "cancelled" {
            return DispatchError {
                code: -32800,
                message: "RequestCancelled".into(),
                data: None,
            };
        }
        self.flush_thinking(&mut acc.thinking, sid);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::Error {
                message: message.clone(),
            },
        );
        DispatchError::internal(message)
    }

    /// Per-session run mutex (决策 #7): a concurrent run on the same session is
    /// rejected with -32805 SessionBusy instead of interleaving streams. The guard
    /// releases the slot when the handler returns.
    fn acquire_run_slot(&self, sid: &str) -> Result<RunGuard, DispatchError> {
        let acquired = self
            .registry
            .write()
            .map(|mut r| r.try_acquire_run(&sid.to_string()))
            .unwrap_or(false);
        if !acquired {
            return Err(DispatchError {
                code: error_codes::SESSION_BUSY,
                message: "SessionBusy".into(),
                data: None,
            });
        }
        Ok(RunGuard {
            registry: Arc::clone(&self.registry),
            sid: sid.to_string(),
        })
    }

    /// The run initiator owns the session for approval routing (决策 #5) and observes
    /// `$/progress` without an explicit subscribe. Re-recording here also covers
    /// clients that skip session/new (hard-coded ids).
    fn claim_run_owner(&self, sid: &str, conn_id: &str) {
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid.to_string(), conn_id);
            reg.subscribe(&sid.to_string(), conn_id);
        }
    }

    pub(crate) async fn dispatch_session_run(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let (sid, content, overrides) = parse_run_params(req)?;
        let _run_guard = self.acquire_run_slot(sid)?;
        self.claim_run_owner(sid, conn_id);
        let progress_token = progress_token_for(req);
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::User {
                content: content.to_string(),
            },
        );
        let mut rx = self.run_turn_stream_with(sid, content, overrides);
        let mut acc = RunAccum::default();
        while let Some(event) = rx.recv().await {
            if self.apply_run_event(sid, &progress_token, event, &mut acc)? {
                break;
            }
        }
        Ok(build_run_reply(&acc.into_result()))
    }

    pub(crate) async fn dispatch_cancel_request(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params: CancelParams =
            serde_json::from_value(req.params.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| DispatchError::invalid_request(e.to_string()))?;
        let session_id = match params.session_id {
            Some(sid) => Some(sid),
            None => {
                let inflight: Vec<String> = self
                    .registry
                    .read()
                    .ok()
                    .map(|r| r.pending_runs.iter().cloned().collect())
                    .unwrap_or_default();
                match inflight.len() {
                    1 => Some(inflight[0].clone()),
                    n => {
                        if n > 1 {
                            tracing::warn!(
                                "$/cancelRequest without session_id is ambiguous ({} in-flight); ignoring",
                                n
                            );
                        }
                        None
                    }
                }
            }
        };
        if let Some(ref sid) = session_id {
            if let Some(module) = self.permission_module.as_ref() {
                let _ = module.cancel_session(sid, "cancelled");
            }
            if let Some(module) = self.question_module.as_ref() {
                let _ = module.cancel_session(sid);
            }
            let (tx, _rx) = tokio::sync::oneshot::channel();
            let _ = self.kernel.sender().send(KernelMessage::ServiceCall {
                service: "agent".into(),
                method: "cancel".into(),
                params: serde_json::json!({"session_id": sid}),
                reply: Arc::new(StdMutex::new(Some(tx))),
            });
        }
        Ok(Value::Null)
    }
}

/// Build the hand-shaped `session/run` reply. `tool_calls` is always emitted (an
/// empty array when `None`); `usage`, `run_id`, `provider` and `model` are inserted
/// conditionally so an unknown value is absent from the JSON rather than `null`.
pub(crate) fn build_run_reply(result: &SessionRunResult) -> serde_json::Value {
    let tool_calls = result.tool_calls.as_deref().unwrap_or(&[]);
    let mut obj = serde_json::json!({
        "content": result.content.clone(),
        "tool_calls": tool_calls,
    });
    if let Some(usage) = &result.usage {
        obj["usage"] = serde_json::json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
        });
    }
    if let Some(run_id) = &result.run_id
        && let Some(sanitized) = sanitize_run_id(Some(run_id.clone()))
    {
        obj["run_id"] = Value::String(sanitized);
    }
    if let Some(provider) = &result.provider {
        obj["provider"] = serde_json::json!(provider);
    }
    if let Some(model) = &result.model {
        obj["model"] = serde_json::json!(model);
    }
    obj
}
