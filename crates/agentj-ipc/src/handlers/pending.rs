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
use agentj_core::permission::PermissionRequestId;
use agentj_permission::module::PendingView;
use agentj_permission::{ApprovalDecision, PermissionModule};
use agentj_protocol::base::error_codes;
use agentj_protocol::permission::{PermissionDecision, PermissionRespondParams};
use agentj_protocol::question::{AskQuestionParams, QuestionRespondParams, QuestionRespondResult};
use agentj_question::{
    QuestionNotifier, QuestionOutcome, QuestionSettlement, RespondError, RespondOutcome, SettleHook,
};
use serde_json::Value;
use std::sync::Arc;

impl IpcServer {
    /// Owner gate (决策 #5): only the connection that claimed the session may answer
    /// its approvals and questions.
    fn require_session_owner(&self, sid: &str, conn_id: &str) -> Result<(), DispatchError> {
        let is_owner = self
            .registry
            .read()
            .ok()
            .and_then(|r| r.session_owner(sid).cloned())
            .map(|owner| owner == conn_id)
            .unwrap_or(false);
        if is_owner {
            return Ok(());
        }
        Err(DispatchError {
            code: error_codes::NOT_AUTHORIZED,
            message: "NotAuthorized".into(),
            data: None,
        })
    }

    /// `permission/respond` params → (module, request id, wire decision). A missing
    /// module is an error, never a silent success.
    fn parse_permission_respond(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<(Arc<PermissionModule>, String, PermissionDecision), DispatchError> {
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
        Ok((module, params.request_id, params.decision))
    }

    /// Settle the decision the client asked for: a `false` from the module means the
    /// request id was unknown (never silent success), and a known one also updates
    /// the client view and the trajectory.
    fn settle_permission(
        &self,
        module: &PermissionModule,
        rid: &PermissionRequestId,
        request_id: &str,
        decision: ApprovalDecision,
        wire_decision: &PermissionDecision,
    ) -> Result<(), DispatchError> {
        let view = self
            .permission_module
            .as_ref()
            .and_then(|m| m.view(request_id));
        if !module.respond(rid, decision) {
            return Err(unknown_permission(rid));
        }
        if let Some(view) = view {
            self.broadcast_permission_decision(&view, request_id, wire_decision);
        }
        Ok(())
    }

    /// `$/progress` Permission settle + trajectory node for an answered approval.
    fn broadcast_permission_decision(
        &self,
        view: &PendingView,
        request_id: &str,
        wire_decision: &PermissionDecision,
    ) {
        let sid = view.session_id.clone();
        let frame = build_progress_frame(
            &Value::Null,
            &agentj_protocol::streaming::ProgressValue::Permission {
                request_id: request_id.to_string(),
                tool: view.tool.clone(),
                decision: wire_decision.clone(),
                session_id: Some(sid.clone()),
            },
        );
        if let Ok(reg) = self.registry.read() {
            reg.broadcast_to_session(&sid, &frame);
        }
        self.record_trajectory(
            &sid,
            agentj_protocol::trajectory::TrajectoryValue::Permission {
                request_id: request_id.to_string(),
                tool: view.tool.clone(),
                decision: wire_decision.clone(),
            },
        );
    }

    /// `permission/respond`: deliver the client's decision to a pending approval.
    /// An unknown request_id is an error (-32600), never silent success.
    pub(crate) async fn dispatch_permission_respond(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let (module, request_id, wire_decision) = self.parse_permission_respond(req)?;
        let rid = PermissionRequestId(request_id.clone());
        let sid = module
            .session_of(&rid)
            .ok_or_else(|| unknown_permission(&rid))?;
        self.require_session_owner(&sid, conn_id)?;
        let decision = decision_from_wire(&wire_decision)?;
        self.settle_permission(&module, &rid, &request_id, decision, &wire_decision)?;
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
        self.require_session_owner(&session_id, conn_id)?;
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
        self.settle_sessions(conn_id, owned);
    }

    /// Settle an ownership snapshot taken just before this call. Split out so the window between
    /// "snapshot" and "settle" is explicit: `owned` can already be stale.
    pub(crate) fn settle_sessions(&self, conn_id: &str, owned: Vec<agentj_core::types::SessionId>) {
        if let Ok(mut reg) = self.registry.write() {
            reg.clear_owner(conn_id);
        }
        // `owned` is a snapshot taken before the write: another client may have claimed one of
        // these sessions in between (`session/run` claims unconditionally). Cancelling then would
        // kill an approval the *new* owner just raised, so re-check ownership and only settle
        // sessions nobody owns now.
        let orphaned = self.still_ownerless(&owned);
        if let Some(module) = &self.question_module {
            for sid in &orphaned {
                let _ = module.cancel_session(sid);
            }
        }
        if let Some(module) = &self.permission_module {
            for sid in &orphaned {
                module.cancel_session(sid, DISCONNECTED_REASON);
            }
        }
    }

    /// Filter a pre-clear ownership snapshot down to the sessions that are still unowned.
    fn still_ownerless(&self, owned: &[String]) -> Vec<String> {
        let Ok(reg) = self.registry.read() else {
            return Vec::new();
        };
        owned
            .iter()
            .filter(|sid| reg.session_owner(sid).is_none())
            .cloned()
            .collect()
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
                .and_then(|r| r.session_owner(session_id).cloned());
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

/// The single error for an unknown/expired approval id (both lookups share it).
fn unknown_permission(rid: &PermissionRequestId) -> DispatchError {
    DispatchError::invalid_request(format!("unknown permission request: {}", rid.0))
}

/// Wire decision → engine decision; the forward-compatible `Unknown` variant is a
/// rejected parameter rather than a silent deny.
fn decision_from_wire(decision: &PermissionDecision) -> Result<ApprovalDecision, DispatchError> {
    match decision {
        PermissionDecision::Allow => Ok(ApprovalDecision::Allow),
        PermissionDecision::Deny => Ok(ApprovalDecision::Deny),
        PermissionDecision::AllowAlways => Ok(ApprovalDecision::AllowAlways),
        PermissionDecision::Unknown => Err(DispatchError::invalid_request("unknown decision")),
    }
}
