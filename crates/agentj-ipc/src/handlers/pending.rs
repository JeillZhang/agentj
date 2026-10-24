//! Pending-request settlement: `permission/respond`, `question/respond` and
//! owner-disconnect cleanup.
//!
//! Verbatim move out of `lib.rs`: signatures, error codes and control flow are
//! unchanged; only visibility widened to `pub(crate)` where the serve loop and
//! the notifier attach point reach in.

use crate::{
    DISCONNECTED_REASON, DispatchError, IpcServer, JsonRpcRequest, build_progress_frame, to_wire,
    trajectory,
};
use agentj_permission::ApprovalDecision;
use agentj_protocol::base::error_codes;
use agentj_protocol::permission::PermissionRespondParams;
use agentj_protocol::question::{AskQuestionParams, QuestionRespondParams, QuestionRespondResult};
use agentj_question::{
    QuestionNotifier, QuestionOutcome, QuestionSettlement, RespondError, RespondOutcome, SettleHook,
};
use serde_json::Value;
use std::sync::Arc;

impl IpcServer {
    /// `permission/respond`: deliver the client's decision to a pending approval.
    /// An unknown request_id is an error (-32600), never silent success.
    pub(crate) async fn dispatch_permission_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_request("missing params"))?;
        let params: PermissionRespondParams = serde_json::from_value(params.clone())
            .map_err(|e| DispatchError::invalid_request(e.to_string()))?;
        let module = self
            .permission_module
            .clone()
            .ok_or_else(|| DispatchError::invalid_request("no permission module"))?;
        let request_id = params.request_id.clone();
        let wire_decision = params.decision.clone();
        let rid = agentj_core::permission::PermissionRequestId(request_id.clone());
        let sid = module.session_of(&rid).ok_or_else(|| {
            DispatchError::invalid_request(format!("unknown permission request: {}", rid.0))
        })?;
        let is_owner = self
            .registry
            .read()
            .ok()
            .and_then(|r| r.session_owner(&sid).cloned())
            .map(|owner| owner == conn_id)
            .unwrap_or(false);
        if !is_owner {
            return Err(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
                data: None,
            });
        }
        let decision = match params.decision {
            agentj_protocol::permission::PermissionDecision::Allow => ApprovalDecision::Allow,
            agentj_protocol::permission::PermissionDecision::Deny => ApprovalDecision::Deny,
            agentj_protocol::permission::PermissionDecision::AllowAlways => {
                ApprovalDecision::AllowAlways
            }
            agentj_protocol::permission::PermissionDecision::Unknown => {
                return Err(DispatchError::invalid_request("unknown decision"));
            }
        };
        let view = self
            .permission_module
            .as_ref()
            .and_then(|m| m.view(&request_id));
        if !module.respond(&rid, decision) {
            return Err(DispatchError::invalid_request(format!(
                "unknown permission request: {}",
                rid.0
            )));
        }
        if let Some(view) = view {
            let sid_clone = view.session_id.clone();
            let frame = build_progress_frame(
                &Value::Null,
                &agentj_protocol::streaming::ProgressValue::Permission {
                    request_id: request_id.clone(),
                    tool: view.tool.clone(),
                    decision: wire_decision.clone(),
                    session_id: Some(sid_clone.clone()),
                },
            );
            if let Ok(reg) = self.registry.read() {
                reg.broadcast_to_session(&sid_clone, &frame);
            }
            self.record_trajectory(
                &sid_clone,
                agentj_protocol::trajectory::TrajectoryValue::Permission {
                    request_id,
                    tool: view.tool,
                    decision: wire_decision,
                },
            );
        }
        Ok(serde_json::json!({}))
    }

    /// Wire result for `question/respond` (design item 5): `false` is the
    /// idempotent answer for duplicate / already-settled / unknown ids.
    fn question_result(accepted: bool) -> Result<Value, DispatchError> {
        to_wire(QuestionRespondResult { accepted })
    }

    /// `question/respond`: owner-gated, idempotent answer delivery. Check order
    /// (design item 5): question_id shape → pending session lookup (unknown →
    /// `{accepted:false}` without owner check) → owner gate → parameter
    /// validation/settlement inside the module.
    pub(crate) fn dispatch_question_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let module = self
            .question_module
            .clone()
            .ok_or_else(|| DispatchError::invalid_request("no question module"))?;
        let raw = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let question_id = raw
            .get("question_id")
            .and_then(Value::as_str)
            .ok_or_else(|| DispatchError::invalid_params("missing question_id"))?
            .to_string();
        let Some(session_id) = module.session_of(&question_id) else {
            return Self::question_result(false);
        };
        let is_owner = self
            .registry
            .read()
            .ok()
            .and_then(|r| r.session_owner(&session_id).cloned())
            .map(|owner| owner == conn_id)
            .unwrap_or(false);
        if !is_owner {
            return Err(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
                data: None,
            });
        }
        let params: QuestionRespondParams = serde_json::from_value(raw.clone())
            .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        match module.respond(&question_id, params.answers, params.skipped) {
            Ok(RespondOutcome::Accepted(accepted)) => Self::question_result(accepted),
            Err(RespondError::UnknownOptions { unknown }) => {
                Err(DispatchError::invalid_params_with_data(
                    "unknown option ids",
                    serde_json::json!({ "unknown_options": unknown }),
                ))
            }
            Err(RespondError::TooManyAnswers) => Err(DispatchError::invalid_params_with_data(
                "multiple answers for a single-choice question",
                serde_json::json!({ "reason": "too_many_answers" }),
            )),
        }
    }

    /// Unregister a closed connection and settle whatever its sessions were
    /// waiting on.
    pub(crate) fn cleanup_connection(&self, conn_id: &str) {
        self.settle_owner_pending(conn_id);
        if let Ok(mut reg) = self.registry.write() {
            reg.unregister(conn_id);
        }
    }

    /// Settle everything still waiting on this connection's sessions: only the
    /// owner may answer a question or an approval (设计决策 #5), so once the owner is
    /// gone nothing can ever resolve them (design 错误处理: owner disconnect →
    /// skipped). Must run while `session_owner` still maps those sessions to
    /// `conn_id`, i.e. *before* `unregister`, and at the moment the connection ends
    /// rather than after in-flight runs are drained — a run waiting on an approval
    /// is itself part of that drain, so settling later would deadlock it until the
    /// approval timeout. Ownership is dropped here as well, so approvals raised
    /// later *during* the drain are denied at once instead of being routed to a
    /// connection that is already gone.
    pub(crate) fn settle_owner_pending(&self, conn_id: &str) {
        let owned = self
            .registry
            .read()
            .ok()
            .map(|r| r.sessions_owned_by(conn_id))
            .unwrap_or_default();
        if let Ok(mut reg) = self.registry.write() {
            reg.clear_owner(conn_id);
        }
        if let Some(module) = &self.question_module {
            for sid in &owned {
                let _ = module.cancel_session(sid);
            }
        }
        if let Some(module) = &self.permission_module {
            for sid in &owned {
                module.cancel_session(sid, DISCONNECTED_REASON);
            }
        }
    }

    /// Owner-directed question notifier; the returned bool is the module's
    /// "delivered" signal (an absent owner settles the question as skipped).
    pub(crate) fn question_notifier(&self) -> QuestionNotifier {
        let registry = Arc::clone(&self.registry);
        let trajectory = self.trajectory_store.clone();
        Arc::new(move |p: &AskQuestionParams, session_id: &str| {
            record_ask_question(trajectory.as_ref(), session_id, p);
            let owner = registry
                .read()
                .ok()
                .and_then(|r| r.session_owner(&session_id.to_string()).cloned());
            let delivered = owner.is_some();
            if let Some(owner_id) = owner {
                let msg = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "$/askQuestion",
                    "params": serde_json::to_value(p).expect("serializable"),
                });
                if let Ok(r) = registry.read() {
                    let _ = r.send_to(&owner_id, &msg.to_string());
                }
            }
            delivered
        })
    }

    /// Settlement sink: one QuestionAnswer trajectory node per settled question.
    pub(crate) fn question_settle_hook(&self) -> SettleHook {
        let trajectory = self.trajectory_store.clone();
        Arc::new(
            move |session_id: &str, _question_id: &str, s: &QuestionSettlement| {
                record_question_answer(trajectory.as_ref(), session_id, s);
            },
        )
    }
}

