//! PermissionModule: bridges PermissionEngine into the kernel (Module trait,
//! design 决策 #17) and owns the approval channel.
//!
//! Channel design: each pending request carries a `watch::Sender<Option<Decision>>`
//! — None while awaiting, Some once responded. `watch` is used instead of
//! `oneshot` because respond() and await_approval() are concurrent. The entry
//! keeps an initial receiver alive (`_keepalive`), so a respond that arrives
//! BEFORE await_approval subscribes is still stored and delivered on subscribe
//! (a zero-receiver `watch::Sender::send` would drop the value).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

use agentj_core::error::ModuleError;
use agentj_core::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister, ToolSource,
};
use agentj_core::permission::{
    PermissionChecker, PermissionContext, PermissionDecision, PermissionRequestId,
};
use agentj_core::types::ModuleId;
use agentj_protocol::permission::PermissionRequestParams;
use async_trait::async_trait;
use uuid::Uuid;

use crate::engine::{EvalOutcome, PermissionEngine};
use crate::error::PermissionError;
use crate::policy::{PermissionMode, ToolPolicy, builtin_default_mode};
use crate::session_allow::{RuleFingerprint, SessionAllowState};

/// Decision delivered by the client via permission/respond.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Allow,
    Deny,
    AllowAlways,
}

/// Pending approval entry. `decision` is None while awaiting, Some(resolved)
/// once respond() delivers (watch keeps the value across the respond/await race).
///
/// Channel design: each pending request carries a `watch::Sender<Option<Decision>>`
/// — None while awaiting, Some once responded. `watch` is used instead of
/// `oneshot` because respond() and await_approval() are concurrent. `register_ask`
/// keeps a `_rx` alive in the entry (see register_ask) so a respond that arrives
/// BEFORE await_approval subscribes is still stored and delivered — the guard
/// against the respond/await race is exact, not approximate.
#[derive(Debug, Clone)]
struct PendingRequest {
    decision: Arc<watch::Sender<Option<PermissionDecision>>>,
    // Keep a receiver alive: `watch::Sender::send` with zero receivers drops the
    // value; an early respond must not be lost before await_approval subscribes.
    _keepalive: watch::Receiver<Option<PermissionDecision>>,
    tool: String,
    args: serde_json::Value,
    session_id: String,
}

/// Notification sink for Ask requests (set once at startup by the IPC layer).
type Notifier = Arc<dyn Fn(&PermissionRequestParams) + Send + Sync>;

#[derive(Clone)]
pub struct PermissionModule {
    engine: Arc<PermissionEngine>,
    policies: Arc<HashMap<String, ToolPolicy>>,
    session_state: Arc<SessionAllowState>,
    pending: Arc<Mutex<HashMap<PermissionRequestId, PendingRequest>>>,
    timeout: Duration,
    /// Emitted when an Ask request is registered (IPC layer subscribes).
    notifier: Arc<Mutex<Option<Notifier>>>,
}

impl PermissionModule {
    pub fn new(policies: HashMap<String, ToolPolicy>, timeout: Duration) -> Self {
        Self {
            engine: Arc::new(PermissionEngine::new()),
            policies: Arc::new(policies),
            session_state: Arc::new(SessionAllowState::new()),
            pending: Arc::new(Mutex::new(HashMap::new())),
            timeout,
            notifier: Arc::new(Mutex::new(None)),
        }
    }

    /// Register the notification sink (IPC layer). Called once at startup.
    pub fn set_notifier(&self, f: Notifier) {
        *self.notifier.lock().unwrap() = Some(f);
    }

