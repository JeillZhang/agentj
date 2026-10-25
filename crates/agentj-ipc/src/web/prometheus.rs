//! Prometheus text exposition: limiter aggregates, the ranked per-IP series
//! capped at [`PER_IP_SERIES_LIMIT`], the startup/runtime integrity summaries and
//! the persisted-state gauges (design G2). Pure formatting over `WebConfig`
//! snapshots — no I/O, so the `/metrics` handler stays a thin gate.

use std::time::Instant;

use crate::client_ip::IpSource;
use crate::login_limiter::{Counters, IpSnapshot};
use agentj_core::sync::read_or_recover;

use super::*;

/// Prometheus text exposition (design G2): limiter aggregates, the ranked
/// per-IP series capped at [`PER_IP_SERIES_LIMIT`], the startup integrity
/// summary and the runtime spot-check counters.
pub(super) fn prometheus_text(web: &WebConfig) -> String {
    let now = Instant::now();
    let counters = web.login_limiter.counters(now);
    let series = web.login_limiter.snapshot(now);
    let (runtime, _) = web.runtime_snapshot();
    let mut out = String::new();
    push_login_aggregates(&mut out, &counters);
    push_login_ip_series(&mut out, &series, now);
    push_integrity_summary(&mut out, web.integrity());
    push_spot_summary(&mut out, &runtime);
    push_integrity_request_summary(&mut out, &web.request_integrity_snapshot());
    let (on_demand, _) = web.on_demand_snapshot();
    push_integrity_sweep_summary(&mut out, &on_demand);
    push_client_ip_summary(web, &mut out);
    push_state_summary(web, &mut out);
    out
}

fn push_login_aggregates(out: &mut String, counters: &Counters) {
    push_metric(
        out,
        "agentj_login_attempts_total",
        "counter",
        "Login attempts that reached the throttler.",
        counters.attempts,
    );
    push_metric(
        out,
        "agentj_login_failures_total",
        "counter",
        "Token check failures (locked-out attempts excluded).",
        counters.failures,
    );
    push_metric(
        out,
        "agentj_login_lockouts_total",
        "counter",
        "Lockouts newly triggered (extensions excluded).",
        counters.lockouts,
    );
    push_metric(
        out,
        "agentj_login_blocked_total",
        "counter",
        "Requests rejected while their source IP was locked out.",
        counters.blocked,
    );
    push_metric(
        out,
        "agentj_login_locked_ips",
        "gauge",
        "Source IPs currently locked out.",
        counters.locked_now,
    );
    push_metric(
        out,
        "agentj_login_tracked_ips",
        "gauge",
        "Source IPs currently tracked by the throttler.",
        counters.tracked_ips,
    );
}

fn push_login_ip_series(out: &mut String, snapshot: &[IpSnapshot], now: Instant) {
    let truncated = snapshot.len() > PER_IP_SERIES_LIMIT;
    let shown = &snapshot[..snapshot.len().min(PER_IP_SERIES_LIMIT)];
    push_ip_family(
        out,
        "agentj_login_ip_failures_total",
        "counter",
        "Token check failures per source IP (top series by lock state and failures).",
        shown,
        |s| u64::from(s.failures),
    );
    push_ip_family(
        out,
        "agentj_login_ip_locked",
        "gauge",
        "Whether the source IP is currently locked out (1) or not (0).",
        shown,
        |s| u64::from(s.locked),
    );
    push_ip_family(
        out,
        "agentj_login_ip_retry_after_seconds",
        "gauge",
        "Remaining lockout for the source IP in whole seconds (0 when unlocked).",
        shown,
        |s| s.retry_after.as_secs(),
    );
    push_ip_family(
        out,
        "agentj_login_ip_last_seen_seconds",
        "gauge",
        "Seconds since the source IP last reached the throttler.",
        shown,
        |s| now.duration_since(s.last_seen).as_secs(),
    );
    push_metric(
        out,
        "agentj_login_ip_series_truncated",
        "gauge",
        "Whether the per-IP series were capped at the Top-50 limit (1) or not (0).",
        u64::from(truncated),
    );
}

fn push_integrity_summary(out: &mut String, report: &IntegrityReport) {
    push_metric(
        out,
        "agentj_web_manifest_present",
        "gauge",
        "Whether the dist asset manifest was present and usable (1) or not (0).",
        u64::from(report.manifest_present),
    );
    push_metric(
        out,
        "agentj_web_assets_checked",
        "gauge",
        "Startup integrity check: assets hashed.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_assets_mismatched",
        "gauge",
        "Startup integrity check: digest mismatches.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_assets_missing",
        "gauge",
        "Startup integrity check: manifest entries missing on disk.",
        report.missing,
    );
}

