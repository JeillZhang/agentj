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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

use agentj_core::error::ModuleError;
use agentj_core::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister, ToolSource,
};
use agentj_core::pending::PendingRegistry;
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
use agentj_core::sync::lock_or_recover;

/// Decision delivered by the client via permission/respond.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Allow,
    Deny,
    AllowAlways,
}

/// How long a settled-but-unconsumed pending entry may linger before the
/// registry sweeps it. Far above the approval timeout (60s default), so no live
/// waiter can lose its decision to pruning.
const PENDING_TTL: Duration = Duration::from_secs(300);

/// Pending approval entry. `decision` is None while awaiting, Some(resolved)
/// once respond() delivers (watch keeps the value across the respond/await race).
///
/// Channel design: each pending request carries a `watch::Sender<Option<Decision>>`
/// — None while awaiting, Some once responded. `watch` is used instead of
/// `oneshot` because respond() and await_approval() are concurrent. `register_ask`
/// keeps a `_rx` alive in the entry (see register_ask) so a respond that arrives
/// BEFORE await_approval subscribes is still stored and delivered — the guard
/// against the respond/await race is exact, not approximate.
///
/// `finished` is the single-winner arbiter for the *decision*: the first of
/// respond / cancel_session / clear_all to swap it delivers, every other settler
/// gets `false`. It must be an `Arc` so the clones handed out by
/// `PendingRegistry::get` observe the same flag as the stored entry.
#[derive(Debug, Clone)]
struct PendingRequest {
    decision: Arc<watch::Sender<Option<PermissionDecision>>>,
    // Keep a receiver alive: `watch::Sender::send` with zero receivers drops the
    // value; an early respond must not be lost before await_approval subscribes.
    _keepalive: watch::Receiver<Option<PermissionDecision>>,
    finished: Arc<AtomicBool>,
    tool: String,
    args: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    pub tool: String,
    pub session_id: String,
}

/// Notification sink for Ask requests (set once at startup by the IPC layer).
type Notifier = Arc<dyn Fn(&PermissionRequestParams, &str) + Send + Sync>;

