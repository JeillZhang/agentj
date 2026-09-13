//! AgentJ question channel (design 决策 #6): a blocking ask → owner response
//! mechanism that mirrors `agentj-permission`'s pending/notifier pattern but is
//! semantically independent.
//!
//! The module owns the question state machine: a pending table keyed by
//! `question_id` (each entry carries the `std::sync::mpsc::Sender` used to
//! unblock the asker), a settled table recording terminal outcomes, an
//! owner-delivery notifier (`$/askQuestion`) and a settle hook (trajectory
//! recording) fired at most once per question.
//!
//! `AskTool` (T19) is added to `tool.rs` by a downstream task.
pub mod module;

pub use module::{
    QuestionModule, QuestionNotifier, QuestionOutcome, QuestionSettlement, RespondError,
    RespondOutcome, SettleHook,
};