/// Runtime spot-check counters (design G2), cumulative across rounds.
fn push_spot_summary(out: &mut String, report: &SpotCheckReport) {
    push_metric(
        out,
        "agentj_web_spot_rounds_total",
        "counter",
        "Web asset spot-check rounds completed.",
        report.rounds,
    );
    push_metric(
        out,
        "agentj_web_spot_checked_total",
        "counter",
        "Web asset spot-check: files hashed across rounds.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_spot_mismatched_total",
        "counter",
        "Web asset spot-check: digest mismatches across rounds.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_spot_missing_total",
        "counter",
        "Web asset spot-check: manifest entries missing on disk across rounds.",
        report.missing,
    );
}

/// Request-path verification series (design G2): how many served assets were
/// hashed, how many of those were tampered with or already gone, and how many
/// verdicts the memo currently holds.
fn push_integrity_request_summary(out: &mut String, snapshot: &RequestIntegritySnapshot) {
    push_metric(
        out,
        "agentj_web_integrity_request_checked_total",
        "counter",
        "Per-request web asset checks that read and hashed a file.",
        snapshot.checked,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_mismatched_total",
        "counter",
        "Per-request web asset checks that found a digest mismatch.",
        snapshot.mismatched,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_missing_total",
        "counter",
        "Per-request web asset checks whose manifest entry was missing on disk.",
        snapshot.missing,
    );
    push_metric(
        out,
        "agentj_web_integrity_request_cache_entries",
        "gauge",
        "Memoised per-request verification verdicts currently held.",
        snapshot.cache_entries as u64,
    );
}

/// Manual-sweep series (design G3/G7): swept runs and their cumulative outcome.
fn push_integrity_sweep_summary(out: &mut String, report: &IntegrityOnDemandReport) {
    push_metric(
        out,
        "agentj_web_integrity_sweeps_total",
        "counter",
        "On-demand full asset sweeps actually executed.",
        report.sweeps,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_checked_total",
        "counter",
        "On-demand sweeps: files read and hashed.",
        report.checked,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_mismatched_total",
        "counter",
        "On-demand sweeps: digest mismatches.",
        report.mismatched,
    );
    push_metric(
        out,
        "agentj_web_integrity_sweep_missing_total",
        "counter",
        "On-demand sweeps: manifest entries missing on disk.",
        report.missing,
    );
}

fn push_metric(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    out.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"
    ));
}

/// Resolved client-IP source tallies plus the unusable-forwarded-header count
/// (design D6/G2). Labelled family: one header, then one sample per source.
fn push_client_ip_summary(web: &WebConfig, out: &mut String) {
    let totals = *read_or_recover(&web.ip_sources);
    out.push_str(
        "# HELP agentj_web_client_ip_source_total Resolved client-IP sources.\n\
         # TYPE agentj_web_client_ip_source_total counter\n",
    );
    for (source, value) in [
        (IpSource::Peer, totals.peer),
        (IpSource::XForwardedFor, totals.forwarded),
        (IpSource::XRealIp, totals.real_ip),
    ] {
        out.push_str(&format!(
            "agentj_web_client_ip_source_total{{source=\"{}\"}} {value}\n",
            source.label()
        ));
    }
    push_metric(
        out,
        "agentj_web_proxy_header_invalid_total",
        "counter",
        "Trusted requests whose forwarded headers were present but unusable.",
        totals.invalid,
    );
}

/// Persisted login-limiter state lifecycle (design D3/G2).
fn push_state_summary(web: &WebConfig, out: &mut String) {
    let report = *read_or_recover(&web.state_report);
    push_metric(
        out,
        "agentj_web_metrics_state_restored",
        "gauge",
        "Whether persisted login-limiter state was restored at startup (1) or not (0).",
        u64::from(report.restored),
    );
    push_metric(
        out,
        "agentj_web_metrics_state_entries_restored",
        "gauge",
        "Per-IP login-limiter entries restored at startup.",
        report.restored_entries,
    );
    push_metric(
        out,
        "agentj_web_metrics_state_write_failures_total",
        "counter",
        "Failed login-limiter state flushes since startup.",
        report.write_failures,
    );
    push_metric(
        out,
        "agentj_web_metrics_state_last_flush_unix",
        "gauge",
        "Wall-clock seconds of the last successful state flush (0 = never).",
        report.last_flush_unix,
    );
}

/// Family header followed by one labelled sample per shown IP; the text format
/// wants each family's samples contiguous, so callers loop one family at a time.
fn push_ip_family(
    out: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    series: &[IpSnapshot],
    value: impl Fn(&IpSnapshot) -> u64,
) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
    for snap in series {
        out.push_str(&format!(
            "{name}{{ip=\"{}\"}} {}\n",
            escape_label(&snap.ip.to_string()),
            value(snap)
        ));
    }
}

/// Escape a Prometheus label value: `\`, `"` and newlines (text format 0.0.4).
pub(super) fn escape_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}