#[derive(Clone)]
pub struct PermissionModule {
    engine: Arc<PermissionEngine>,
    policies: Arc<HashMap<String, ToolPolicy>>,
    session_state: Arc<SessionAllowState>,
    /// Session-tagged pending table (shared via `Arc`: the registry is itself
    /// interior-mutable, and clones of this module must see one table).
    pending: Arc<PendingRegistry<PermissionRequestId, PendingRequest>>,
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
            pending: Arc::new(PendingRegistry::new(Some(PENDING_TTL))),
            timeout,
            notifier: Arc::new(Mutex::new(None)),
        }
    }

    /// Register the notification sink (IPC layer). Called once at startup.
    pub fn set_notifier(&self, f: Notifier) {
        *lock_or_recover(&self.notifier) = Some(f);
    }

    /// Read-only peek of the session owning a pending approval. Does NOT
    /// consume the entry (consumption stays in `respond`). Used by the IPC
    /// layer to gate `permission/respond` by the resolved session's owner
    /// (G2 design D4).
    pub fn session_of(&self, request_id: &PermissionRequestId) -> Option<String> {
        self.pending.session_of(request_id)
    }

    pub fn view(&self, request_id: &str) -> Option<PendingView> {
        let key = PermissionRequestId(request_id.to_string());
        let entry = self.pending.get(&key)?;
        Some(PendingView {
            tool: entry.tool,
            session_id: self.pending.session_of(&key)?,
        })
    }

    /// Deliver the client's decision to a pending request. `AllowAlways`
    /// derives the fingerprint from the stored args and grants it for the
    /// request's session (session-scoped, in-memory).
    ///
    /// The entry is NOT removed here: the awaiting tool loop consumes it in
    /// `await_approval` (and again `await_approval` is what removes it). A
    /// decision that arrives before the waiter subscribes must stay readable —
    /// removing it here made the tool loop deny its own approved call with
    /// "request not found" whenever the client answered first.
    /// Returns true when this call is the first decision for the request;
    /// false for an unknown id or an already-settled request. Single-winner is
    /// *enforced*, not hoped for: the entry's `finished` flag is swapped before
    /// the decision is sent, so two concurrent responds (or a respond racing
    /// cancel_session / clear_all) can never both deliver.
    pub fn respond(&self, request_id: &PermissionRequestId, decision: ApprovalDecision) -> bool {
        let Some(req) = self.pending.get(request_id) else {
            return false; // unknown (or already consumed/pruned) request id
        };
        if req.finished.swap(true, Ordering::AcqRel) {
            return false; // already settled (another respond / cancel_session / shutdown)
        }
        let Some(session_id) = self.pending.session_of(request_id) else {
            return false;
        };
        let resolved = match decision {
            ApprovalDecision::Allow => PermissionDecision::Allow,
            ApprovalDecision::Deny => PermissionDecision::Deny {
                reason: "denied by user".into(),
            },
            ApprovalDecision::AllowAlways => {
                let key = rule_key_for(&req.tool, &req.args);
                self.session_state.grant(
                    &session_id,
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
    /// termination / $/cancelRequest / client disconnect). The entries stay
    /// until their tool loop consumes the decision, so the waiter reports the
    /// real reason instead of a missing request. Claims each entry with the same
    /// `finished` flag as `respond`, so a disconnect Deny and a user Allow can
    /// never both settle one request.
    pub fn cancel_session(&self, session_id: &str, reason: &str) -> usize {
        let mut n = 0;
        self.pending.for_session(session_id, |_, req| {
            if !req.finished.swap(true, Ordering::AcqRel) {
                let _ = req.decision.send(Some(PermissionDecision::Deny {
                    reason: reason.to_string(),
                }));
                n += 1;
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
            let cmd = bash_arg(&ctx.args).unwrap_or("");
            return self.engine.eval_command(policy, cmd);
        }
        // File/glob tools: path rules bind on the path-semantic arg ONLY, and
        // that key is tool-specific (path_arg_keys). Deny > Allow: any decisive
        // outcome wins.
        let mut any_allow = false;
        for key in path_arg_keys(&ctx.tool) {
            let Some(arg) = ctx.args.get(key).and_then(|v| v.as_str()) else {
                continue;
            };
            match self.engine.eval_path(policy, &ctx.workspace_root, arg) {
                EvalOutcome::Deny => return EvalOutcome::Deny,
                EvalOutcome::Allow => any_allow = true,
                EvalOutcome::Mode(_) => {}
            }
        }
        if any_allow {
            EvalOutcome::Allow
        } else {
            EvalOutcome::Mode(policy.mode)
        }
    }

    fn eval_external(&self, ctx: &PermissionContext, policy: &ToolPolicy) -> EvalOutcome {
        // MCP/plugin tools: rules evaluate first (Deny > Allow), only a miss
        // falls back to Ask (no default Allow for external tools).
        if let Some(path) = ctx.args.get("path").and_then(|v| v.as_str()) {
            let o = self.engine.eval_path(policy, &ctx.workspace_root, path);
            if o != EvalOutcome::Mode(policy.mode) {
                return o;
            }
        }
        EvalOutcome::Mode(PermissionMode::Ask)
    }

    fn register_ask(&self, ctx: &PermissionContext) -> PermissionDecision {
        let request_id = PermissionRequestId(Uuid::new_v4().to_string());
        let (tx, rx) = watch::channel(None);
        let inserted = self.pending.insert(
            request_id.clone(),
            &ctx.session_id,
            PendingRequest {
                decision: Arc::new(tx),
                // Keep the initial receiver alive so an early respond() (before
                // await_approval subscribes) is stored, not silently dropped.
                _keepalive: rx,
                finished: Arc::new(AtomicBool::new(false)),
                tool: ctx.tool.clone(),
                args: ctx.args.clone(),
            },
        );
        // The registry never overwrites, so a colliding id would hand this ask
        // the previous request's waiter channel. Enforce the invariant loudly in
        // debug builds instead of dropping the bool on the floor.
        debug_assert!(inserted, "duplicate approval request id {}", request_id.0);
        let params = PermissionRequestParams {
            request_id: request_id.0.clone(),
            tool: ctx.tool.clone(),
            args: ctx.args.clone(),
            reason: format!("tool '{}' default mode is ask", ctx.tool),
            origin_client: None,
            session_id: Some(ctx.session_id.clone()),
        };
        if let Ok(guard) = self.notifier.lock()
            && let Some(f) = guard.clone()
        {
            f(&params, &ctx.session_id);
        }
        PermissionDecision::Ask { request_id }
    }
}

/// Path-semantic argument keys per builtin tool. grep's `pattern` is a regex
/// search term (GrepTool passes it to `rg <pattern>`), NOT a path — only its
/// `include` arg (`rg --glob`) is path-like. glob's `pattern` IS a path glob;
/// read/write (and unknown builtins) bind on `path`.
fn path_arg_keys(tool: &str) -> &'static [&'static str] {
    match tool {
        "grep" => &["include"],
        "glob" => &["pattern"],
        _ => &["path"],
    }
}

/// Read the bash command argument: the REAL schema field is "command"
/// (BashTool input_schema); "cmd" is tolerated as a legacy fallback so old
/// callers/tests keep working. Missing/empty → "" (matches no rule).
fn bash_arg(args: &serde_json::Value) -> Option<&str> {
    args.get("command")
        .or_else(|| args.get("cmd"))
        .and_then(|v| v.as_str())
}

/// Fingerprint key for a tool invocation, tool-specific: bash → command;
/// grep → pattern+include (both delimit one search); glob → pattern;
/// read/write (and unknown builtins) → path.
fn rule_key_for(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "bash" => bash_arg(args).unwrap_or("").trim().to_string(),
        "grep" => {
            let pattern = args
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let include = args
                .get("include")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            format!("{pattern}\u{0}{include}")
        }
        "glob" => args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
    }
}

#[async_trait]
impl PermissionChecker for PermissionModule {
    fn check(&self, ctx: &PermissionContext) -> PermissionDecision {
        self.eval(ctx)
    }

    async fn await_approval(&self, request_id: &PermissionRequestId) -> PermissionDecision {
        let Some(req) = self.pending.get(request_id) else {
            return PermissionDecision::Deny {
                reason: "unknown or expired approval request".into(),
            };
        };
        let mut rx = req.decision.subscribe();
        // Map to a plain bool so wait_for's Ref guard is dropped before the
        // match arms re-borrow `rx` (otherwise E0502 on the scrutinee).
        let received = tokio::time::timeout(self.timeout, rx.wait_for(|d| d.is_some()))
            .await
            .map(|res| res.is_ok());
        let decision = match received {
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
        };
        // The decision is delivered exactly once: drop the entry now so the
        // pending map stays bounded (a late respond then gets a clean "unknown
        // request" error instead of silently overriding what the tool acted on).
        self.prune_consumed(request_id);
        decision
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
        // `false` = never prune: entries stay readable until their tool loop
        // consumes them (same contract as `cancel_session`).
        self.pending.prune_where(|_, req| {
            if !req.finished.swap(true, Ordering::AcqRel) {
                let _ = req.decision.send(Some(PermissionDecision::Deny {
                    reason: "shutdown".into(),
                }));
            }
            false
        });
    }

    /// Entries currently registered: awaiting a decision, or settled and still
    /// waiting for their tool loop to consume the decision. (test/observability)
    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Rebuild the pending table with a short TTL so a test can exercise the
    /// registry's expiry sweep without waiting 300s.
    #[cfg(test)]
    fn with_pending_ttl(mut self, ttl: Duration) -> Self {
        self.pending = Arc::new(PendingRegistry::new(Some(ttl)));
        self
    }

    /// Remove the entry a waiter just consumed (exactly-once delivery).
    fn prune_consumed(&self, request_id: &PermissionRequestId) {
        self.pending
            .prune_where(|key, req| key == request_id && req._keepalive.borrow().is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::*;
    use crate::session_allow::RuleFingerprint;
    use agentj_core::module::{Module, ToolSource};
    use agentj_core::permission::{PermissionContext, PermissionDecision, PermissionRequestId};
    use agentj_protocol::permission::PermissionRequestParams;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
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
            // Real bash schema field (BashTool input_schema); "cmd" is legacy.
            args: serde_json::json!({"command": cmd}),
            workspace_root: PathBuf::from("/ws"),
        }
    }

    fn bash_deny_policy() -> std::collections::HashMap<String, ToolPolicy> {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "bash".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![CommandRule {
                    effect: PermissionEffect::Deny,
                    prefix: "rm -rf".into(),
                }],
            },
        );
        policies
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

    /// Regression (0.29.12): a decision that arrives BEFORE the waiter
    /// subscribes must still be delivered. `respond` used to remove the entry,
    /// so the awaiting tool loop looked up a missing id and denied the call with
    /// the misleading "request not found" even though the user had allowed it.
    #[tokio::test]
    async fn approval_survives_a_respond_that_arrives_before_the_await() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c) else {
            panic!("expected Ask");
        };
        assert!(m.respond(&request_id, ApprovalDecision::Allow));
        assert_eq!(
            m.await_approval(&request_id).await,
            PermissionDecision::Allow
        );
    }

    /// Regression (0.29.12): a settled disconnect must reach the waiter as the
    /// disconnect reason — the awaiting tool loop used to find no entry at all
    /// and report "request not found", hiding what actually happened.
    #[tokio::test]
    async fn a_settled_disconnect_still_tells_the_waiter_why() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c) else {
            panic!("expected Ask");
        };
        assert_eq!(m.cancel_session("s1", "disconnected"), 1);
        assert_eq!(
            m.await_approval(&request_id).await,
            PermissionDecision::Deny {
                reason: "disconnected".into()
            }
        );
    }

    /// A second decision on an already-settled request is refused (the client
    /// sees an error instead of silently overriding the first answer).
    #[tokio::test]
    async fn respond_refuses_a_second_decision_for_the_same_request() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c) else {
            panic!("expected Ask");
        };
        assert!(m.respond(&request_id, ApprovalDecision::Allow));
        assert!(!m.respond(&request_id, ApprovalDecision::Deny));
        assert_eq!(
            m.await_approval(&request_id).await,
            PermissionDecision::Allow
        );
    }

    /// Settled requests are consumed by their waiter; anything nobody ever
    /// consumed (late respond after the timeout, cancelled run) is pruned so the
    /// pending map cannot grow without bound.
    #[tokio::test]
    async fn stale_pending_entries_are_pruned() {
        let m = module();
        let c = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c) else {
            panic!("expected Ask");
        };
        assert_eq!(m.pending_len(), 1, "registered while awaiting");
        assert!(m.respond(&request_id, ApprovalDecision::Allow));
        assert_eq!(m.pending_len(), 1, "kept until the waiter consumes it");
        assert_eq!(
            m.await_approval(&request_id).await,
            PermissionDecision::Allow
        );
        assert_eq!(m.pending_len(), 0, "consumed by the waiter");

        let c2 = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c2) else {
            panic!("expected Ask");
        };
        assert!(m.respond(&request_id, ApprovalDecision::Allow));

        // TTL sweep: the registry drops a settled-but-unconsumed entry on the
        // next registration (a 1ms TTL keeps the test instantaneous).
        let m = module().with_pending_ttl(Duration::from_millis(1));
        let c3 = ctx(&m, "curl http://example.com");
        let PermissionDecision::Ask { request_id } = m.check(&c3) else {
            panic!("expected Ask");
        };
        assert!(m.respond(&request_id, ApprovalDecision::Allow));
        assert_eq!(m.pending_len(), 1);
        std::thread::sleep(Duration::from_millis(5));
        let c4 = ctx(&m, "curl http://example.com");
        assert!(matches!(m.check(&c4), PermissionDecision::Ask { .. }));
        assert_eq!(m.pending_len(), 1, "the stale entry was swept on insert");
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

    /// H1 regression: the REAL bash schema field is "command" (BashTool
    /// input_schema). A deny command rule must bind on it — a model-scripted
    /// `{"command": "rm -rf /x"}` ToolCall must be Denied, never slip through
    /// as an empty command that matches no rule.
    #[test]
    fn bash_command_field_is_checked() {
        let m = PermissionModule::new(bash_deny_policy(), Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "rm -rf /x"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// H1 regression: an allow_always grant must be scoped to the EXACT
    /// "command" argument — no session-wide bash wildcard collapse. The grant
    /// fingerprint derives from the stored args, so if the engine read only
    /// "cmd" (legacy), the fingerprint would be "" and every bash command in
    /// the session would be allowed.
    #[tokio::test]
    async fn allow_always_scoped_to_exact_command() {
        let m = PermissionModule::new(bash_deny_policy(), Duration::from_millis(50));
        let c = ctx(&m, "npm test");
        if let PermissionDecision::Ask { request_id } = m.check(&c) {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
            // The same "command" arg in the same session: allowed via grant.
            assert_eq!(m.check(&c), PermissionDecision::Allow);
            // A DIFFERENT command must NOT hit the grant — deny rule binds.
            let c2 = ctx(&m, "rm -rf /x");
            assert_eq!(
                m.check(&c2),
                PermissionDecision::Deny {
                    reason: "matched deny rule".into()
                }
            );
        } else {
            panic!("expected Ask");
        }
    }

    /// BUG-002 regression: allow_always for grep must scope to BOTH `pattern`
    /// and `include` — a different search term or file scope must re-Ask, not
    /// silently hit the grant (previously `rule_key_for` read only `path`,
    /// which grep lacks → fingerprint collapsed to "" → session wildcard).
    #[tokio::test]
    async fn allow_always_scoped_to_grep_pattern_and_include() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "grep".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c1 = PermissionContext {
            session_id: "s1".into(),
            tool: "grep".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": "TODO"}),
            workspace_root: PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = m.check(&c1) {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
            // Same invocation: grant hit.
            assert_eq!(m.check(&c1), PermissionDecision::Allow);
            // Different pattern: must re-Ask (no wildcard collapse).
            let c2 = PermissionContext {
                session_id: "s1".into(),
                tool: "grep".into(),
                source: ToolSource::Builtin,
                args: serde_json::json!({"pattern": "SECRET_PASSWORD"}),
                workspace_root: PathBuf::from("/ws"),
            };
            assert!(matches!(m.check(&c2), PermissionDecision::Ask { .. }));
            // Same pattern, different include (file scope): must re-Ask.
            let c3 = PermissionContext {
                session_id: "s1".into(),
                tool: "grep".into(),
                source: ToolSource::Builtin,
                args: serde_json::json!({"pattern": "TODO", "include": "*.rs"}),
                workspace_root: PathBuf::from("/ws"),
            };
            assert!(matches!(m.check(&c3), PermissionDecision::Ask { .. }));
        } else {
            panic!("expected Ask");
        }
    }

    /// BUG-002 regression: allow_always for glob must scope to its `pattern`
    /// arg — a different glob must re-Ask (previously collapsed to "").
    #[tokio::test]
    async fn allow_always_scoped_to_glob_pattern() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "glob".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c1 = PermissionContext {
            session_id: "s1".into(),
            tool: "glob".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": "src/**"}),
            workspace_root: PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = m.check(&c1) {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
            assert_eq!(m.check(&c1), PermissionDecision::Allow);
            let c2 = PermissionContext {
                session_id: "s1".into(),
                tool: "glob".into(),
                source: ToolSource::Builtin,
                args: serde_json::json!({"pattern": "docs/**"}),
                workspace_root: PathBuf::from("/ws"),
            };
            assert!(matches!(m.check(&c2), PermissionDecision::Ask { .. }));
        } else {
            panic!("expected Ask");
        }
    }

    /// BUG-002 regression: read/write still bind the fingerprint on `path`
    /// (unchanged behavior) — allow_always on one path must not allow another.
    #[tokio::test]
    async fn allow_always_scoped_to_read_write_path() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "read".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c1 = PermissionContext {
            session_id: "s1".into(),
            tool: "read".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"path": "a.txt"}),
            workspace_root: PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = m.check(&c1) {
            let handle = tokio::spawn({
                let m = m.clone();
                let rid = request_id.clone();
                async move { m.await_approval(&rid).await }
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            m.respond(&request_id, ApprovalDecision::AllowAlways);
            assert_eq!(handle.await.unwrap(), PermissionDecision::Allow);
            assert_eq!(m.check(&c1), PermissionDecision::Allow);
            let c2 = PermissionContext {
                session_id: "s1".into(),
                tool: "read".into(),
                source: ToolSource::Builtin,
                args: serde_json::json!({"path": "b.txt"}),
                workspace_root: PathBuf::from("/ws"),
            };
            assert!(matches!(m.check(&c2), PermissionDecision::Ask { .. }));
        } else {
            panic!("expected Ask");
        }
    }

    /// Backward compat: the legacy "cmd" field must still be honored.
    #[test]
    fn legacy_cmd_field_still_denied() {
        let m = PermissionModule::new(bash_deny_policy(), Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"cmd": "rm -rf /x"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// M2 regression: an MCP/plugin-sourced tool with a deny path rule must
    /// short-circuit to Deny (Deny > Allow > Ask holds for external tools too,
    /// not only builtins).
    #[test]
    fn external_tool_deny_path_rule_short_circuits() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "mcp-fetch".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "mcp-fetch".into(),
            source: ToolSource::Mcp {
                server_id: "srv".into(),
            },
            args: serde_json::json!({"path": ".env"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// M3 regression: grep's `include` arg is the path-semantic key (it maps to
    /// `rg --glob` file scope), so a deny rule for `.env` must bind on `include`
    /// — NOT on `pattern` (the regex search term). This test exercises the
    /// `include` trigger explicitly with a non-path `pattern`.
    #[test]
    fn grep_include_arg_binds_path_deny_rule() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "grep".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "grep".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": ".*", "include": ".env"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// BUG-001 regression: grep's `pattern` is a REGEX search term (see
    /// GrepTool — it is passed verbatim to `rg <pattern>`), NOT a file path.
    /// A `grep {"pattern": ".env"}` with no `include` must NOT bind the `.env`
    /// path deny rule — it falls back to the tool's default mode (Ask).
    #[test]
    fn grep_pattern_is_regex_not_a_path_for_rules() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "grep".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "grep".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": ".env"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert!(matches!(m.check(&c), PermissionDecision::Ask { .. }));
    }

    /// read/write regression: `path` remains the path-semantic key for file
    /// tools — a deny rule for `.env` must bind on write's `path` arg.
    #[test]
    fn write_path_arg_binds_path_deny_rule() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "write".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "write".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"path": ".env", "content": "x"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// M3 regression: glob's "pattern" arg is a workspace-relative glob and
    /// must be evaluated against path rules too.
    #[test]
    fn glob_pattern_arg_binds_path_deny_rule() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "glob".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "glob".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": ".env"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(
            m.check(&c),
            PermissionDecision::Deny {
                reason: "matched deny rule".into()
            }
        );
    }

    /// M3 regression: glob's `pattern` keeps path semantics (unlike grep's
    /// regex `pattern`) — an allow rule `src/**` must bind on it.
    #[test]
    fn glob_pattern_arg_binds_path_allow_rule() {
        let mut policies = std::collections::HashMap::new();
        policies.insert(
            "glob".to_string(),
            ToolPolicy {
                mode: PermissionMode::Ask,
                path_rules: vec![PathRule {
                    effect: PermissionEffect::Allow,
                    pattern: "src/**".into(),
                }],
                command_rules: vec![],
            },
        );
        let m = PermissionModule::new(policies, Duration::from_millis(50));
        let c = PermissionContext {
            session_id: "s1".into(),
            tool: "glob".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"pattern": "src/**"}),
            workspace_root: PathBuf::from("/ws"),
        };
        assert_eq!(m.check(&c), PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn session_of_peeks_pending_request_session() {
        let module = PermissionModule::new(HashMap::new(), Duration::from_secs(1));
        let ctx = PermissionContext {
            session_id: "s-1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "ls"}),
            workspace_root: PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
            assert_eq!(module.session_of(&request_id).as_deref(), Some("s-1"));
            assert!(module.respond(&request_id, ApprovalDecision::Allow));
            assert_eq!(
                module.session_of(&request_id).as_deref(),
                Some("s-1"),
                "a settled entry stays readable until its waiter consumes it"
            );
            module.await_approval(&request_id).await;
            assert_eq!(
                module.session_of(&request_id),
                None,
                "the waiter consumes the entry"
            );
        } else {
            panic!("expected Ask");
        }
    }
    /// The table is shared with `PendingRegistry`: a settled entry is still
    /// readable by its waiter, and consumption is exactly-once.
    #[test]
    fn settled_entries_are_pruned_only_after_the_waiter_consumes() {
        let m = module();
        let id = pending_id(&m, "s1", "rm -rf /");
        assert!(m.respond(&id, ApprovalDecision::Allow));
        assert_eq!(m.pending_len(), 1, "still readable by the waiter");
        m.prune_consumed(&id);
        assert_eq!(m.pending_len(), 0);
        m.prune_consumed(&id);
        assert_eq!(m.pending_len(), 0, "consumption is idempotent");
    }

    #[test]
    fn cancel_session_keeps_the_entry_for_the_waiter() {
        let m = module();
        let id = pending_id(&m, "s1", "rm -rf /");
        assert_eq!(m.cancel_session("s1", "disconnected"), 1);
        assert_eq!(m.pending_len(), 1);
        m.prune_consumed(&id);
        assert_eq!(m.pending_len(), 0);
    }

    /// A disconnect Deny must win against a later user Allow: once the entry is
    /// claimed, a second settler is rejected instead of overwriting it.
    #[test]
    fn respond_after_cancel_session_is_rejected() {
        let m = module();
        let id = pending_id(&m, "s1", "curl http://example.com");
        assert_eq!(m.cancel_session("s1", "disconnected"), 1);
        assert!(
            !m.respond(&id, ApprovalDecision::Allow),
            "a later Allow must not overwrite the disconnect Deny"
        );
        assert_eq!(m.cancel_session("s1", "disconnected"), 0, "settled once");
    }

    /// Single winner under concurrency: the entry's one-shot claim flag admits
    /// exactly one of N racing `respond` calls, so the delivered decision is
    /// never a coin flip between two clients.
    #[test]
    fn concurrent_respond_calls_have_exactly_one_winner() {
        const THREADS: usize = 8;
        const ROUNDS: usize = 32;
        for round in 0..ROUNDS {
            let m = module();
            let id = pending_id(&m, "s1", "curl http://example.com");
            let winners = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(Barrier::new(THREADS));
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let m = m.clone();
                    let id = id.clone();
                    let winners = Arc::clone(&winners);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        if m.respond(&id, ApprovalDecision::Allow) {
                            winners.fetch_add(1, Ordering::AcqRel);
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
            assert_eq!(
                winners.load(Ordering::Acquire),
                1,
                "round {round}: exactly one responder may win"
            );
        }
    }

    /// `respond` racing `cancel_session` for one request: both go through the
    /// same claim flag, so exactly one of them settles it.
    #[test]
    fn cancel_session_and_respond_cannot_both_settle() {
        const ROUNDS: usize = 32;
        for round in 0..ROUNDS {
            let m = module();
            let id = pending_id(&m, "s1", "curl http://example.com");
            let barrier = Arc::new(Barrier::new(2));
            let responder = {
                let m = m.clone();
                let id = id.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    m.respond(&id, ApprovalDecision::Allow)
                })
            };
            barrier.wait();
            let cancelled = m.cancel_session("s1", "disconnected");
            let allowed = responder.join().unwrap();
            assert_eq!(
                usize::from(allowed) + cancelled,
                1,
                "round {round}: exactly one settler may claim the request"
            );
        }
    }

    fn pending_id(m: &PermissionModule, session: &str, cmd: &str) -> PermissionRequestId {
        let mut c = ctx(m, cmd);
        c.session_id = session.to_string();
        match m.eval(&c) {
            PermissionDecision::Ask { request_id } => request_id,
            other => panic!("expected Ask, got {other:?}"),
        }
    }

    #[test]
    fn session_of_unknown_request_is_none() {
        let module = PermissionModule::new(HashMap::new(), Duration::from_secs(1));
        assert_eq!(module.session_of(&PermissionRequestId("nope".into())), None);
    }
    #[test]
    fn register_ask_notifier_receives_session_id() {
        let module = PermissionModule::new(HashMap::new(), Duration::from_secs(1));
        let seen = Arc::new(Mutex::new(None::<String>));
        let seen2 = Arc::clone(&seen);
        module.set_notifier(Arc::new(move |p: &PermissionRequestParams, _sid: &str| {
            *lock_or_recover(&seen2) = p.session_id.clone();
        }));
        let ctx = PermissionContext {
            session_id: "s-9".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "ls"}),
            workspace_root: PathBuf::from("/ws"),
        };
        module.check(&ctx);
        assert_eq!(lock_or_recover(&seen).as_deref(), Some("s-9"));
    }

    #[tokio::test]
    async fn view_peeks_without_consuming() {
        let module = PermissionModule::new(HashMap::new(), Duration::from_secs(1));
        let ctx = PermissionContext {
            session_id: "s-1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"command": "ls"}),
            workspace_root: PathBuf::from("/ws"),
        };
        if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
            let view = module.view(&request_id).expect("pending entry is visible");
            assert_eq!(view.tool, "bash");
            assert_eq!(view.session_id, "s-1");
            assert!(
                module.respond(&request_id, ApprovalDecision::Allow),
                "a view peek must not consume the entry"
            );
            assert_eq!(
                module.view(&request_id).map(|v| v.tool),
                Some("bash".to_string()),
                "a settled entry stays readable until its waiter consumes it"
            );
            module.await_approval(&request_id).await;
            assert_eq!(
                module.view(&request_id),
                None,
                "the waiter consumed the entry"
            );
            assert_eq!(module.view("nope"), None);
        } else {
            panic!("expected Ask");
        }
    }
}
