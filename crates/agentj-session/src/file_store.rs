use agentj_core::error::{ModuleError, SessionError};
use agentj_core::module::{SessionStatus, SessionStore};
use agentj_core::types::{ChatMessage, SessionId, SessionSnapshot, now_epoch_secs};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Disk-backed session store: one JSON file per session (design §会话持久化与恢复).
/// Atomic write (.tmp → fsync → rename — a crash never leaves a half-written file),
/// startup scan + corrupt-skip (.corrupt backup), leftover-.tmp cleanup, session-id
/// charset guard [A-Za-z0-9_-] and IO errors → SessionError::PersistenceFailed.
pub struct FileBackedStore {
    dir: PathBuf,
    sessions: Mutex<HashMap<SessionId, Vec<ChatMessage>>>,
    statuses: Mutex<HashMap<SessionId, SessionStatus>>,
    created: Mutex<HashMap<SessionId, u64>>,
    updated: Mutex<HashMap<SessionId, u64>>,
}

fn is_valid_sid(sid: &str) -> bool {
    !sid.is_empty()
        && sid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn persistence_err(reason: String) -> ModuleError {
    ModuleError::Session(SessionError::PersistenceFailed(reason))
}

/// Poisoned-mutex recovery: a panicked holder must not take the store down with it.
fn lock_guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl FileBackedStore {
    /// Open/create the session dir; clean crash leftovers (.tmp), then scan and parse
    /// every <sid>.json into memory. Corrupt files → rename to <sid>.json.corrupt +
    /// tracing warn + skip (does NOT block construction); unreadable files are skipped.
    pub fn new(dir: &Path) -> Result<Self, ModuleError> {
        fs::create_dir_all(dir).map_err(|e| persistence_err(e.to_string()))?;
        let mut sessions = HashMap::new();
        let mut statuses = HashMap::new();
        let mut created = HashMap::new();
        let mut updated = HashMap::new();
        for entry in fs::read_dir(dir).map_err(|e| persistence_err(e.to_string()))? {
            let path = entry.map_err(|e| persistence_err(e.to_string()))?.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".tmp") {
                let _ = fs::remove_file(&path);
                continue;
            }
            if !name.ends_with(".json") {
                continue;
            }
            let sid = name.trim_end_matches(".json").to_string();
            if !is_valid_sid(&sid) {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&path) {
                match serde_json::from_str::<SessionSnapshot>(&raw) {
                    Ok(snap) => {
                        sessions.insert(sid.clone(), snap.messages);
                        statuses.insert(sid.clone(), snap.status);
                        created.insert(sid.clone(), snap.created_at);
                        updated.insert(sid.clone(), snap.updated_at);
                    }
                    Err(_) => {
                        let backup = path.with_extension("json.corrupt");
                        let _ = fs::rename(&path, backup);
                        tracing::warn!(session = %sid, "corrupt session file backed up as .corrupt");
                    }
                }
            } // unreadable file: skip, keep going
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            sessions: Mutex::new(sessions),
            statuses: Mutex::new(statuses),
            created: Mutex::new(created),
            updated: Mutex::new(updated),
        })
    }

    fn path_for(&self, sid: &SessionId) -> PathBuf {
        self.dir.join(format!("{sid}.json"))
    }

    fn require_active(&self, sid: &SessionId) -> Result<(), ModuleError> {
        match lock_guard(&self.statuses).get(sid) {
            Some(SessionStatus::Active) | None => Ok(()),
            _ => Err(ModuleError::Session(SessionError::InactiveSession(
                sid.clone(),
            ))),
        }
    }

    /// Synchronous persistence on every mutation (append/archive/delete/restore) — agent
    /// sessions append rarely and are small, so full-rewrite cost is acceptable (design
    /// §会话持久化与恢复「写入时机」). Atomic: write <sid>.json.tmp → fsync → rename.
    fn persist(&self, sid: &SessionId) -> Result<(), ModuleError> {
        if !is_valid_sid(sid) {
            return Err(persistence_err(format!(
                "session id '{sid}' has characters outside [A-Za-z0-9_-]"
            )));
        }
        let status = lock_guard(&self.statuses)
            .get(sid)
            .copied()
            .unwrap_or(SessionStatus::Active);
        let messages = lock_guard(&self.sessions)
            .get(sid)
            .cloned()
            .unwrap_or_default();
        let now = now_epoch_secs();
        let created_at = lock_guard(&self.created).get(sid).copied().unwrap_or(now);
        let snap = SessionSnapshot {
            session_id: sid.clone(),
            status,
            messages,
            created_at,
            updated_at: now,
        };
        let raw = serde_json::to_vec(&snap).map_err(|e| persistence_err(e.to_string()))?;
        let tmp = self
            .dir
            .join(format!("{sid}.{}.json.tmp", uuid::Uuid::new_v4()));
        let mut f = fs::File::create(&tmp).map_err(|e| persistence_err(e.to_string()))?;
        f.write_all(&raw)
            .and_then(|_| f.sync_all())
            .map_err(|e| persistence_err(e.to_string()))?;
        drop(f);
        fs::rename(&tmp, self.path_for(sid)).map_err(|e| persistence_err(e.to_string()))?;
        let dir_f = fs::File::open(&self.dir).map_err(|e| persistence_err(e.to_string()))?;
        dir_f.sync_all().map_err(|e| persistence_err(e.to_string()))?;
        lock_guard(&self.updated).insert(sid.clone(), now);
        Ok(())
    }
}

