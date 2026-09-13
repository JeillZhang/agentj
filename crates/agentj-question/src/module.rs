//! QuestionModule — pending/settled tables, blocking ask, idempotent respond,
//! owner-delivery notifier and exactly-once settle hook (design 决策 #6).
//!
//! Mirrors `agentj-permission`'s pending/notifier mechanism, but the semantics
//! are question-specific: `timeout_s` is optional (absent = wait forever),
//! duplicate/unknown responses are idempotent `accepted=false` (not errors),
//! and every terminal path funnels through [`QuestionModule::settle_once`].
//!
//! Channel design: each pending question carries the `std::sync::mpsc::Sender`
//! the blocking asker waits on. Removing the pending entry is the single-winner
//! arbiter for respond/timeout/cancel races; the winner always delivers exactly
//! one outcome on the channel before the sender is dropped, and fires the
//! settle hook exactly once.

use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use agentj_core::error::ModuleError;
use agentj_core::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister,
};
use agentj_core::types::ModuleId;
use agentj_protocol::question::{AskOption, AskQuestionParams};
use uuid::Uuid;

/// Free-text answer prefix; ids carrying it are always accepted.
const OTHER_PREFIX: &str = "other:";

/// Terminal outcome of a blocking ask: the owner answered, or the question was
/// settled without an answer (timeout / cancel / owner unavailable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionOutcome {
    Answered { answers: Vec<String> },
    Skipped,
}

impl QuestionOutcome {
    /// Answers for `Answered`; empty for `Skipped`.
    pub fn answers(&self) -> &[String] {
        match self {
            QuestionOutcome::Answered { answers } => answers,
            QuestionOutcome::Skipped => &[],
        }
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, QuestionOutcome::Skipped)
    }
}

/// Result of `question/respond` — `Accepted(false)` is the idempotent answer
/// for duplicate, already-settled or unknown question ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespondOutcome {
    Accepted(bool),
}

/// A `question/respond` request was recognized but malformed. The IPC layer
/// maps these to `-32602` (with `error.data`), never to a silent rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespondError {
    /// Answer ids outside the pending options and not `other:`-prefixed.
    UnknownOptions { unknown: Vec<String> },
    /// More than one answer for a single-choice question.
    TooManyAnswers,
}

impl fmt::Display for RespondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RespondError::UnknownOptions { unknown } => {
                write!(f, "unknown option ids: {}", unknown.join(", "))
            }
            RespondError::TooManyAnswers => {
                write!(f, "multiple answers for a single-choice question")
            }
        }
    }
}

impl std::error::Error for RespondError {}

/// Terminal record delivered to the settle hook exactly once per question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSettlement {
    pub question_id: String,
    pub session_id: String,
    pub outcome: QuestionOutcome,
}

/// Owner-delivery sink (`$/askQuestion`). Returns `true` when the owner
/// connection received the notification; `false` (or no notifier) settles the
/// question as skipped immediately.
pub type QuestionNotifier = Arc<dyn Fn(&AskQuestionParams, &str) -> bool + Send + Sync>;

/// Settlement sink. Args: `(session_id, question_id, settlement)`; the
/// settlement carries both ids as well. Panics are isolated.
pub type SettleHook = Arc<dyn Fn(&str, &str, &QuestionSettlement) + Send + Sync>;

/// Pending entry: the blocking asker waits on `sender`.
#[derive(Clone)]
struct PendingQuestion {
    sender: mpsc::Sender<QuestionOutcome>,
    session_id: String,
    options: Vec<AskOption>,
    allow_multiple: bool,
}

/// Terminal record kept for idempotency and per-session cleanup.
struct SettledQuestion {
    session_id: String,
    outcome: QuestionOutcome,
}

pub struct QuestionModule {
    pending: Mutex<HashMap<String, PendingQuestion>>,
    settled: Mutex<HashMap<String, SettledQuestion>>,
    notifier: RwLock<Option<QuestionNotifier>>,
    settle_hook: RwLock<Option<SettleHook>>,
}

impl Default for QuestionModule {
    fn default() -> Self {
        Self::new()
    }
}

