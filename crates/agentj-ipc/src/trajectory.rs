//! Append-only per-session trajectory log (concretization 3): NDJSON at
//! `<session-dir>/<sid>.trajectory`, one canonical TrajectoryEvent per line
//! (byte-identical to the `session/history` wire payload). Rolling trim keeps
//! the newest tail under caps; below thresholds append is O(1). Placed in the
//! ipc crate deliberately: agentj-session stays protocol-free.
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agentj_core::sync::lock_or_recover;
use agentj_protocol::trajectory::{TrajectoryEvent, TrajectoryValue};
use agentj_protocol::types::SessionId;

pub const DEFAULT_MAX_EVENTS: u64 = 10_000;
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;

struct LogState {
    count: u64,
    bytes: u64,
    next_seq: u64,
}

pub struct TrajectoryStore {
    dir: PathBuf,
    max_events: u64,
    max_bytes: u64,
    logs: Mutex<HashMap<SessionId, LogState>>,
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl TrajectoryStore {
    pub fn new(dir: PathBuf, max_events: Option<u64>, max_bytes: Option<u64>) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            max_events: max_events.unwrap_or(DEFAULT_MAX_EVENTS),
            max_bytes: max_bytes.unwrap_or(DEFAULT_MAX_BYTES),
            logs: Mutex::new(HashMap::new()),
        }
    }

    fn path(&self, sid: &SessionId) -> PathBuf {
        self.dir.join(format!("{sid}.trajectory"))
    }

    /// Assigns the next per-session seq and appends (trimming first if a cap
    /// would be exceeded). Returns the assigned seq.
    pub fn record(&self, sid: &SessionId, value: TrajectoryValue) -> std::io::Result<u64> {
        self.record_at(sid, now_epoch_ms(), value)
    }

    pub fn record_at(
        &self,
        sid: &SessionId,
        ts: u64,
        value: TrajectoryValue,
    ) -> std::io::Result<u64> {
        let mut logs = lock_or_recover(&self.logs);
        let path = self.path(sid);
        let state = logs.entry(sid.clone()).or_insert_with(|| load_state(&path));
        let event = TrajectoryEvent {
            session_id: sid.clone(),
            seq: state.next_seq,
            ts,
            value,
        };
        let line = match serde_json::to_string(&event) {
            Ok(json) => format!("{json}\n"),
            Err(e) => {
                tracing::error!(error = %e, "trajectory event is not serializable; skipped");
                return Ok(event.seq);
            }
        };
        let add = line.len() as u64;
        if state.count >= self.max_events || state.bytes + add > self.max_bytes {
            trim_to_caps(&path, state, self.max_events, self.max_bytes)?;
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        f.write_all(line.as_bytes())?;
        f.flush()?;
        state.count += 1;
        state.bytes += add;
        state.next_seq += 1;
        Ok(event.seq)
    }

    /// Events with seq >= cursor, oldest first, capped at `limit`.
    /// `(events, next_cursor)`: next_cursor = last seen seq + 1 when the
    /// window was full, else None.
    pub fn read_paged(
        &self,
        sid: &SessionId,
        cursor: Option<u64>,
        limit: usize,
    ) -> std::io::Result<(Vec<TrajectoryEvent>, Option<u64>)> {
        let path = self.path(sid);
        if !path.exists() {
            return Ok((vec![], None));
        }
        let f = fs::File::open(&path)?;
        let mut out = Vec::new();
        let mut seen = 0u64;
        for line in BufReader::new(f).lines() {
            let Ok(line) = line else { continue };
            let Ok(ev) = serde_json::from_str::<TrajectoryEvent>(&line) else {
                continue; // corrupt/partial tail line — skip, never fatal
            };
            if ev.seq < cursor.unwrap_or(0) {
                continue;
            }
            seen = ev.seq;
            out.push(ev);
            if out.len() == limit {
                break;
            }
        }
        let next = if out.len() == limit {
            Some(seen + 1)
        } else {
            None
        };
        Ok((out, next))
    }
}

/// Re-parse the file: line count, byte length and the max seq on disk.
fn load_state(path: &Path) -> LogState {
    if !path.exists() {
        return LogState {
            count: 0,
            bytes: 0,
            next_seq: 1,
        };
    }
    let mut count = 0u64;
    let mut max_seq = 0u64;
    if let Ok(f) = fs::File::open(path) {
        for line in BufReader::new(f).lines() {
            count += 1;
            let Ok(line) = line else { continue };
            if let Ok(ev) = serde_json::from_str::<TrajectoryEvent>(&line) {
                max_seq = max_seq.max(ev.seq);
            }
        }
    }
    let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    LogState {
        count,
        bytes,
        next_seq: max_seq + 1,
    }
}

