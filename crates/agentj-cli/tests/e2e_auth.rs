//! E2E: `agentj auth login|status|logout` round-trip and the keyless startup failure.
//!
//! Integration tests are per-binary, so the temp-dir/config helpers are local copies
//! of the `e2e_cli.rs` patterns; every spawn runs inside a `common::TempEnv` so the
//! real credential store is never read or written.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;

fn e2e_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "agentj-e2e-auth-{tag}-{}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_cfg(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, body).unwrap();
    path
}

/// `agentj <args…>` with an isolated data/config/home root; API keys inherited from
/// the developer's shell are removed by `apply`.
fn run(env: &common::TempEnv, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentj"));
    env.apply(&mut cmd);
    cmd.args(args).output().unwrap()
}

#[test]
fn login_status_logout_roundtrip() {
    let env = common::TempEnv::new();
    let cfg_dir = e2e_temp_dir("roundtrip");
    let cfg = write_cfg(&cfg_dir, "[model]\nprovider = \"opencode-go\"\n");
    let cfg_str = cfg.to_str().unwrap();

    let login = run(
        &env,
        &[
            "--config",
            cfg_str,
            "auth",
            "login",
            "--api-key",
            "sk-test-1234",
            "--offline",
        ],
    );
    assert!(
        login.status.success(),
        "login failed: {}",
        String::from_utf8_lossy(&login.stderr)
    );

    let cred = env.data_dir().join("agentj/auth.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cred).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "credential file must be 0600");
    }

    let status = run(&env, &["--config", cfg_str, "auth", "status", "--offline"]);
    assert!(status.status.success());
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(out.contains("…1234"), "status must show last 4: {out}");
    assert!(
        !out.contains("sk-test-1234"),
        "status must not leak the key: {out}"
    );

    let logout = run(&env, &["--config", cfg_str, "auth", "logout"]);
    assert!(logout.status.success());
    assert!(
        !cred.exists(),
        "logout must delete the emptied credential file"
    );
    let _ = std::fs::remove_dir_all(&cfg_dir);
}

/// A keyless real provider makes one-shot `run` fail: there is no client to link a
/// credential into this kernel later, so the failure must land before any HTTP attempt.
#[test]
fn keyless_real_provider_fails_run() {
    let env = common::TempEnv::new();
    let cfg_dir = e2e_temp_dir("keyless");
    let cfg = write_cfg(
        &cfg_dir,
        "[model]\nprovider = \"deepseek\"\n[[model.providers]]\nname = \"deepseek\"\n\
         kind = \"openai-compat\"\nbase_url = \"https://example.invalid\"\n\
         api_key_env = \"AGENTJ_MISSING_KEY\"\n",
    );
    let out = run(&env, &["--config", cfg.to_str().unwrap(), "run", "hi"]);
    assert!(!out.status.success(), "keyless run must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no usable model provider"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("example.invalid"),
        "failure must happen before any HTTP attempt against base_url: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&cfg_dir);
}