impl QuestionModule {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            settled: Mutex::new(HashMap::new()),
            notifier: RwLock::new(None),
            settle_hook: RwLock::new(None),
        }
    }

    /// Install the owner-delivery notifier (IPC layer, once at startup).
    pub fn set_notifier(&self, f: QuestionNotifier) {
        *self.notifier.write().unwrap() = Some(f);
    }

    /// Install the settle hook (IPC layer, once at startup).
    pub fn set_settle_hook(&self, f: SettleHook) {
        *self.settle_hook.write().unwrap() = Some(f);
    }

    /// Register a question and block until it settles. Generates the
    /// `question_id` (overwriting `params.question_id`), delivers `params` to
    /// the owner notifier, then waits on an mpsc channel — bounded by
    /// `params.timeout_s` when present, unbounded otherwise. An undelivered
    /// question (no notifier / no owner) settles as skipped immediately.
    pub fn ask_blocking(
        &self,
        session_id: &str,
        params: &AskQuestionParams,
    ) -> (String, QuestionOutcome) {
        let question_id = Uuid::new_v4().to_string();
        let mut wire = params.clone();
        wire.question_id = question_id.clone();
        let (sender, receiver) = mpsc::channel();
        self.pending.lock().unwrap().insert(
            question_id.clone(),
            PendingQuestion {
                sender,
                session_id: session_id.to_string(),
                options: params.options.clone(),
                allow_multiple: params.allow_multiple,
            },
        );
        if !self.deliver(&wire, session_id) {
            self.settle_skipped(&question_id, session_id);
            return (question_id, QuestionOutcome::Skipped);
        }
        let outcome = self.wait_outcome(&question_id, session_id, &receiver, params.timeout_s);
        (question_id, outcome)
    }

    /// Deliver the owner's answer. The pending entry is the single-winner
    /// arbiter, so exactly one of respond/timeout/cancel settles a question;
    /// duplicate, already-settled and unknown ids all return
    /// `Accepted(false)` without error.
    pub fn respond(
        &self,
        question_id: &str,
        answers: Vec<String>,
        skipped: bool,
    ) -> Result<RespondOutcome, RespondError> {
        let Some(pending) = self.pending.lock().unwrap().get(question_id).cloned() else {
            return Ok(RespondOutcome::Accepted(false));
        };
        if !skipped {
            validate_answers(&pending.options, pending.allow_multiple, &answers)?;
        }
        let outcome = if skipped {
            QuestionOutcome::Skipped
        } else {
            QuestionOutcome::Answered { answers }
        };
        let settlement = QuestionSettlement {
            question_id: question_id.to_string(),
            session_id: pending.session_id,
            outcome,
        };
        if self.settle_once(question_id, &settlement) {
            Ok(RespondOutcome::Accepted(true))
        } else {
            Ok(RespondOutcome::Accepted(false))
        }
    }

    /// Session owning a PENDING question. Settled or unknown ids return `None`
    /// so callers can answer `accepted:false` without an owner check.
    pub fn session_of(&self, question_id: &str) -> Option<String> {
        self.pending
            .lock()
            .unwrap()
            .get(question_id)
            .map(|q| q.session_id.clone())
    }

    /// Settle every pending question of a session as Skipped (run cancel /
    /// owner disconnect) and drop the session's settled records (决策 #4).
    /// Returns how many pending questions were settled.
    pub fn cancel_session(&self, session_id: &str) -> usize {
        let ids: Vec<String> = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, q)| q.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        let settled = ids
            .iter()
            .filter(|id| self.settle_skipped(id, session_id))
            .count();
        self.settled
            .lock()
            .unwrap()
            .retain(|_, s| s.session_id != session_id);
        settled
    }

    /// Settle every pending question (module shutdown).
    pub fn clear_all(&self) -> usize {
        let ids: Vec<(String, String)> = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .map(|(id, q)| (id.clone(), q.session_id.clone()))
            .collect();
        ids.iter()
            .filter(|(id, session_id)| self.settle_skipped(id, session_id))
            .count()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Terminal outcome of an already-settled question (diagnostics; `None`
    /// for pending/unknown ids).
    pub fn settled_of(&self, question_id: &str) -> Option<QuestionOutcome> {
        self.settled
            .lock()
            .unwrap()
            .get(question_id)
            .map(|s| s.outcome.clone())
    }

    /// Single-winner settlement: removing the pending entry is the arbiter.
    /// The winner records the terminal state, fires the settle hook exactly
    /// once and delivers the outcome to the blocked asker.
    fn settle_once(&self, question_id: &str, settlement: &QuestionSettlement) -> bool {
        let Some(entry) = self.pending.lock().unwrap().remove(question_id) else {
            return false;
        };
        self.settled.lock().unwrap().insert(
            question_id.to_string(),
            SettledQuestion {
                session_id: settlement.session_id.clone(),
                outcome: settlement.outcome.clone(),
            },
        );
        self.fire_settle_hook(settlement);
        let _ = entry.sender.send(settlement.outcome.clone());
        true
    }

    fn deliver(&self, params: &AskQuestionParams, session_id: &str) -> bool {
        let notifier = self.notifier.read().unwrap().clone();
        match notifier {
            Some(f) => f(params, session_id),
            None => false,
        }
    }

    fn settle_skipped(&self, question_id: &str, session_id: &str) -> bool {
        let settlement = QuestionSettlement {
            question_id: question_id.to_string(),
            session_id: session_id.to_string(),
            outcome: QuestionOutcome::Skipped,
        };
        self.settle_once(question_id, &settlement)
    }

    fn wait_outcome(
        &self,
        question_id: &str,
        session_id: &str,
        receiver: &mpsc::Receiver<QuestionOutcome>,
        timeout_s: Option<u64>,
    ) -> QuestionOutcome {
        match timeout_s {
            None => receiver.recv().unwrap_or(QuestionOutcome::Skipped),
            Some(secs) => match receiver.recv_timeout(Duration::from_secs(secs)) {
                Ok(outcome) => outcome,
                Err(RecvTimeoutError::Timeout) => {
                    self.settle_timeout(question_id, session_id, receiver)
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.settle_skipped(question_id, session_id);
                    QuestionOutcome::Skipped
                }
            },
        }
    }

    /// Timeout race: if this caller wins the pending removal it returns
    /// Skipped; otherwise respond/cancel won and is guaranteed to deliver on
    /// the channel, so wait for that outcome instead of reporting a stale
    /// timeout.
    fn settle_timeout(
        &self,
        question_id: &str,
        session_id: &str,
        receiver: &mpsc::Receiver<QuestionOutcome>,
    ) -> QuestionOutcome {
        if self.settle_skipped(question_id, session_id) {
            return QuestionOutcome::Skipped;
        }
        receiver.recv().unwrap_or(QuestionOutcome::Skipped)
    }

    /// Fire the settle hook with panic isolation: a panicking callback must
    /// not propagate into ask_blocking/respond nor poison the module.
    fn fire_settle_hook(&self, settlement: &QuestionSettlement) {
        let hook = self.settle_hook.read().unwrap().clone();
        if let Some(f) = hook {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                f(&settlement.session_id, &settlement.question_id, settlement);
            }));
        }
    }
}