/// Rewrite keeping the newest events that fit BOTH caps (tmp + rename).
fn trim_to_caps(
    path: &Path,
    state: &mut LogState,
    max_events: u64,
    max_bytes: u64,
) -> std::io::Result<()> {
    let events = read_events(path)?;
    let mut kept: Vec<TrajectoryEvent> = Vec::with_capacity(events.len());
    let mut kept_bytes: u64 = 0;
    // Walk newest-first (reversed later) — stop when either cap would be exceeded.
    // Reserve one slot for the incoming record so count stays <= max_events after append.
    let reserve_events = max_events.saturating_sub(1);
    for ev in events.iter().rev() {
        let line_len = serde_json::to_string(ev)
            .map(|s| s.len() as u64 + 1)
            .unwrap_or(0);
        if kept.len() as u64 >= reserve_events || kept_bytes + line_len > max_bytes {
            break;
        }
        kept.push(ev.clone());
        kept_bytes += line_len;
    }
    kept.reverse();
    let mut text = String::new();
    for ev in &kept {
        match serde_json::to_string(ev) {
            Ok(json) => {
                text.push_str(&json);
                text.push('\n');
            }
            Err(e) => tracing::error!(error = %e, "trajectory event is not serializable; dropped"),
        }
    }
    let tmp = path.with_extension("trajectory.tmp");
    fs::write(&tmp, text.as_bytes())?;
    fs::rename(&tmp, path)?;
    state.count = kept.len() as u64;
    state.bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let last_seq = kept.last().map(|e| e.seq).unwrap_or(0);
    state.next_seq = state.next_seq.max(last_seq + 1);
    Ok(())
}

fn read_events(path: &Path) -> std::io::Result<Vec<TrajectoryEvent>> {
    let f = fs::File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { continue };
        if let Ok(ev) = serde_json::from_str::<TrajectoryEvent>(&line) {
            out.push(ev);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_protocol::session::UsageInfo;
    use agentj_protocol::trajectory::{TrajectoryEvent, TrajectoryValue};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aj-trajectory-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_all(dir: &std::path::Path, sid: &str) -> Vec<TrajectoryEvent> {
        let f = std::fs::File::open(dir.join(format!("{sid}.trajectory"))).unwrap();
        std::io::BufRead::lines(std::io::BufReader::new(f))
            .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn record_appends_ndjson_with_sequential_seq() {
        let dir = temp_dir("append");
        let store = TrajectoryStore::new(dir.clone(), None, None);
        let sid: SessionId = "s-1".into();
        store
            .record(
                &sid,
                TrajectoryValue::User {
                    content: "hi".into(),
                },
            )
            .unwrap();
        store
            .record(
                &sid,
                TrajectoryValue::Thinking {
                    content: "plan".into(),
                },
            )
            .unwrap();
        store
            .record(
                &sid,
                TrajectoryValue::Usage {
                    usage: UsageInfo {
                        prompt_tokens: 1,
                        completion_tokens: 2,
                    },
                },
            )
            .unwrap();
        let events = read_all(&dir, "s-1");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
        assert!(matches!(events[0].value, TrajectoryValue::User { .. }));
        assert_eq!(events[2].session_id, "s-1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_keeps_newest_tail_when_over_event_cap() {
        let dir = temp_dir("trim");
        let store = TrajectoryStore::new(dir.clone(), Some(3), None);
        let sid: SessionId = "s-1".into();
        for i in 1..=5 {
            store
                .record(
                    &sid,
                    TrajectoryValue::User {
                        content: format!("m{i}"),
                    },
                )
                .unwrap();
        }
        let events = read_all(&dir, "s-1");
        assert_eq!(events.len(), 3, "only newest 3 survive");
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert!(matches!(&events[0].value, TrajectoryValue::User { content } if content == "m3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seq_resumes_after_store_restart() {
        let dir = temp_dir("resume");
        let sid: SessionId = "s-1".into();
        {
            let store = TrajectoryStore::new(dir.clone(), None, None);
            store
                .record(
                    &sid,
                    TrajectoryValue::User {
                        content: "a".into(),
                    },
                )
                .unwrap();
            store
                .record(
                    &sid,
                    TrajectoryValue::User {
                        content: "b".into(),
                    },
                )
                .unwrap();
        }
        let store2 = TrajectoryStore::new(dir.clone(), None, None);
        store2
            .record(
                &sid,
                TrajectoryValue::User {
                    content: "c".into(),
                },
            )
            .unwrap();
        let events = read_all(&dir, "s-1");
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_paged_cursor_and_next_boundary() {
        let dir = temp_dir("page");
        let store = TrajectoryStore::new(dir.clone(), None, None);
        let sid: SessionId = "s-1".into();
        for i in 1..=5 {
            store
                .record(
                    &sid,
                    TrajectoryValue::User {
                        content: format!("m{i}"),
                    },
                )
                .unwrap();
        }
        let (page, next) = store.read_paged(&sid, None, 3).unwrap();
        assert_eq!(
            page.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(next, Some(4), "more events follow the limit window");
        let (page2, next2) = store.read_paged(&sid, next, 10).unwrap();
        assert_eq!(page2.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(next2, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_line_is_skipped_not_fatal() {
        let dir = temp_dir("corrupt");
        let store = TrajectoryStore::new(dir.clone(), None, None);
        let sid: SessionId = "s-1".into();
        store
            .record(
                &sid,
                TrajectoryValue::User {
                    content: "a".into(),
                },
            )
            .unwrap();
        store
            .record(
                &sid,
                TrajectoryValue::User {
                    content: "b".into(),
                },
            )
            .unwrap();
        let path = dir.join("s-1.trajectory");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, "{{ not json").unwrap();
        drop(f);
        let (page, _) = store.read_paged(&sid, None, 10).unwrap();
        assert_eq!(page.len(), 2, "corrupt line skipped, good events survive");
        std::fs::remove_dir_all(&dir).ok();
    }
}