/// AskQuestion trajectory node recorded at notify time (design 决策 #10).
fn record_ask_question(
    store: Option<&Arc<trajectory::TrajectoryStore>>,
    session_id: &str,
    p: &AskQuestionParams,
) {
    let Some(store) = store else {
        return;
    };
    let value = agentj_protocol::trajectory::TrajectoryValue::AskQuestion {
        question_id: p.question_id.clone(),
        question: p.question.clone(),
        header: p.header.clone(),
        options: p.options.clone(),
        allow_multiple: p.allow_multiple,
        allow_other: p.allow_other,
    };
    if let Err(e) = store.record(&session_id.to_string(), value) {
        tracing::warn!("trajectory record failed for {session_id}: {e}");
    }
}

/// QuestionAnswer trajectory node; the settle hook fires at most once per
/// question, so exactly one node is recorded (module single-winner settle).
fn record_question_answer(
    store: Option<&Arc<trajectory::TrajectoryStore>>,
    session_id: &str,
    s: &QuestionSettlement,
) {
    let Some(store) = store else {
        return;
    };
    let (answers, skipped) = match &s.outcome {
        QuestionOutcome::Answered { answers } => (answers.clone(), false),
        QuestionOutcome::Skipped => (Vec::new(), true),
    };
    let value = agentj_protocol::trajectory::TrajectoryValue::QuestionAnswer {
        question_id: s.question_id.clone(),
        answers,
        skipped,
    };
    if let Err(e) = store.record(&session_id.to_string(), value) {
        tracing::warn!("trajectory record failed for {session_id}: {e}");
    }
}
