//! Persisted login-limiter state: restore at startup, the debounced flush task
//! and the graceful-shutdown flush (design D3). A second `impl WebConfig` block
//! so the config module stays free of filesystem and task lifecycles.

use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinHandle;

use agentj_core::sync::{lock_or_recover, write_or_recover};

use super::*;

impl WebConfig {
    /// Load the persisted limiter state (design D3): a missing file is a normal
    /// first run; an unusable one warns once and starts from zero, never blocking
    /// startup. Returns whether state was restored.
    pub(crate) fn restore_metrics_state(&self) -> bool {
        let Some(path) = self.metrics_state.as_deref() else {
            return false;
        };
        match crate::metrics_state::read(path) {
            Ok(Some(state)) => {
                let entries = self.login_limiter.restore_from(
                    &state,
                    Instant::now(),
                    crate::metrics_state::unix_now(),
                );
                let mut report = write_or_recover(&self.state_report);
                report.restored = true;
                report.restored_entries = entries as u64;
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.warn_once(
                    "metrics-state-read",
                    &format!(
                        "warning: login metrics state at {} is unusable ({error}); counters start at zero",
                        path.display()
                    ),
                );
                false
            }
        }
    }
    /// Write the limiter state if a path is configured (design D3). Failures are
    /// counted and warned once; the dirty flag stays set so the next tick retries.
    ///
    /// Graceful-shutdown API: the CLI calls this (before `stop_metrics_flush`)
    /// so a recorded login-failure window survives the process exiting.
    pub fn flush_metrics_state(&self) {
        let Some(path) = self.metrics_state.as_deref() else {
            return;
        };
        let now = Instant::now();
        let now_unix = crate::metrics_state::unix_now();
        let generation = self.login_limiter.generation();
        let state = self.login_limiter.to_state(now, now_unix);
        match crate::metrics_state::write(path, &state) {
            Ok(()) => {
                // Conditional clear: a lockout recorded while this snapshot was
                // being written is missing from it, so the next tick must retry.
                self.login_limiter.mark_clean_at(generation);
                let mut report = write_or_recover(&self.state_report);
                report.last_flush_unix = now_unix;
            }
            Err(error) => {
                {
                    let mut report = write_or_recover(&self.state_report);
                    report.write_failures += 1;
                }
                self.warn_once(
                    "metrics-state-write",
                    &format!(
                        "warning: cannot persist login metrics state to {} ({error})",
                        path.display()
                    ),
                );
            }
        }
    }
    /// Periodic debounced flush (design D3): one tick writes only when the
    /// limiter is dirty, mirroring the spot-check task's lifecycle.
    pub(crate) fn spawn_metrics_flush(self: &Arc<Self>) -> Option<JoinHandle<()>> {
        self.metrics_state.as_ref()?;
        let web = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(web.flush_interval);
            loop {
                ticks.tick().await;
                if web.login_limiter.dirty() {
                    web.flush_metrics_state();
                }
            }
        });
        *lock_or_recover(&self.flush_task) = Some(handle.abort_handle());
        Some(handle)
    }
    /// Graceful-shutdown API: abort the debounced flush task. Callers flush
    /// first (`flush_metrics_state`), so the last snapshot is never lost.
    pub fn stop_metrics_flush(&self) {
        let task = agentj_core::sync::lock_or_recover(&self.flush_task).take();
        if let Some(task) = task {
            task.abort();
        }
    }
}
