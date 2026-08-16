//! AgentJ permission management (design 决策 #17). Leaf crate: decision engine,
//! policy model, session allow-state. Implements agentj-core's PermissionChecker.
//! Modules engine/session_allow/module are added incrementally by T3/T4/T5.
pub mod error;
pub mod policy;

pub use error::PermissionError;
pub use policy::{builtin_default_mode, CommandRule, PathRule, PermissionEffect, PermissionMode, ToolPolicy};
