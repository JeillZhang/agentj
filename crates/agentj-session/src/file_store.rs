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
}