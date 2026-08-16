//! Permission capability interface (design 决策 #17, §7 capability-interface /
//! channel separation). The trait lives in agentj-core — the same crate that
//! holds KernelContext — so the kernel can hold Arc<dyn PermissionChecker>
//! without a dependency cycle. agentj-permission implements it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::module::ToolSource;
use crate::types::SessionId;

/// Opaque approval-request id (uuid), decoupled from the JSON-RPC request id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PermissionRequestId(pub String);

/// Decision result of a permission check.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow,
    Deny {
        reason: String,
    },
    /// Approval pending — the implementation has registered a pending
    /// request, sent the $/permissionRequest notification, and the caller
    /// must now await_approval().
    Ask {
        request_id: PermissionRequestId,
    },
}

/// Everything the permission engine needs to evaluate a tool invocation.
#[derive(Debug, Clone)]
pub struct PermissionContext {
    pub session_id: SessionId,
    pub tool: String,
    pub source: ToolSource,
    pub args: serde_json::Value,
    pub workspace_root: PathBuf,
}

/// Capability interface consumed by ToolRegistry::execute_async (design §7).
pub trait PermissionChecker: Send + Sync {
    /// Synchronous decision: Allow / Deny / Ask. Ask means the implementation
    /// registered a pending request and emitted the $/permissionRequest
    /// notification; the caller must follow with await_approval().
    fn check(&self, ctx: &PermissionContext) -> PermissionDecision;

    /// Await the Ask decision (async; internally awaits the IPC response
    /// channel; timeout/cancel → Deny). Only called after check() returns Ask.
    fn await_approval(
        &self,
        request_id: &PermissionRequestId,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ToolSource;
    use std::path::PathBuf;

    struct MockChecker;
    impl PermissionChecker for MockChecker {
        fn check(&self, _ctx: &PermissionContext) -> PermissionDecision {
            PermissionDecision::Ask {
                request_id: PermissionRequestId("req-1".into()),
            }
        }
        async fn await_approval(&self, _request_id: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    #[test]
    fn permission_context_builds() {
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({"cmd": "ls"}),
            workspace_root: PathBuf::from("/tmp/ws"),
        };
        assert_eq!(ctx.tool, "bash");
        assert_eq!(ctx.workspace_root, PathBuf::from("/tmp/ws"));
    }

    #[test]
    fn decision_ask_carries_request_id() {
        let d = PermissionDecision::Ask {
            request_id: PermissionRequestId("abc".into()),
        };
        match d {
            PermissionDecision::Ask { request_id } => {
                assert_eq!(request_id.0, "abc");
            }
            _ => panic!("expected Ask"),
        }
    }

    #[tokio::test]
    async fn mock_checker_ask_and_approval() {
        let c = MockChecker;
        let ctx = PermissionContext {
            session_id: "s1".into(),
            tool: "bash".into(),
            source: ToolSource::Builtin,
            args: serde_json::json!({}),
            workspace_root: PathBuf::from("."),
        };
        let d = c.check(&ctx);
        assert!(matches!(&d, PermissionDecision::Ask { .. }));
        if let PermissionDecision::Ask { request_id } = d {
            assert!(matches!(
                c.await_approval(&request_id).await,
                PermissionDecision::Allow
            ));
        }
    }
}