    /// Deliver the client's decision to a pending request. `AllowAlways`
    /// derives the fingerprint from the stored args and grants it for the
    /// request's session (session-scoped, in-memory).
    /// Returns true if the request was found and resolved; false if the
    /// request_id is unknown (caller must return an error to the client).
    pub fn respond(&self, request_id: &PermissionRequestId, decision: ApprovalDecision) -> bool {
        let Some(req) = self.pending.lock().unwrap().remove(request_id) else {
            return false; // unknown request id
        };
        let resolved = match decision {
            ApprovalDecision::Allow => PermissionDecision::Allow,
            ApprovalDecision::Deny => PermissionDecision::Deny {
                reason: "denied by user".into(),
            },
            ApprovalDecision::AllowAlways => {
                let key = rule_key_for(&req.tool, &req.args);
                self.session_state.grant(
                    &req.session_id,
                    RuleFingerprint {
                        tool: req.tool.clone(),
                        rule_key: key,
                    },
                );
                PermissionDecision::Allow
            }
        };
        let _ = req.decision.send(Some(resolved));
        true
    }

    /// Resolve ALL pending approvals for a session to Deny (session
    /// termination / $/cancelRequest / client disconnect).
    pub fn cancel_session(&self, session_id: &str, reason: &str) -> usize {
        let mut map = self.pending.lock().unwrap();
        let mut n = 0;
        map.retain(|_rid, req| {
            if req.session_id == session_id {
                let _ = req.decision.send(Some(PermissionDecision::Deny {
                    reason: reason.to_string(),
                }));
                n += 1;
                false
            } else {
                true
            }
        });
        n
    }

    fn eval(&self, ctx: &PermissionContext) -> PermissionDecision {
        let policy = self.policies.get(&ctx.tool).cloned().unwrap_or_else(|| {
            // No configured policy — factory default mode (design decision table).
            ToolPolicy {
                mode: builtin_default_mode(&ctx.tool),
                path_rules: vec![],
                command_rules: vec![],
            }
        });
        let fp = RuleFingerprint {
            tool: ctx.tool.clone(),
            rule_key: rule_key_for(&ctx.tool, &ctx.args),
        };
        // 1. allow_always grant hit
        if self.session_state.allows(&ctx.session_id, &fp) {
            return PermissionDecision::Allow;
        }
        // 2. rule evaluation (Deny > Allow > Mode)
        let outcome = match ctx.source {
            ToolSource::Mcp { .. } | ToolSource::Plugin { .. } => self.eval_external(ctx, &policy),
            ToolSource::Builtin => self.eval_builtin(ctx, &policy),
        };
        match outcome {
            EvalOutcome::Allow => PermissionDecision::Allow,
            EvalOutcome::Deny => PermissionDecision::Deny {
                reason: "matched deny rule".into(),
            },
            EvalOutcome::Mode(PermissionMode::Deny) => PermissionDecision::Deny {
                reason: "tool default mode is deny".into(),
            },
            EvalOutcome::Mode(PermissionMode::Allow) => PermissionDecision::Allow,
            EvalOutcome::Mode(PermissionMode::Ask) => self.register_ask(ctx),
        }
    }

    fn eval_builtin(&self, ctx: &PermissionContext, policy: &ToolPolicy) -> EvalOutcome {
        if ctx.tool == "bash" {
            let cmd = ctx.args.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            self.engine.eval_command(policy, cmd)
        } else if let Some(path) = ctx.args.get("path").and_then(|v| v.as_str()) {
            self.engine.eval_path(policy, &ctx.workspace_root, path)
        } else {
            EvalOutcome::Mode(policy.mode)
        }
    }

    fn eval_external(&self, ctx: &PermissionContext, policy: &ToolPolicy) -> EvalOutcome {
        // MCP/plugin tools: an allow rule wins; otherwise Ask (no default Allow).
        if let Some(path) = ctx.args.get("path").and_then(|v| v.as_str()) {
            let o = self.engine.eval_path(policy, &ctx.workspace_root, path);
            if o == EvalOutcome::Allow {
                return o;
            }
        }
        EvalOutcome::Mode(PermissionMode::Ask)
    }

