//! QuestionModule — pending/settled tables, blocking ask, idempotent respond,
//! owner-delivery notifier and exactly-once settle hook (design 决策 #6).
//!
//! Mirrors `agentj-permission`'s pending/notifier mechanism, but the semantics
//! are question-specific: `timeout_s` is optional (absent falls back to the
//! configured default; `0`/`None` = unbounded),
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use agentj_core::error::ModuleError;
use agentj_core::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister,
};
use agentj_core::sync::{lock_or_recover, read_or_recover, write_or_recover};
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
    /// Configured default timeout for requests that omit `timeout_s`;
    /// `0` == unbounded (the library default until the CLI injects one).
    default_timeout_s: AtomicU64,
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
            default_timeout_s: AtomicU64::new(0),
        }
    }

    /// Install the owner-delivery notifier (IPC layer, once at startup).
    pub fn set_notifier(&self, f: QuestionNotifier) {
        *write_or_recover(&self.notifier) = Some(f);
    }

    /// Install the settle hook (IPC layer, once at startup).
    pub fn set_settle_hook(&self, f: SettleHook) {
        *write_or_recover(&self.settle_hook) = Some(f);
    }

    /// Set the default timeout applied when a request omits `timeout_s`.
    /// `None` (or `0`) means unbounded — `QuestionModule::new()` leaves it
    /// unset; the CLI injects `cfg.question_timeout_s()` in T15.
    pub fn set_default_timeout_s(&self, secs: Option<u64>) {
        self.default_timeout_s
            .store(secs.unwrap_or(0), Ordering::Relaxed);
    }

    /// Resolve the effective timeout: an explicit per-call `timeout_s` wins;
    /// otherwise the configured default applies (0 == unbounded).
    fn effective_timeout(&self, params: &AskQuestionParams) -> Option<u64> {
        params
            .timeout_s
            .or_else(|| match self.default_timeout_s.load(Ordering::Relaxed) {
                0 => None,
                s => Some(s),
            })
    }

    /// Register a question and block until it settles. Generates the
    /// `question_id` (overwriting `params.question_id`), delivers `params` to
    /// the owner notifier, then waits on an mpsc channel — bounded by the
    /// effective timeout (an explicit `params.timeout_s`, else the configured
    /// default; `0`/`None` = unbounded). An undelivered question (no notifier
    /// / no owner) settles as skipped immediately.
    pub fn ask_blocking(
        &self,
        session_id: &str,
        params: &AskQuestionParams,
    ) -> (String, QuestionOutcome) {
        let question_id = Uuid::new_v4().to_string();
        let mut wire = params.clone();
        wire.question_id = question_id.clone();
        let (sender, receiver) = mpsc::channel();
        lock_or_recover(&self.pending).insert(
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
        let outcome = self.wait_outcome(
            &question_id,
            session_id,
            &receiver,
            self.effective_timeout(params),
        );
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
        let Some(pending) = lock_or_recover(&self.pending).get(question_id).cloned() else {
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
        lock_or_recover(&self.pending).len()
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
        let Some(entry) = lock_or_recover(&self.pending).remove(question_id) else {
            return false;
        };
        lock_or_recover(&self.settled).insert(
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
        let notifier = read_or_recover(&self.notifier).clone();
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
        let hook = read_or_recover(&self.settle_hook).clone();
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

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

    /// Busy-wait until at least `n` pending questions are registered and return
    /// their generated ids.
    fn wait_for_pending_count(m: &QuestionModule, n: usize) -> Vec<String> {
        for _ in 0..500 {
            if let Ok(pending) = m.pending.lock()
                && pending.len() >= n
            {
                return pending.keys().cloned().collect();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("fewer than {n} pending questions registered within 5s");
    }

    fn recording_hook() -> (Arc<Mutex<Vec<QuestionSettlement>>>, SettleHook) {
        let seen = Arc::new(Mutex::new(Vec::<QuestionSettlement>::new()));
        let seen2 = Arc::clone(&seen);
        let hook: SettleHook = Arc::new(
            move |_session_id: &str, _question_id: &str, s: &QuestionSettlement| {
                lock_or_recover(&seen2).push(s.clone());
            },
        );
        (seen, hook)
    }

    fn panicking_hook() -> (Arc<AtomicUsize>, SettleHook) {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = Arc::clone(&calls);
        let hook: SettleHook = Arc::new(
            move |_session_id: &str, _question_id: &str, _s: &QuestionSettlement| {
                calls2.fetch_add(1, Ordering::SeqCst);
                panic!("settle hook panic must be isolated");
            },
        );
        (calls, hook)
    }

    fn answered(ids: &[&str]) -> QuestionOutcome {
        QuestionOutcome::Answered {
            answers: ids.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    /// Run one respond-vs-timeout race and assert the responder and the blocked
    /// asker agree on the single winner. Returns (question id, responder
    /// outcome, asker outcome).
    fn race_once(
        m: &Arc<QuestionModule>,
        delay_ms: u64,
    ) -> (String, RespondOutcome, QuestionOutcome) {
        let mut params = params_with_options();
        params.timeout_s = Some(1);
        let asker = Arc::clone(m);
        let h = std::thread::spawn(move || asker.ask_blocking("s1", &params));
        let qid = wait_for_pending(m);
        let responder = Arc::clone(m);
        let qid2 = qid.clone();
        let r = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(delay_ms));
            responder.respond(&qid2, vec!["a".into()], false)
        });
        let response = r.join().unwrap().unwrap();
        let (asked_id, outcome) = h.join().unwrap();
        assert_eq!(asked_id, qid);
        match response {
            RespondOutcome::Accepted(true) => {
                assert_eq!(
                    outcome,
                    answered(&["a"]),
                    "respond won but asker saw {outcome:?}"
                );
            }
            RespondOutcome::Accepted(false) => {
                assert_eq!(
                    outcome,
                    QuestionOutcome::Skipped,
                    "timeout won but respond was accepted"
                );
            }
        }
        assert_eq!(m.settled_of(&qid), Some(outcome.clone()));
        (qid, response, outcome)
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
                lock_or_recover(&seen2).push(s.clone());
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
        let hook = lock_or_recover(&seen);
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

    #[test]
    fn timeout_settles_skip_and_fires_hook_once() {
        let m = delivering_module();
        let (seen, hook) = recording_hook();
        m.set_settle_hook(hook);
        let mut params = params_with_options();
        params.timeout_s = Some(1);
        let started = Instant::now();
        let (qid, outcome) = m.ask_blocking("s1", &params);
        let elapsed = started.elapsed();
        assert_eq!(outcome, QuestionOutcome::Skipped);
        assert!(
            elapsed >= Duration::from_millis(900),
            "returned before the timeout: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout overshot: {elapsed:?}"
        );
        assert_eq!(m.pending_count(), 0);
        assert_eq!(m.settled_of(&qid), Some(QuestionOutcome::Skipped));
        let seen = lock_or_recover(&seen);
        assert_eq!(seen.len(), 1, "hook must fire exactly once on timeout");
        assert_eq!(seen[0].question_id, qid);
        assert_eq!(seen[0].session_id, "s1");
        assert_eq!(seen[0].outcome, QuestionOutcome::Skipped);
    }

    #[test]
    fn default_timeout_settles_a_never_answered_question() {
        let module = delivering_module();
        module.set_default_timeout_s(Some(1));
        let mut params = params_with_options();
        params.timeout_s = None;
        let started = Instant::now();
        let (qid, outcome) = module.ask_blocking("s1", &params);
        assert!(matches!(outcome, QuestionOutcome::Skipped));
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(900),
            "default 1s timeout returned before firing: {elapsed:?}"
        );
        assert!(elapsed < Duration::from_millis(3_000));
        assert_eq!(module.pending_count(), 0);
        assert_eq!(module.settled_of(&qid), Some(QuestionOutcome::Skipped));
    }

    #[test]
    fn default_timeout_deadline_tracks_the_configured_value() {
        let module = delivering_module();
        let mut params = params_with_options();
        params.timeout_s = None;

        module.set_default_timeout_s(Some(1));
        let started = Instant::now();
        let (_id, outcome) = module.ask_blocking("s1", &params);
        assert!(matches!(outcome, QuestionOutcome::Skipped));
        let one_sec = started.elapsed();
        assert!(
            one_sec >= Duration::from_millis(900),
            "1s default returned early: {one_sec:?}"
        );

        module.set_default_timeout_s(Some(2));
        let started = Instant::now();
        let (_id, outcome) = module.ask_blocking("s1", &params);
        assert!(matches!(outcome, QuestionOutcome::Skipped));
        let two_sec = started.elapsed();
        assert!(
            two_sec >= Duration::from_millis(1_900),
            "2s default returned early: {two_sec:?}"
        );
        assert!(
            two_sec > one_sec,
            "a longer default must produce a longer deadline: {one_sec:?} vs {two_sec:?}"
        );
    }

    #[test]
    fn explicit_timeout_beats_the_default() {
        let module = delivering_module();
        module.set_default_timeout_s(Some(9));
        let mut params = params_with_options();
        params.timeout_s = Some(1);
        let started = Instant::now();
        let (_id, _outcome) = module.ask_blocking("s1", &params);
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(900),
            "explicit 1s must be honoured, not the 9s default: {elapsed:?}"
        );
        assert!(elapsed < Duration::from_millis(3_000));
    }

    #[test]
    fn default_timeout_settles_once_and_late_respond_is_rejected() {
        let module = delivering_module();
        let (seen, hook) = recording_hook();
        module.set_settle_hook(hook);
        module.set_default_timeout_s(Some(1));
        let mut params = params_with_options();
        params.timeout_s = None;
        let (qid, outcome) = module.ask_blocking("s1", &params);
        assert!(matches!(outcome, QuestionOutcome::Skipped));
        assert_eq!(module.pending_count(), 0);
        assert_eq!(module.session_of(&qid), None);
        assert_eq!(
            module.respond(&qid, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(false),
            "a late response must be rejected idempotently, not resurrect the question"
        );
        assert_eq!(module.pending_count(), 0);
        let seen = lock_or_recover(&seen);
        assert_eq!(
            seen.len(),
            1,
            "settle hook must fire exactly once on the default-timeout path"
        );
        assert_eq!(seen[0].question_id, qid);
        assert_eq!(seen[0].outcome, QuestionOutcome::Skipped);
    }

    #[test]
    fn set_default_timeout_none_restores_unbounded_wait() {
        let module = QuestionModule::new();
        let params = params_with_options(); // timeout_s: None
        assert_eq!(module.effective_timeout(&params), None);
        module.set_default_timeout_s(Some(1));
        assert_eq!(module.effective_timeout(&params), Some(1));
        let mut explicit = params.clone();
        explicit.timeout_s = Some(5);
        assert_eq!(module.effective_timeout(&explicit), Some(5));
        module.set_default_timeout_s(None);
        assert_eq!(module.effective_timeout(&params), None);
    }

    #[test]
    fn cancel_session_settles_all_pending() {
        let m = delivering_module();
        let (seen, hook) = recording_hook();
        m.set_settle_hook(hook);
        let asker1 = Arc::clone(&m);
        let h1 = std::thread::spawn(move || asker1.ask_blocking("s1", &params_with_options()));
        let asker2 = Arc::clone(&m);
        let h2 = std::thread::spawn(move || asker2.ask_blocking("s1", &params_with_options()));
        let ids = wait_for_pending_count(&m, 2);
        assert_eq!(m.cancel_session("s1"), 2);
        let (id1, outcome1) = h1.join().unwrap();
        let (id2, outcome2) = h2.join().unwrap();
        assert_eq!(outcome1, QuestionOutcome::Skipped);
        assert_eq!(outcome2, QuestionOutcome::Skipped);
        assert!(ids.contains(&id1) && ids.contains(&id2));
        assert_eq!(m.pending_count(), 0);
        assert_eq!(m.cancel_session("s1"), 0, "no pending left to cancel");
        // cancel_session drops the session's settled records (决策 #4), so a
        // late respond is an idempotent false instead of a resurrection.
        assert_eq!(m.settled_of(&id1), None);
        assert_eq!(m.settled_of(&id2), None);
        assert_eq!(
            m.respond(&id1, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(false)
        );
        let seen = lock_or_recover(&seen);
        assert_eq!(seen.len(), 2, "one hook fire per settled question");
        for recorded in seen.iter() {
            assert_eq!(recorded.session_id, "s1");
            assert_eq!(recorded.outcome, QuestionOutcome::Skipped);
        }
        let mut recorded_ids: Vec<&str> = seen.iter().map(|s| s.question_id.as_str()).collect();
        recorded_ids.sort_unstable();
        let mut expected = vec![id1.as_str(), id2.as_str()];
        expected.sort_unstable();
        assert_eq!(recorded_ids, expected);
    }

    #[test]
    fn cancel_session_without_pending_is_noop() {
        let m = delivering_module();
        let (seen, hook) = recording_hook();
        m.set_settle_hook(hook);
        assert_eq!(m.cancel_session("s-missing"), 0);
        assert_eq!(m.pending_count(), 0);
        assert!(
            lock_or_recover(&seen).is_empty(),
            "no-op cancel must not fire hooks"
        );
        let asker = Arc::clone(&m);
        let h = std::thread::spawn(move || asker.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        assert_eq!(m.cancel_session("s-other"), 0);
        assert_eq!(m.pending_count(), 1);
        assert_eq!(m.session_of(&qid), Some("s1".into()));
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
        assert_eq!(lock_or_recover(&seen).len(), 1);
    }

    #[test]
    fn respond_racing_timeout_settles_exactly_once() {
        let m = delivering_module();
        let (seen, hook) = recording_hook();
        m.set_settle_hook(hook);
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for i in 0..6 {
            // 950ms: respond usually wins; 1000ms: true race; 1050ms: timeout wins.
            let (qid, response, outcome) = race_once(&m, 950 + (i as u64 % 3) * 50);
            let seen = lock_or_recover(&seen);
            assert_eq!(seen.len(), i + 1, "exactly one settlement per question");
            assert_eq!(seen[i].question_id, qid);
            assert_eq!(
                seen[i].outcome, outcome,
                "hook must match the asker's outcome"
            );
            match response {
                RespondOutcome::Accepted(true) => accepted += 1,
                RespondOutcome::Accepted(false) => rejected += 1,
            }
        }
        assert_eq!(accepted + rejected, 6);
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn timeout_losing_removal_race_waits_for_winner() {
        let m = delivering_module();
        let delivered = Arc::new(AtomicUsize::new(0));
        let delivered2 = Arc::clone(&delivered);
        m.set_settle_hook(Arc::new(
            move |_s: &str, _q: &str, _st: &QuestionSettlement| {
                delivered2.fetch_add(1, Ordering::SeqCst);
                // Delay the winner's channel delivery past the 1s deadline.
                std::thread::sleep(Duration::from_millis(1000));
            },
        ));
        let mut params = params_with_options();
        params.timeout_s = Some(1);
        let asker = Arc::clone(&m);
        let started = Instant::now();
        let h = std::thread::spawn(move || asker.ask_blocking("s1", &params));
        let qid = wait_for_pending(&m);
        let responder = Arc::clone(&m);
        let qid2 = qid.clone();
        let r = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            responder.respond(&qid2, vec!["a".into()], false)
        });
        let response = r.join().unwrap().unwrap();
        let (asked_id, outcome) = h.join().unwrap();
        let elapsed = started.elapsed();
        assert_eq!(asked_id, qid);
        assert_eq!(
            response,
            RespondOutcome::Accepted(true),
            "respond won at ~200ms"
        );
        assert_eq!(outcome, answered(&["a"]));
        assert!(
            elapsed >= Duration::from_millis(1000),
            "asker must have timed out before the delayed delivery: {elapsed:?}"
        );
        assert_eq!(delivered.load(Ordering::SeqCst), 1);
        assert_eq!(m.settled_of(&qid), Some(answered(&["a"])));
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn settle_hook_panic_is_isolated() {
        let m = delivering_module();
        let (calls, hook) = panicking_hook();
        m.set_settle_hook(hook);
        // Respond path: the hook panics, respond still reports acceptance.
        let asker = Arc::clone(&m);
        let h1 = std::thread::spawn(move || asker.ask_blocking("s1", &params_with_options()));
        let q1 = wait_for_pending(&m);
        assert_eq!(
            m.respond(&q1, vec!["a".into()], false).unwrap(),
            RespondOutcome::Accepted(true)
        );
        let (_, outcome) = h1.join().unwrap();
        assert_eq!(
            outcome,
            QuestionOutcome::Answered {
                answers: vec!["a".into()]
            }
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(m.pending_count(), 0);
        assert_eq!(
            m.settled_of(&q1),
            Some(QuestionOutcome::Answered {
                answers: vec!["a".into()]
            })
        );
        // The module is neither poisoned nor torn: swap in a recording hook
        // and run one more full roundtrip.
        let (seen, good_hook) = recording_hook();
        m.set_settle_hook(good_hook);
        let asker = Arc::clone(&m);
        let h2 = std::thread::spawn(move || asker.ask_blocking("s1", &params_with_options()));
        let q2 = wait_for_pending(&m);
        assert_eq!(
            m.respond(&q2, vec!["b".into()], false).unwrap(),
            RespondOutcome::Accepted(true)
        );
        let (_, outcome) = h2.join().unwrap();
        assert_eq!(
            outcome,
            QuestionOutcome::Answered {
                answers: vec!["b".into()]
            }
        );
        assert_eq!(lock_or_recover(&seen).len(), 1);
    }

    #[test]
    fn settle_hook_panic_on_cancel_path_is_isolated() {
        let m = delivering_module();
        let (calls, hook) = panicking_hook();
        m.set_settle_hook(hook);
        let asker = Arc::clone(&m);
        let h = std::thread::spawn(move || asker.ask_blocking("s1", &params_with_options()));
        let qid = wait_for_pending(&m);
        assert_eq!(m.cancel_session("s1"), 1);
        let (_, outcome) = h.join().unwrap();
        assert_eq!(
            outcome,
            QuestionOutcome::Skipped,
            "cancel must settle Skipped despite panic"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(m.pending_count(), 0);
        assert_eq!(m.settled_of(&qid), None);
        assert_eq!(m.session_of(&qid), None);
    }
}
