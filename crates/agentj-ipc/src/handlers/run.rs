//! `session/run` streaming and cancellation handlers.
//!
//! Verbatim move out of `lib.rs`: the run slot guard, progress-frame fan-out and
//! cancel routing are unchanged; only visibility widened to `pub(crate)` where
//! the route table or the crate-root tests reach in.

use crate::{
    DispatchError, IpcServer, JsonRpcRequest, RunGuard, build_progress_frame,
    file_change_into_wire, optional_trimmed, sanitize_run_id, todo_items_into_wire,
};
use agentj_core::module::KernelMessage;
use agentj_core::types::{AgentEvent, RunOverrides};
use agentj_protocol::base::error_codes;
use agentj_protocol::session::{SessionRunResult, UsageInfo};
use agentj_protocol::streaming::CancelParams;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

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

    pub(crate) async fn dispatch_session_run(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let sid = params["session_id"]
            .as_str()
            .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
        let content = params["content"].as_str().unwrap_or("");
        // Per-run model overrides (T9): parsed before the run mutex so an
        // invalid field is rejected without touching registry/owner state.
        let overrides = RunOverrides {
            provider: optional_trimmed(params, "provider")?,
            model: optional_trimmed(params, "model")?,
        };
        // Per-session run mutex (决策 #7): a concurrent run on the same session
        // is rejected with -32805 SessionBusy instead of interleaving streams.
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
        let _run_guard = RunGuard {
            registry: Arc::clone(&self.registry),
            sid: sid.to_string(),
        };
        // The run initiator owns the session for approval routing (决策 #5)
        // and observes $/progress without an explicit subscribe. Re-recording
        // here also covers clients that skip session/new (hard-coded ids).
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid.to_string(), conn_id);
            reg.subscribe(&sid.to_string(), conn_id);
        }
        // §7.2: a `$/progress` `token` must be ProgressToken (integer|string).
        // Echo `progress_token` when provided; otherwise fall back to the
        // request id; a notification (no id) falls back to a valid number.
        let progress_token = match params.get("progress_token") {
            Some(t) if !t.is_null() => t.clone(),
            _ => match &req.id {
                Value::Null => Value::Number(0.into()),
                id => id.clone(),
            },
        };
        self.record_trajectory(
            sid,
            agentj_protocol::trajectory::TrajectoryValue::User {
                content: content.to_string(),
            },
        );
        let mut rx = self.run_turn_stream_with(sid, content, overrides);
        let mut text = String::new();
        let mut tc_list = vec![];
        let mut usage_opt: Option<UsageInfo> = None;
        let mut current_run_id: Option<String> = None;
        let mut result_provider: Option<String> = None;
        let mut result_model: Option<String> = None;
        let mut pending_thinking = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Delta(d) => {
                    text.push_str(&d);
                    pending_thinking.push_str(&d);
                    let progress = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "$/progress",
                        "params": {
                            "token": progress_token.clone(),
                            "value": { "kind": "text", "delta": d },
                        },
                    })
                    .to_string();
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &progress);
                    }
                }
                AgentEvent::ToolCall(tc) => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    let wire: agentj_protocol::types::ToolCall = agentj_protocol::types::ToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    };
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::ToolCall {
                            tool_call: wire.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call: wire },
                    );
                }
                AgentEvent::ToolResult { call_id, result } => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::ToolResult {
                            tool_call_id: call_id.clone(),
                            content: result.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::ToolResult {
                            tool_call_id: call_id,
                            content: result,
                        },
                    );
                }
                AgentEvent::ToolResultTruncated { .. } => {}
                AgentEvent::Done {
                    run_id,
                    tool_calls,
                    usage,
                    provider,
                    model,
                    ..
                } => {
                    let done_run_id = sanitize_run_id(run_id);
                    if done_run_id.is_some() {
                        current_run_id = done_run_id.clone();
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    if let Some(u) = &usage {
                        let info = agentj_protocol::session::UsageInfo {
                            prompt_tokens: u.prompt_tokens,
                            completion_tokens: u.completion_tokens,
                        };
                        let frame = build_progress_frame(
                            &progress_token,
                            &agentj_protocol::streaming::ProgressValue::Usage { usage: info },
                        );
                        if let Ok(reg) = self.registry.read() {
                            reg.broadcast_to_session(&sid.to_string(), &frame);
                        }
                        self.record_trajectory(
                            sid,
                            agentj_protocol::trajectory::TrajectoryValue::Usage { usage: info },
                        );
                    }
                    tc_list = tool_calls;
                    usage_opt = usage.map(|u| UsageInfo {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                    });
                    // Record the effective provider/model once per run, only when
                    // the turn carried at least one identity field.
                    if provider.is_some() || model.is_some() {
                        self.record_trajectory(
                            sid,
                            agentj_protocol::trajectory::TrajectoryValue::RunModel {
                                provider: provider.clone(),
                                model: model.clone(),
                            },
                        );
                    }
                    result_provider = provider;
                    result_model = model;
                    break;
                }
                AgentEvent::Error(e) => {
                    if e == "cancelled" {
                        return Err(DispatchError {
                            code: -32800,
                            message: "RequestCancelled".into(),
                            data: None,
                        });
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::Error { message: e.clone() },
                    );
                    return Err(DispatchError::internal(e));
                }
                AgentEvent::Todo {
                    run_id,
                    revision,
                    snapshot,
                } => {
                    let sanitized = sanitize_run_id(run_id);
                    if sanitized.is_some() {
                        current_run_id = sanitized.clone();
                    }
                    self.flush_thinking(&mut pending_thinking, sid);
                    let items = todo_items_into_wire(snapshot.items);
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::Todo {
                            session_id: sid.to_string(),
                            run_id: sanitized.clone(),
                            revision,
                            items: items.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::Todo {
                            revision,
                            items,
                            run_id: sanitized,
                        },
                    );
                }
                AgentEvent::FileChange(artifact) => {
                    self.flush_thinking(&mut pending_thinking, sid);
                    // artifact already carries run_id/seq from TurnRunner; sanitize illegal ids to None
                    let mut artifact = artifact;
                    artifact.run_id = sanitize_run_id(artifact.run_id);
                    if artifact.run_id.is_some() {
                        current_run_id = artifact.run_id.clone();
                    }
                    let change = file_change_into_wire(artifact.clone());
                    let top_run_id = change.run_id.clone();
                    let file_change_run_id = change.run_id.clone();
                    let frame = build_progress_frame(
                        &progress_token,
                        &agentj_protocol::streaming::ProgressValue::FileChange {
                            session_id: sid.to_string(),
                            run_id: top_run_id,
                            change: change.clone(),
                        },
                    );
                    if let Ok(reg) = self.registry.read() {
                        reg.broadcast_to_session(&sid.to_string(), &frame);
                    }
                    self.record_trajectory(
                        sid,
                        agentj_protocol::trajectory::TrajectoryValue::FileChange {
                            change,
                            run_id: file_change_run_id,
                        },
                    );
                }
            }
        }
        let tool_calls = tc_list
            .into_iter()
            .map(|tc| agentj_protocol::types::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect();
        let result = SessionRunResult {
            content: text,
            tool_calls: Some(tool_calls),
            usage: usage_opt,
            run_id: current_run_id,
            provider: result_provider,
            model: result_model,
        };
        Ok(build_run_reply(&result))
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