    fn register_ask(&self, ctx: &PermissionContext) -> PermissionDecision {
        let request_id = PermissionRequestId(Uuid::new_v4().to_string());
        let (tx, rx) = watch::channel(None);
        self.pending.lock().unwrap().insert(
            request_id.clone(),
            PendingRequest {
                decision: Arc::new(tx),
                // Keep the initial receiver alive so an early respond() (before
                // await_approval subscribes) is stored, not silently dropped.
                _keepalive: rx,
                tool: ctx.tool.clone(),
                args: ctx.args.clone(),
                session_id: ctx.session_id.clone(),
            },
        );
        let params = PermissionRequestParams {
            request_id: request_id.0.clone(),
            tool: ctx.tool.clone(),
            args: ctx.args.clone(),
            reason: format!("tool '{}' default mode is ask", ctx.tool),
        };
        if let Some(f) = self.notifier.lock().unwrap().clone() {
            f(&params);
        }
        PermissionDecision::Ask { request_id }
    }
}

/// Fingerprint key for a tool invocation: bash → cmd, file tools → path.
fn rule_key_for(tool: &str, args: &serde_json::Value) -> String {
    if tool == "bash" {
        args.get("cmd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        args.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

#[async_trait]
impl PermissionChecker for PermissionModule {
    fn check(&self, ctx: &PermissionContext) -> PermissionDecision {
        self.eval(ctx)
    }

    async fn await_approval(&self, request_id: &PermissionRequestId) -> PermissionDecision {
        let Some(req) = self.pending.lock().unwrap().get(request_id).cloned() else {
            return PermissionDecision::Deny {
                reason: "request not found".into(),
            };
        };
        let mut rx = req.decision.subscribe();
        // Map to a plain bool so wait_for's Ref guard is dropped before the
        // match arms re-borrow `rx` (otherwise E0502 on the scrutinee).
        let received = tokio::time::timeout(self.timeout, rx.wait_for(|d| d.is_some()))
            .await
            .map(|res| res.is_ok());
        match received {
            Ok(true) => rx.borrow().clone().unwrap_or(PermissionDecision::Deny {
                reason: "no decision".into(),
            }),
            Ok(false) => PermissionDecision::Deny {
                reason: "sender dropped".into(),
            },
            Err(_) => {
                tracing::warn!(
                    request_id = %request_id.0,
                    timeout_ms = self.timeout.as_millis(),
                    "permission approval timed out"
                );
                // Design 错误传播链: 超时 → PermissionError::RequestTimeout → Deny.
                // (Recorded for observability; the caller surface is
                // ToolError::PermissionDenied via execute_async.)
                let _ = PermissionError::RequestTimeout {
                    request_id: request_id.0.clone(),
                    timeout: self.timeout,
                };
                PermissionDecision::Deny {
                    reason: "timeout".into(),
                }
            }
        }
    }
}

impl Module for PermissionModule {
    fn id(&self) -> ModuleId {
        "permission".into()
    }
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "permission".into(),
            name: "Permission".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Tool permission management".into(),
            author: "agentj".into(),
        }
    }
    fn init(
        &mut self,
        _ctx: &KernelContext,
        _register: &mut ModuleRegister,
    ) -> Result<(), ModuleError> {
        Ok(())
    }
    fn shutdown(&mut self) -> Result<(), ModuleError> {
        self.clear_all();
        Ok(())
    }
}

/// KernelModule — the v0.1 kernel's register_module accepts `Box<dyn KernelModule>`
/// (Module + handle). The permission module participates in lifecycle only; it
/// routes no bus messages, so handle() is a no-op (returns Handled).
impl KernelModule for PermissionModule {
    fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        Ok(KernelResponse::Handled)
    }
}

