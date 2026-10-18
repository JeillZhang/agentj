//! Persisted login-limiter state (design D3/D4): the serde snapshot, its atomic
//! 0700/0600 write, the bounded read, and the `Instant` <-> wall-clock contract.
//!
//! Lock windows are relative durations (a monotonic `Instant` cannot be
//! serialized), so every conversion is clamped: a lock never exceeds
//! `max_lockout` and an idle age never exceeds `idle_ttl`, no matter what the
//! wall clock did across restarts.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub(crate) const STATE_VERSION: u32 = 1;
/// Anything larger is not a state file we wrote; refuse to parse it.
pub(crate) const MAX_STATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MetricsState {
    pub(crate) version: u32,
    pub(crate) saved_at_unix: u64,
    pub(crate) counters: CountersSnapshot,
    pub(crate) ips: Vec<IpState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct CountersSnapshot {
    pub(crate) attempts: u64,
    pub(crate) failures: u64,
    pub(crate) lockouts: u64,
    pub(crate) blocked: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IpState {
    pub(crate) ip: String,
    pub(crate) failures: u32,
    pub(crate) locked_until_unix: Option<u64>,
    pub(crate) last_unix: u64,
}

/// One restored entry, expressed relative to the current `Instant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntrySeed {
    pub(crate) failures: u32,
    pub(crate) locked_for: Option<Duration>,
    pub(crate) last_age: Duration,
}

#[derive(Debug)]
pub(crate) enum StateError {
    Io(String),
    TooLarge(u64),
    BadVersion(u32),
    Parse(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Io(message) => write!(f, "io error: {message}"),
            StateError::TooLarge(bytes) => {
                write!(f, "state file is {bytes} bytes (limit {MAX_STATE_BYTES})")
            }
            StateError::BadVersion(version) => write!(f, "unsupported state version {version}"),
            StateError::Parse(message) => write!(f, "unparsable state: {message}"),
        }
    }
}

fn io_err(error: std::io::Error) -> StateError {
    StateError::Io(error.to_string())
}

/// Best-effort removal of a partially written tmp file before surfacing the
/// error: a failed write must never leave a `*.json.tmp` behind (design D4).
fn discard_tmp(tmp: &Path, error: std::io::Error) -> StateError {
    let _ = std::fs::remove_file(tmp);
    io_err(error)
}

/// State files are per-port: two instances in one directory must not overwrite
/// each other (design D4).
pub(crate) fn state_path_in(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("metrics-state-{port}.json"))
}

/// Seconds since the Unix epoch; 0 if the clock is before it (never panics).
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

pub(crate) fn write(path: &Path, state: &MetricsState) -> Result<(), StateError> {
    let dir = path
        .parent()
        .ok_or_else(|| StateError::Io("state path has no parent".into()))?;
    std::fs::create_dir_all(dir).map_err(io_err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io_err)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let body = serde_json::to_string(state).map_err(|e| StateError::Parse(e.to_string()))?;
    let mut file = opts.open(&tmp).map_err(io_err)?;
    let written =
        std::io::Write::write_all(&mut file, body.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        return Err(discard_tmp(&tmp, error));
    }
    std::fs::rename(&tmp, path).map_err(|error| discard_tmp(&tmp, error))
}

/// `Ok(None)` means "no state yet" (a normal first run); `Err` means the file
/// exists but is unusable, and the caller warns once and starts from zero.
pub(crate) fn read(path: &Path) -> Result<Option<MetricsState>, StateError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_err(error)),
    };
    if metadata.len() > MAX_STATE_BYTES {
        return Err(StateError::TooLarge(metadata.len()));
    }
    let text = std::fs::read_to_string(path).map_err(io_err)?;
    let state: MetricsState =
        serde_json::from_str(&text).map_err(|e| StateError::Parse(e.to_string()))?;
    if state.version != STATE_VERSION {
        return Err(StateError::BadVersion(state.version));
    }
    Ok(Some(state))
}

