//! Poison-tolerant lock accessors and panic-payload rendering.
//!
//! A `std::sync` lock is poisoned once a thread panics while holding it. The
//! default ergonomics (`.lock().unwrap()`) then turn one panic into a permanent
//! outage: every later caller panics too, so a single bug in a module handler
//! would take the resident kernel down for good. This module is the single
//! place that decides what to do instead.
//!
//! Policy (see `docs/superpowers/specs/2026-10-24-hardening-dedup-batch-design.md`
//! §4.1): log the poison once and reuse the inner state. The state may be
//! partially updated by the panicking critical section, but the request entry
//! points isolate panics (`catch_unwind`), so the same bug cannot repeat in a
//! loop, and serving with a possibly stale cache beats refusing every request.

use std::any::Any;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Upper bound on the panic text carried into logs and `error.data.reason`.
pub const PANIC_MESSAGE_LIMIT: usize = 200;

/// Acquire a `Mutex`, recovering the inner value when it is poisoned.
pub fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| {
        tracing::error!("mutex poisoned; recovering inner state");
        poisoned.into_inner()
    })
}

/// Acquire a shared `RwLock` guard, recovering the inner value when poisoned.
pub fn read_or_recover<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|poisoned| {
        tracing::error!("rwlock poisoned; recovering inner state");
        poisoned.into_inner()
    })
}

/// Acquire an exclusive `RwLock` guard, recovering the inner value when poisoned.
pub fn write_or_recover<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|poisoned| {
        tracing::error!("rwlock poisoned; recovering inner state");
        poisoned.into_inner()
    })
}

/// Render a caught panic payload as text, truncating it to
/// [`PANIC_MESSAGE_LIMIT`] characters on a char boundary.
///
/// Both `panic!("literal")` (payload `&str`) and `panic!("{}", x)` (payload
/// `String`) are understood; anything else (e.g. `panic_any(42)`) yields a
/// fixed placeholder so logs and `error.data` stay well-formed.
pub fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .map(str::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());
    truncate_chars(&text, PANIC_MESSAGE_LIMIT)
}

/// Truncate to at most `limit` characters without splitting a multibyte char.
fn truncate_chars(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn unpoisoned_mutex_guard_mutates() {
        let m = Mutex::new(1u32);
        *lock_or_recover(&m) += 1;
        assert_eq!(*lock_or_recover(&m), 2);
        assert!(!m.is_poisoned());
    }

    #[test]
    fn poisoned_mutex_recovers_inner_state() {
        let m = Mutex::new(7u32);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let mut guard = lock_or_recover(&m);
            *guard = 9;
            panic!("boom");
        }));
        assert!(m.is_poisoned(), "the panic must poison the mutex");
        assert_eq!(*lock_or_recover(&m), 9);
    }

    #[test]
    fn poisoned_rwlock_read_and_write_recover() {
        let l = RwLock::new(3u32);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let mut guard = write_or_recover(&l);
            *guard = 5;
            panic!("boom");
        }));
        assert!(l.is_poisoned());
        assert_eq!(*read_or_recover(&l), 5);
        // The poisoned write lock keeps working too.
        *write_or_recover(&l) = 6;
        assert_eq!(*read_or_recover(&l), 6);
    }

    #[test]
    fn panic_payload_message_reads_str_and_string() {
        let from_str = catch_unwind(|| panic!("literal payload")).unwrap_err();
        assert_eq!(panic_payload_message(from_str.as_ref()), "literal payload");
        let from_fmt = catch_unwind(|| panic!("formatted {}", 42)).unwrap_err();
        assert_eq!(panic_payload_message(from_fmt.as_ref()), "formatted 42");
    }

    #[test]
    fn panic_payload_message_falls_back_for_non_string_payloads() {
        let payload = catch_unwind(|| std::panic::panic_any(7u8)).unwrap_err();
        assert_eq!(
            panic_payload_message(payload.as_ref()),
            "<non-string panic payload>"
        );
    }

    #[test]
    fn panic_payload_message_truncates_at_the_limit() {
        let long = "你".repeat(PANIC_MESSAGE_LIMIT + 20);
        let payload = catch_unwind(move || panic!("{long}")).unwrap_err();
        let rendered = panic_payload_message(payload.as_ref());
        assert_eq!(rendered.chars().count(), PANIC_MESSAGE_LIMIT);
        assert!(rendered.chars().all(|c| c == '你'), "no split characters");
    }
}
