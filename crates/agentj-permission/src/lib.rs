//! AgentJ permission management (design 决策 #17). Leaf crate: decision engine,
//! policy model, session allow-state. Implements agentj-core's PermissionChecker.
//! Modules engine/session_allow/module are added incrementally by T3/T4/T5.
pub mod engine;
pub mod error;
pub mod module;
pub mod policy;
pub mod session_allow;

pub use engine::PermissionEngine;
pub use error::PermissionError;
pub use module::{ApprovalDecision, NO_APPROVER_REASON, PermissionModule};
pub use policy::{
    CommandRule, PathRule, PermissionConfig, PermissionEffect, PermissionMode, ToolPolicy,
    builtin_default_mode,
};
pub use session_allow::SessionAllowState;