impl PermissionModule {
    /// Clear session grants and resolve every pending approval to Deny.
    pub fn clear_all(&self) {
        self.session_state.clear_all();
        let mut map = self.pending.lock().unwrap();
        for (_, req) in map.drain() {
            let _ = req.decision.send(Some(PermissionDecision::Deny {
                reason: "shutdown".into(),
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::*;
    use crate::session_allow::RuleFingerprint;
    use agentj_core::module::{Module, ToolSource};
    use agentj_core::permission::{PermissionContext, PermissionDecision};
    use std::path::PathBuf;
    use std::time::Duration;

    fn module() -> PermissionModule {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "bash".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![CommandRule {
                    effect: PermissionEffect::Allow,
                    prefix: "npm test".into(),
                }],
            },
        );
        PermissionModule::new(policies, Duration::from_millis(50))
    }

    fn ctx(_module: &PermissionModule, cmd: &str) -> PermissionContext {
        PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"cmd": cmd}),
            workspace_root: PathBuf::from("/ws"),
        }
    }

    #[tokio::test]
    async fn ask_requires_approval_flow() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let d = m.check(&c);
        assert!(matches!(d, PermissionDecision::Ask { .. }));
        if let PermissionDecision::Ask { request_id } = d {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            // Give the awaiting task time to register, then respond.
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::Allow);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
        }
    }

    #[tokio::test]
    async fn allow_rule_bypasses_ask() {
        let m = module();
        let c = ctx(&m, "npm test -- --watch");
        assert_eq!(m.check(&c), PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn allow_always_respond_records_grant() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let d = m.check(&c);
        assert!(matches!(d, PermissionDecision::Ask { .. }));
        if let PermissionDecision::Ask { request_id } = d {
            // Respond allow_always THROUGH the respond() path — not direct grant.
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
            // The SAME command in the SAME session must now be allowed without asking.
            assert_eq!(m.check(&c), PermissionDecision::Allow);
        } else {
            panic!("expected Ask");
        }
    }

    #[tokio::test]
    async fn allow_always_not_leaking_to_other_session() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        if let PermissionDecision::Ask { request_id } = m.check(&c) {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            handle.await.unwrap();
        }
        // Different session must still Ask.
        let mut c2 = ctx(&m, "curl http://example.com");
        c2.session_id = "s2".into();
        assert!(matches!(m.check(&c2), PermissionDecision::Ask { .. }));
    }

    #[tokio::test]
    async fn approval_timeout_auto_denies() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        if let PermissionDecision::Ask { request_id } = m.check(&c) {
            let d = m.await_approval(&request_id).await;
            assert_eq!(
                d,
                PermissionDecision::Deny {
                    reason: "timeout".into()
                }
            );
        } else {
            panic!("expected Ask");
        }
    }

    #[test]
    fn module_identity_and_shutdown() {
        let mut m = module(); // Module::shutdown takes &mut self
        assert_eq!(m.id(), "permission");
        m.session_state.grant(
            "s1",
            RuleFingerprint {
                tool: "bash".into(),
                rule_key: "x".into(),
            },
        );
        assert!(m.session_state.allows(
            "s1",
            &RuleFingerprint {
                tool: "bash".into(),
                rule_key: "x".into()
            }
        ));
        Module::shutdown(&mut m).unwrap();
        assert!(!m.session_state.allows(
            "s1",
            &RuleFingerprint {
                tool: "bash".into(),
                rule_key: "x".into()
            }
        ));
    }

    #[tokio::test]
    async fn shutdown_resolves_pending_to_deny() {
        let mut m = module();
        let c = ctx(&m, "curl http://example.com");
        if let PermissionDecision::Ask { request_id } = m.check(&c) {
            let m2 = m.clone();
            let handle = tokio::spawn(async move { m2.await_approval(&request_id).await });
            tokio::time::sleep(Duration::from_millis(10)).await;
            Module::shutdown(&mut m).unwrap();
            assert_eq!(
                handle.await.unwrap(),
                PermissionDecision::Deny {
                    reason: "shutdown".into()
                }
            );
        } else {
            panic!("expected Ask");
        }
    }
}