impl SessionStore for FileBackedStore {
    fn create(&self) -> Result<SessionId, ModuleError> {
        let sid = uuid::Uuid::new_v4().to_string(); // uuid already satisfies the charset guard
        lock_guard(&self.sessions).insert(sid.clone(), vec![]);
        lock_guard(&self.statuses).insert(sid.clone(), SessionStatus::Active);
        let now = now_epoch_secs();
        lock_guard(&self.created).insert(sid.clone(), now);
        self.persist(&sid)?;
        Ok(sid)
    }

    fn append(&self, sid: &SessionId, message: ChatMessage) -> Result<(), ModuleError> {
        self.require_active(sid)?;
        lock_guard(&self.sessions)
            .entry(sid.clone())
            .or_default()
            .push(message);
        self.persist(sid)
    }

    fn get(&self, sid: &SessionId) -> Result<Vec<ChatMessage>, ModuleError> {
        self.require_active(sid)?;
        Ok(lock_guard(&self.sessions)
            .get(sid)
            .cloned()
            .unwrap_or_default())
    }

    fn estimate_tokens(&self, sid: &SessionId) -> Result<usize, ModuleError> {
        let messages = self.get(sid)?;
        Ok(messages.iter().map(|m| m.content.len()).sum::<usize>() / 4)
    }

    fn status(&self, sid: &SessionId) -> Result<SessionStatus, ModuleError> {
        lock_guard(&self.statuses)
            .get(sid)
            .copied()
            .ok_or_else(|| ModuleError::Session(SessionError::NotFound(sid.clone())))
    }

    fn archive(&self, sid: &SessionId) -> Result<(), ModuleError> {
        if !lock_guard(&self.statuses).contains_key(sid) {
            return Err(ModuleError::Session(SessionError::NotFound(sid.clone())));
        }
        lock_guard(&self.statuses).insert(sid.clone(), SessionStatus::Archived);
        self.persist(sid)
    }

    fn delete(&self, sid: &SessionId) -> Result<(), ModuleError> {
        match lock_guard(&self.statuses).get(sid).copied() {
            None => return Err(ModuleError::Session(SessionError::NotFound(sid.clone()))),
            Some(SessionStatus::Deleted) => return Ok(()), // idempotent, matches InMemoryStore
            Some(_) => {}
        }
        lock_guard(&self.statuses).insert(sid.clone(), SessionStatus::Deleted);
        lock_guard(&self.sessions).remove(sid);
        match fs::remove_file(self.path_for(sid)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(persistence_err(e.to_string())),
        }
    }

    fn snapshot(&self, sid: &SessionId) -> Result<SessionSnapshot, ModuleError> {
        let status = lock_guard(&self.statuses)
            .get(sid)
            .copied()
            .ok_or_else(|| ModuleError::Session(SessionError::NotFound(sid.clone())))?;
        if status == SessionStatus::Deleted {
            return Err(ModuleError::Session(SessionError::NotFound(sid.clone())));
        }
        let messages = lock_guard(&self.sessions)
            .get(sid)
            .cloned()
            .unwrap_or_default();
        let now = now_epoch_secs();
        Ok(SessionSnapshot {
            session_id: sid.clone(),
            status,
            messages,
            created_at: lock_guard(&self.created).get(sid).copied().unwrap_or(now),
            updated_at: lock_guard(&self.updated).get(sid).copied().unwrap_or(now),
        })
    }