/// Convert stored wall-clock entries into `Instant`-relative seeds (design D3):
/// expired locks are dropped, live ones are clamped to `max_lockout`, entries
/// idle for `idle_ttl` or longer are discarded, and the result is truncated to
/// `max_tracked` in a deterministic order.
pub(crate) fn restore_seeds(
    ips: &[IpState],
    now_unix: u64,
    max_lockout: Duration,
    idle_ttl: Duration,
    max_tracked: usize,
) -> Vec<(IpAddr, EntrySeed)> {
    let idle_secs = idle_ttl.as_secs();
    let mut seeds: Vec<(IpAddr, EntrySeed)> = ips
        .iter()
        .filter_map(|stored| {
            let ip: IpAddr = stored.ip.parse().ok()?;
            let last_age_secs = now_unix.saturating_sub(stored.last_unix);
            if last_age_secs >= idle_secs {
                return None;
            }
            let locked_for = stored.locked_until_unix.and_then(|until| {
                // Clamp in the `Duration` domain: a policy with a sub-second
                // `max_lockout` must not round its own cap down to zero seconds.
                let remaining = Duration::from_secs(until.saturating_sub(now_unix));
                let clamped = remaining.min(max_lockout);
                (!clamped.is_zero()).then_some(clamped)
            });
            Some((
                ip,
                EntrySeed {
                    failures: stored.failures,
                    locked_for,
                    last_age: Duration::from_secs(last_age_secs),
                },
            ))
        })
        .collect();
    seeds.sort_by(|a, b| {
        b.1.failures
            .cmp(&a.1.failures)
            .then(a.1.last_age.cmp(&b.1.last_age))
            .then(a.0.cmp(&b.0))
    });
    seeds.truncate(max_tracked);
    seeds
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample_ip(
        ip: &str,
        failures: u32,
        locked_until_unix: Option<u64>,
        last_unix: u64,
    ) -> IpState {
        IpState {
            ip: ip.to_string(),
            failures,
            locked_until_unix,
            last_unix,
        }
    }

    fn sample_state() -> MetricsState {
        MetricsState {
            version: STATE_VERSION,
            saved_at_unix: 1_700_000_000,
            counters: CountersSnapshot {
                attempts: 42,
                failures: 7,
                lockouts: 1,
                blocked: 2,
            },
            ips: vec![
                sample_ip("203.0.113.9", 5, Some(1_700_000_030), 1_700_000_000),
                sample_ip("198.51.100.4", 2, None, 1_699_999_990),
            ],
        }
    }

    const MAX_LOCKOUT: Duration = Duration::from_secs(900);
    const IDLE_TTL: Duration = Duration::from_secs(1800);

    #[test]
    fn state_path_is_port_scoped() {
        let path = state_path_in(std::path::Path::new("/tmp/agentj"), 4096);
        assert_eq!(
            path,
            std::path::Path::new("/tmp/agentj/metrics-state-4096.json")
        );
        assert_ne!(
            state_path_in(std::path::Path::new("/tmp/agentj"), 4096),
            state_path_in(std::path::Path::new("/tmp/agentj"), 4097)
        );
    }

    #[test]
    fn write_then_read_round_trips_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path(), 4096);
        write(&path, &sample_state()).unwrap();
        let back = read(&path).unwrap().expect("state present");
        assert_eq!(back, sample_state());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "tmp files leaked: {leftovers:?}");
    }

    #[test]
    fn write_is_atomic_and_never_leaves_a_tmp_behind() {
        // Force the final `rename` to fail: the destination already exists as a
        // directory, so the tmp file must be cleaned up on that error path too.
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path(), 4096);
        std::fs::create_dir(&path).unwrap();
        assert!(
            write(&path, &sample_state()).is_err(),
            "writing over a directory must fail"
        );
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "tmp files leaked: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn write_sets_directory_0700_and_file_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("agentj");
        let path = state_path_in(&nested, 4096);
        write(&path, &sample_state()).unwrap();
        assert_eq!(
            std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&state_path_in(dir.path(), 4096)).unwrap().is_none());
    }

    #[test]
    fn corrupt_json_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path(), 4096);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(matches!(read(&path), Err(StateError::Parse(_))));
    }

    #[test]
    fn wrong_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path(), 4096);
        let mut state = sample_state();
        state.version = STATE_VERSION + 1;
        // Written through serde directly: `write` is not version-gated.
        std::fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
        assert!(matches!(read(&path), Err(StateError::BadVersion(_))));
    }

    #[test]
    fn oversized_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path(), 4096);
        std::fs::write(&path, vec![b' '; MAX_STATE_BYTES as usize + 1]).unwrap();
        assert!(matches!(read(&path), Err(StateError::TooLarge(_))));
    }

    #[test]
    fn expired_lock_keeps_the_failure_count() {
        let state = vec![sample_ip(
            "203.0.113.9",
            7,
            Some(1_700_000_000),
            1_700_000_000,
        )];
        let seeds = restore_seeds(&state, 1_700_000_060, MAX_LOCKOUT, IDLE_TTL, 1024);
        assert_eq!(seeds.len(), 1);
        assert_eq!(
            seeds[0].1.failures, 7,
            "an expired lock must not clear history"
        );
        assert!(seeds[0].1.locked_for.is_none());
    }

    #[test]
    fn remaining_lockout_is_clamped_to_max_lockout() {
        // Wall clock jumped back two days: the lock must not become 48h long.
        let state = vec![sample_ip(
            "203.0.113.9",
            5,
            Some(1_700_200_000),
            1_700_000_000,
        )];
        let seeds = restore_seeds(&state, 1_700_000_000, MAX_LOCKOUT, IDLE_TTL, 1024);
        assert_eq!(seeds[0].1.locked_for, Some(MAX_LOCKOUT));
    }

    #[test]
    fn entries_idle_beyond_the_ttl_are_dropped() {
        let state = vec![
            sample_ip("203.0.113.9", 5, None, 1_699_998_000),
            sample_ip("198.51.100.4", 1, None, 1_700_000_000),
        ];
        let seeds = restore_seeds(&state, 1_700_000_000, MAX_LOCKOUT, IDLE_TTL, 1024);
        assert_eq!(seeds.len(), 1);
        assert_eq!(
            seeds[0].0,
            "198.51.100.4".parse::<std::net::IpAddr>().unwrap()
        );
    }

    #[test]
    fn more_than_max_tracked_entries_are_truncated_stably() {
        let state: Vec<IpState> = (0..5000u32)
            .map(|n| {
                let ip = format!("10.{}.{}.{}", n / 65536, (n / 256) % 256, n % 256);
                sample_ip(&ip, n % 9, None, 1_700_000_000)
            })
            .collect();
        let seeds = restore_seeds(&state, 1_700_000_000, MAX_LOCKOUT, IDLE_TTL, 1024);
        assert_eq!(seeds.len(), 1024);
        assert_eq!(
            seeds[0].1.failures, 8,
            "the highest failure count sorts first"
        );
        let again = restore_seeds(&state, 1_700_000_000, MAX_LOCKOUT, IDLE_TTL, 1024);
        assert_eq!(
            seeds.iter().map(|(ip, _)| *ip).collect::<Vec<_>>(),
            again.iter().map(|(ip, _)| *ip).collect::<Vec<_>>(),
            "truncation must be deterministic"
        );
    }
}
