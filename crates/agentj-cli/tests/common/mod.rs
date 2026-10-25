// crates/agentj-cli/tests/common/mod.rs
#![allow(dead_code)] // each test binary uses a subset

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
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
    /// Write a config body to a stable path and return it — `--config <path>` is how the
    /// CLI actually loads configuration (it does NOT read `XDG_CONFIG_HOME`).
    pub fn write_config_file(&self, body: &str) -> PathBuf {
        let path = self.config.join("e2e.toml");
        std::fs::write(&path, body).expect("write config file");
        path
    }

    pub fn write_config(&self, body: &str) {
        let dir = self.config.join("agentj");
        std::fs::create_dir_all(&dir).expect("config dir");
        std::fs::write(dir.join("config.toml"), body).expect("write config");
    }
}

/// Monotonic JSON-RPC request ids for one socket conversation.
#[derive(Default)]
pub struct RequestIds(u64);

impl RequestIds {
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// Read server stdout lines until the resident server prints `listening on
/// <addr>`; returns the address (panics on EOF / timeout / absence).
pub fn read_listening_addr<R: BufRead>(reader: &mut R) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.trim().strip_prefix("listening on ") {
                    return rest.trim().to_string();
                }
            }
            Err(_) => break,
        }
    }
    panic!("server did not print a listening address");
}

/// Read one NDJSON line from the server. Bounded by the socket's read timeout
/// (set by the caller): a stalled or closed server fails fast with the lines
/// seen so far instead of hanging the test.
pub fn read_socket_line(reader: &mut BufReader<TcpStream>, seen: &[String]) -> String {
    let mut line = String::new();
    let count = reader
        .read_line(&mut line)
        .unwrap_or_else(|e| panic!("timed out waiting for a server line ({e}); saw {seen:?}"));
    assert!(count > 0, "server closed the connection; saw {seen:?}");
    line.trim_end().to_string()
}

/// One NDJSON JSON-RPC client over the resident server's TCP socket. Reads skip
/// `$/progress` notifications until the response carrying the request id arrives.
pub struct SocketClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    ids: RequestIds,
}

impl SocketClient {
    pub fn connect(addr: &str) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        Self {
            writer: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
            ids: RequestIds::default(),
        }
    }

    /// Send one request and return its `result`; a JSON-RPC error fails the test.
    pub fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let response = self.request(method, params);
        if let Some(error) = response.get("error") {
            panic!("{method} failed: {error}");
        }
        response["result"].clone()
    }

    /// Send one request and return the whole response envelope (error included).
    pub fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.send(method, params);
        let mut seen: Vec<String> = Vec::new();
        loop {
            let line = read_socket_line(&mut self.reader, &seen);
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
            seen.push(line);
        }
    }

    /// Fire-and-forget: write the request, do not wait for its response.
    pub fn send(&mut self, method: &str, params: serde_json::Value) -> u64 {
        let id = self.ids.next();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.writer, "{request}").expect("write request");
        id
    }

    /// Read frames until `pred` matches (skipping the rest), bounded by the socket read
    /// timeout.
    pub fn next_frame_until(
        &mut self,
        pred: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let mut seen: Vec<String> = Vec::new();
        loop {
            let line = read_socket_line(&mut self.reader, &seen);
            let value: serde_json::Value = serde_json::from_str(&line).expect("ndjson frame");
            if pred(&value) {
                return value;
            }
            seen.push(line);
        }
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
    fn socket_client_request_ids_increase() {
        let mut ids = RequestIds::default();
        assert_eq!(ids.next(), 1);
        assert_eq!(ids.next(), 2);
        assert_eq!(ids.next(), 3);
    }

    #[test]
    fn write_config_file_returns_a_usable_path() {
        let env = TempEnv::new();
        let path = env.write_config_file("[model]\nprovider = \"mock\"\n");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[model]\nprovider = \"mock\"\n"
        );
        assert!(path.is_absolute());
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