    fn restore(&self, snap: &SessionSnapshot) -> Result<(), ModuleError> {
        if !is_valid_sid(&snap.session_id) {
            return Err(persistence_err(format!(
                "session id '{}' has characters outside [A-Za-z0-9_-]",
                snap.session_id
            )));
        }
        lock_guard(&self.statuses).insert(snap.session_id.clone(), snap.status);
        lock_guard(&self.sessions).insert(snap.session_id.clone(), snap.messages.clone());
        lock_guard(&self.created).insert(snap.session_id.clone(), snap.created_at);
        lock_guard(&self.updated).insert(snap.session_id.clone(), snap.updated_at);
        self.persist(&snap.session_id)
    }

    fn load_all(&self) -> Result<Vec<SessionSnapshot>, ModuleError> {
        let sids: Vec<SessionId> = lock_guard(&self.sessions).keys().cloned().collect();
        sids.into_iter()
            .map(|sid| self.snapshot(&sid))
            .collect::<Result<Vec<_>, _>>()
    }

    fn scan_expired(
        &self,
        ttl: std::time::Duration,
        now_epoch: u64,
        bus: &agentj_core::hooks::EventBus,
    ) -> Result<usize, ModuleError> {
        let ledger: Vec<(SessionId, u64)> = lock_guard(&self.updated)
            .iter()
            .map(|(sid, t)| (sid.clone(), *t))
            .collect();
        let mut expired = Vec::new();
        for (sid, last) in ledger {
            let active = matches!(
                lock_guard(&self.statuses).get(&sid),
                Some(SessionStatus::Active)
            );
            if now_epoch.saturating_sub(last) > ttl.as_secs() && active {
                expired.push(sid);
            }
        }
        for sid in &expired {
            self.archive(sid)?;
            bus.publish(&agentj_core::module::EventType::SessionExpired(
                sid.clone(),
            ));
        }
        Ok(expired.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::{ModuleError, SessionError};
    use agentj_core::module::SessionStatus;
    use agentj_core::types::ChatMessage;

    fn dir() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap()
    }

    #[test]
    fn append_persists_and_restart_recovers() -> Result<(), ModuleError> {
        let d = dir();
        let path = d.path().to_path_buf();
        let store = FileBackedStore::new(&path)?;
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hello"))?;
        drop(store);

        let reloaded = FileBackedStore::new(&path)?;
        let all = reloaded.load_all()?;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].session_id, sid);
        assert_eq!(all[0].messages.len(), 1);
        Ok(())
    }

    #[test]
    fn archive_and_delete_update_disk() -> Result<(), ModuleError> {
        let d = dir();
        let path = d.path().to_path_buf();
        let store = FileBackedStore::new(&path)?;
        let sid = store.create()?;
        store.archive(&sid)?;
        drop(store);
        let reloaded = FileBackedStore::new(&path)?;
        assert_eq!(reloaded.status(&sid)?, SessionStatus::Archived);
        reloaded.delete(&sid)?;
        drop(reloaded);
        let empty = FileBackedStore::new(&path)?;
        assert!(empty.load_all()?.is_empty());
        Ok(())
    }

    #[test]
    fn corrupt_file_skipped_with_backup() {
        let d = dir();
        let sessions = d.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("bad.json"), "{not json").unwrap();
        let store = FileBackedStore::new(&sessions).unwrap();
        assert!(store.load_all().unwrap().is_empty());
        assert!(
            sessions.join("bad.json.corrupt").exists(),
            "corrupt file must be backed up, not lost"
        );
    }

    #[test]
    fn delete_is_idempotent_like_inmemory() -> Result<(), ModuleError> {
        let d = dir();
        let path = d.path().to_path_buf();
        let store = FileBackedStore::new(&path)?;
        let sid = store.create()?;

        store.delete(&sid)?;
        assert!(!path.join(format!("{sid}.json")).exists());

        // BUG-013 regression: second delete must be Ok (idempotent, matching
        // InMemoryStore) — NOT PersistenceFailed from fs::remove_file ENOENT
        store.delete(&sid)?;
        assert_eq!(store.status(&sid)?, SessionStatus::Deleted);
        Ok(())
    }

    #[test]
    fn delete_missing_session_returns_not_found() -> Result<(), ModuleError> {
        let d = dir();
        let store = FileBackedStore::new(d.path())?;
        assert!(matches!(
            store.delete(&"nonexistent".into()),
            Err(ModuleError::Session(SessionError::NotFound(_)))
        ));
        Ok(())
    }

    #[test]
    fn illegal_session_id_rejected() -> Result<(), ModuleError> {
        let d = dir();
        let store = FileBackedStore::new(d.path())?;
        let snap = SessionSnapshot {
            session_id: "../evil".into(),
            status: SessionStatus::Active,
            messages: vec![],
            created_at: 0,
            updated_at: 0,
        };
        assert!(matches!(
            store.restore(&snap),
            Err(ModuleError::Session(SessionError::PersistenceFailed(_)))
        ));
        Ok(())
    }

    #[test]
    fn leftover_tmp_cleaned_on_construction() {
        let d = dir();
        let sessions = d.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("stale.json.tmp"), "partial").unwrap();
        let store = FileBackedStore::new(&sessions).unwrap();
        assert!(store.load_all().unwrap().is_empty());
        assert!(
            !sessions.join("stale.json.tmp").exists(),
            "crash leftovers must be cleaned up"
        );
    }

    #[test]
    fn file_store_scan_expired_archives_stale() -> Result<(), ModuleError> {
        use agentj_core::hooks::EventBus;
        use std::sync::Arc;
        use std::time::Duration;
        let d = dir();
        let store = Arc::new(FileBackedStore::new(d.path())?);
        let sid = store.create()?;
        {
            let mut u = lock_guard(&store.updated);
            u.insert(sid.clone(), 90);
        }
        let bus = EventBus::new();
        let n = store.scan_expired(Duration::from_secs(5), 100, &bus)?;
        assert_eq!(n, 1);
        assert_eq!(store.status(&sid)?, SessionStatus::Archived);
        Ok(())
    }

    #[test]
    fn file_store_scan_expired_keeps_fresh() -> Result<(), ModuleError> {
        use agentj_core::hooks::EventBus;
        use std::time::Duration;
        let d = dir();
        let store = FileBackedStore::new(d.path())?;
        let sid = store.create()?;
        let n = store.scan_expired(Duration::from_secs(60), 0, &EventBus::new())?;
        assert_eq!(n, 0);
        assert_eq!(store.status(&sid)?, SessionStatus::Active);
        Ok(())
    }

    #[test]
    fn file_store_scan_expired_skips_archived() -> Result<(), ModuleError> {
        use agentj_core::hooks::EventBus;
        use std::sync::Arc;
        use std::time::Duration;
        let d = dir();
        let store = Arc::new(FileBackedStore::new(d.path())?);
        let sid = store.create()?;
        store.archive(&sid)?;
        {
            let mut u = lock_guard(&store.updated);
            u.insert(sid.clone(), 0);
        }
        let bus = EventBus::new();
        let n = store.scan_expired(Duration::from_secs(5), 100, &bus)?;
        assert_eq!(n, 0);
        assert_eq!(store.status(&sid)?, SessionStatus::Archived);
        Ok(())
    }

    #[test]
    fn persist_uses_unique_tmp_and_leaves_no_fixed_tmp() -> Result<(), ModuleError> {
        let d = dir();
        let path = d.path().to_path_buf();
        let store = FileBackedStore::new(&path)?;
        let sid = store.create()?;
        store.append(&sid, ChatMessage::user("hi"))?;
        assert!(
            !path.join(format!("{sid}.json.tmp")).exists(),
            "fixed-name tmp must not remain after persist"
        );
        assert!(
            path.join(format!("{sid}.json")).exists(),
            "final sid.json must exist after persist"
        );
        Ok(())
    }

    #[test]
    fn concurrent_stores_do_not_clobber_tmp() -> Result<(), ModuleError> {
        let d = dir();
        let path = d.path().to_path_buf();
        let a = FileBackedStore::new(&path)?;
        let b = FileBackedStore::new(&path)?;
        let sid = a.create()?;
        a.append(&sid, ChatMessage::user("from-a"))?;
        b.append(&sid, ChatMessage::user("from-b"))?;
        let snap = a.load_all()?;
        assert_eq!(snap.len(), 1);
        let reloaded = FileBackedStore::new(&path)?;
        assert_eq!(reloaded.load_all()?.len(), 1);
        Ok(())
    }
}
