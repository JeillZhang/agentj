// crates/agentj-cli/tests/common/mod.rs
#![allow(dead_code)] // each test binary uses a subset

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub const AGENTJ_ENV_KEYS_TO_REMOVE: [&str; 3] = [
    "AGENTJ_OPENCODE_GO_API_KEY",
    "AGENTJ_E2E_KEYLESS",
    "AGENTJ_MISSING_KEY",
];

pub struct TempEnv {
    _dir: TempDir,
    data: PathBuf,
    config: PathBuf,
    home: PathBuf,
}

impl TempEnv {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let data = dir.path().join("data");
        let config = dir.path().join("config");
        let home = dir.path().join("home");
        for p in [&data, &config, &home] {
            std::fs::create_dir_all(p).expect("create env dir");
        }
        Self {
            _dir: dir,
            data,
            config,
            home,
        }
    }

    pub fn apply<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        for key in AGENTJ_ENV_KEYS_TO_REMOVE {
            cmd.env_remove(key);
        }
        cmd.env("XDG_DATA_HOME", &self.data)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("HOME", &self.home)
    }

    /// A pre-wired CLI command; T15's `run` tests build on this.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentj"));
        self.apply(&mut cmd);
        cmd
    }

    /// The data root `XDG_DATA_HOME` points at: the kernel writes `agentj/auth.json`
    /// and its metrics state files underneath it.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// Writes `agentj/config.toml` under the temp config root — a stable path for
    /// callers that only need the file to exist. The CLI itself loads its config
    /// from `--config <path>` (default `./config.toml`), not from `XDG_CONFIG_HOME`.
    pub fn write_config(&self, body: &str) {
        let dir = self.config.join("agentj");
        std::fs::create_dir_all(&dir).expect("config dir");
        std::fs::write(dir.join("config.toml"), body).expect("write config");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_env_sets_all_three_xdg_vars_and_home() {
        let env = TempEnv::new();
        let mut cmd = Command::new("true");
        env.apply(&mut cmd);
        let envs: std::collections::HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(|v| v.to_owned())))
            .collect();
        for key in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "HOME"] {
            let v = envs
                .get(std::ffi::OsStr::new(key))
                .expect(key)
                .as_ref()
                .expect("value set");
            assert!(
                PathBuf::from(v).is_dir(),
                "{key} must point at an existing dir"
            );
        }
    }

    #[test]
    fn temp_env_writes_a_config_under_xdg_config_home() {
        let env = TempEnv::new();
        env.write_config("[agent]\nmax_tool_result_chars = 2000\n");
        let written = std::fs::read_to_string(env.config.join("agentj/config.toml")).unwrap();
        assert!(written.contains("max_tool_result_chars = 2000"));
    }

    #[test]
    fn temp_env_removes_api_key_env() {
        let env = TempEnv::new();
        let mut cmd = Command::new("true");
        env.apply(&mut cmd);
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_owned())
            .collect();
        for key in AGENTJ_ENV_KEYS_TO_REMOVE {
            assert!(removed.iter().any(|k| k == key), "{key} must be removed");
        }
    }
}