/// Answer validation: ids must reference a pending option or carry the
/// `other:` free-text prefix; single-choice questions accept at most one.
fn validate_answers(
    options: &[AskOption],
    allow_multiple: bool,
    answers: &[String],
) -> Result<(), RespondError> {
    let unknown: Vec<String> = answers
        .iter()
        .filter(|answer| {
            !answer.starts_with(OTHER_PREFIX) && !options.iter().any(|o| o.id == answer.as_str())
        })
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(RespondError::UnknownOptions { unknown });
    }
    if !allow_multiple && answers.len() > 1 {
        return Err(RespondError::TooManyAnswers);
    }
    Ok(())
}

impl Module for QuestionModule {
    fn id(&self) -> ModuleId {
        "question".into()
    }

    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "question".into(),
            name: "Question".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Model question channel (blocking ask / owner respond)".into(),
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

/// Participation is lifecycle-only (mirrors `PermissionModule`): no bus
/// messages are routed, so `handle` is a no-op.
impl KernelModule for QuestionModule {
    fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        Ok(KernelResponse::Handled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_protocol::question::AskOption;
    use std::time::Duration;

    fn params_with_options() -> AskQuestionParams {
        AskQuestionParams {
            question_id: String::new(), // ask_blocking supplies the real id
            session_id: "s1".into(),
            question: "Pick a letter".into(),
            header: None,
            options: vec![
                AskOption {
                    id: "a".into(),
                    label: "A".into(),
                    hint: None,
                },
                AskOption {
                    id: "b".into(),
                    label: "B".into(),
                    hint: None,
                },
            ],
            allow_multiple: false,
            allow_other: true,
            timeout_s: None,
        }
    }

    /// Busy-wait until ask_blocking registered its pending entry, then return
    /// the generated question id.
    fn wait_for_pending(m: &QuestionModule) -> String {
        for _ in 0..500 {
            if let Ok(pending) = m.pending.lock()
                && let Some(id) = pending.keys().next()
            {
                return id.clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no pending question registered within 5s");
    }

    fn delivering_module() -> Arc<QuestionModule> {
        let m = Arc::new(QuestionModule::new());
        m.set_notifier(Arc::new(|_p: &AskQuestionParams, _sid: &str| true));
        m
    }

    #[test]
    fn ask_and_respond_roundtrip() {
        let m = delivering_module();
        let m2 = Arc::clone(&m);
        let h = std::thread::spawn(move || m2.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        assert_eq!(
            m.respond(&qid, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(true)
        );
        let (settled_id, outcome) = h.join().unwrap();
        assert_eq!(settled_id, qid);
        assert_eq!(
            outcome,
            QuestionOutcome::Answered {
                answers: vec!["a".into()]
            }
        );
    }

    #[test]
    fn duplicate_respond_is_idempotent_false() {
        let m = delivering_module();
        let m2 = Arc::clone(&m);
        let h = std::thread::spawn(move || m2.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        assert_eq!(
            m.respond(&qid, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(true)
        );
        // Already settled: no error, no re-delivery.
        assert_eq!(
            m.respond(&qid, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(false)
        );
        // Unknown id: same idempotent false.
        assert_eq!(
            m.respond("no-such-question", vec!["a".into()], false)
                .unwrap(),
            RespondOutcome::Accepted(false)
        );
        let (_, outcome) = h.join().unwrap();
        assert_eq!(
            outcome,
            QuestionOutcome::Answered {
                answers: vec!["a".into()]
            }
        );
    }

    #[test]
    fn respond_validates_options_and_single_choice() {
        let m = delivering_module();
        let m2 = Arc::clone(&m);
        let h = std::thread::spawn(move || m2.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        // Unknown option id (no other: prefix) is a protocol validation error.
        assert_eq!(
            m.respond(&qid, vec!["zzz".into()], false),
            Err(RespondError::UnknownOptions {
                unknown: vec!["zzz".into()]
            })
        );
        // Single-choice questions reject multiple answers.
        assert_eq!(
            m.respond(&qid, vec!["a".into(), "b".into()], false),
            Err(RespondError::TooManyAnswers)
        );
        // Validation failures leave the question pending for a valid retry.
        assert_eq!(m.pending_count(), 1);
        assert_eq!(
            m.respond(&qid, vec!["other:free text".into()], false)
                .unwrap(),
            RespondOutcome::Accepted(true)
        );
        let (_, outcome) = h.join().unwrap();
        assert_eq!(
            outcome,
            QuestionOutcome::Answered {
                answers: vec!["other:free text".into()]
            }
        );
    }

    #[test]
    fn settle_hook_fires_exactly_once_with_settlement() {
        let m = delivering_module();
        let seen = Arc::new(Mutex::new(Vec::<QuestionSettlement>::new()));
        let seen2 = Arc::clone(&seen);
        m.set_settle_hook(Arc::new(
            move |_session_id: &str, _question_id: &str, s: &QuestionSettlement| {
                seen2.lock().unwrap().push(s.clone());
            },
        ));
        let m2 = Arc::clone(&m);
        let h = std::thread::spawn(move || m2.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        assert_eq!(
            m.respond(&qid, vec!["b".into()], false).unwrap(),
            RespondOutcome::Accepted(true)
        );
        // A duplicate response must not fire the hook again.
        let _ = m.respond(&qid, vec!["b".into()], false);
        h.join().unwrap();
        let hook = seen.lock().unwrap();
        assert_eq!(hook.len(), 1, "hook must fire exactly once");
        assert_eq!(hook[0].question_id, qid);
        assert_eq!(hook[0].session_id, "s1");
        assert_eq!(
            hook[0].outcome,
            QuestionOutcome::Answered {
                answers: vec!["b".into()]
            }
        );
    }

    #[test]
    fn notifier_reports_no_owner_settles_skipped_immediately() {
        let m = QuestionModule::new();
        m.set_notifier(Arc::new(|_p: &AskQuestionParams, _sid: &str| false));
        let (qid, outcome) = m.ask_blocking("s1", &params_with_options());
        assert_eq!(outcome, QuestionOutcome::Skipped);
        assert_eq!(m.session_of(&qid), None);
        assert_eq!(m.pending_count(), 0);
    }
}
