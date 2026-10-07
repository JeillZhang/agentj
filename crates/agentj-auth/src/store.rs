//! Credential store: XDG path resolution, `0600` atomic save, permission-checked load.

use std::path::{Path, PathBuf};

use crate::credential::{AuthError, AuthFile};

/// Reads/writes `$XDG_DATA_HOME/agentj/auth.json` (fallback `~/.local/share`).
#[derive(Debug, Clone)]
pub struct AuthStore {
    path: PathBuf,
}

impl AuthStore {
    /// Resolve the default credential path, or fail if no home directory exists.
    pub fn with_default_path() -> Result<Self, AuthError> {
        let base = match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(v),
            None => PathBuf::from(std::env::var_os("HOME").ok_or(AuthError::NoHomeDir)?)
                .join(".local/share"),
        };
        Ok(Self {
            path: base.join("agentj/auth.json"),
        })
    }

    /// Explicit path (tests and `--config`-style overrides).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AuthFile, AuthError> {
        if !self.path.exists() {
            return Ok(AuthFile::new());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&self.path)
                .map_err(io_err(&self.path))?
                .permissions()
                .mode()
                & 0o777;
            if mode & 0o077 != 0 {
                return Err(AuthError::InsecurePermissions {
                    path: self.path.display().to_string(),
                    mode,
                });
            }
        }
        let text = std::fs::read_to_string(&self.path).map_err(io_err(&self.path))?;
        crate::credential::parse_auth_file(&text, &self.path.display().to_string())
    }

    pub fn save(&self, file: &AuthFile) -> Result<(), AuthError> {
        let dir = self.path.parent().expect("auth.json has a parent");
        std::fs::create_dir_all(dir).map_err(io_err(dir))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(io_err(dir))?;
        }
        let tmp = dir.join("auth.json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(io_err(&tmp))?;
        let body = serde_json::to_string_pretty(file).expect("credential map serializes");
        std::io::Write::write_all(&mut f, body.as_bytes()).map_err(io_err(&tmp))?;
        f.sync_all().map_err(io_err(&tmp))?;
        drop(f);
        std::fs::rename(&tmp, &self.path).map_err(io_err(&self.path))
    }

    pub fn get(&self, provider: &str) -> Result<Option<crate::credential::Credential>, AuthError> {
        Ok(self.load()?.get(provider).cloned())
    }

    pub fn set(
        &self,
        provider: &str,
        cred: crate::credential::Credential,
    ) -> Result<(), AuthError> {
        let mut file = self.load()?;
        file.insert(provider.into(), cred);
        self.save(&file)
    }

    pub fn remove(&self, provider: &str) -> Result<bool, AuthError> {
        let mut file = self.load()?;
        let removed = file.remove(provider).is_some();
        if file.is_empty() {
            if self.path.exists() {
                std::fs::remove_file(&self.path).map_err(io_err(&self.path))?;
            }
        } else {
            self.save(&file)?;
        }
        Ok(removed)
    }
}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> AuthError + '_ {
    move |e| AuthError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::Credential;

    fn store(dir: &std::path::Path) -> AuthStore {
        AuthStore::with_path(dir.join("auth.json"))
    }

    #[test]
    fn save_writes_0600_file_in_0700_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = crate::credential::AuthFile::new();
        file.insert(
            "opencode-go".into(),
            Credential::ApiKey {
                key: "sk-1".into(),
                label: None,
                created_at: None,
            },
        );
        store(dir.path()).save(&file).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(dir.path().join("auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            let dir_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600);
            assert_eq!(dir_mode, 0o700);
        }
    }

    #[test]
    fn load_rejects_group_readable_file() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("auth.json");
            std::fs::write(&path, "{}").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                store(dir.path()).load(),
                Err(AuthError::InsecurePermissions { .. })
            ));
        }
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path()).load().unwrap().is_empty());
    }

    #[test]
    fn remove_last_entry_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        s.set(
            "opencode-go",
            Credential::ApiKey {
                key: "sk-1".into(),
                label: None,
                created_at: None,
            },
        )
        .unwrap();
        assert!(s.remove("opencode-go").unwrap());
        assert!(!dir.path().join("auth.json").exists());
    }

    #[test]
    fn malformed_json_reports_the_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, "{").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let err = store(dir.path()).load().unwrap_err();
        assert!(matches!(err, AuthError::Malformed { ref path, .. } if path.contains("auth.json")));
    }
}
