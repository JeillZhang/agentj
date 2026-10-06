//! E2E: `agentj auth login|status|logout` round-trip and the keyless startup failure.
//!
//! Integration tests are per-binary, so the temp-dir/config helpers are local copies
//! of the `e2e_cli.rs` patterns.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn e2e_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentj-e2e-auth-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_cfg(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, body).unwrap();
    path
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("XDG_DATA_HOME", home)
        .env("HOME", home)
        .env_remove("AGENTJ_MISSING_KEY")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn login_status_logout_roundtrip() {
    let home = e2e_temp_dir("roundtrip");
    let cfg = write_cfg(&home, "[model]\nprovider = \"opencode-go\"\n");
    let cfg_str = cfg.to_str().unwrap();

    let login = run(
        &home,
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

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join("agentj/auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "credential file must be 0600");
    }

    let status = run(&home, &["--config", cfg_str, "auth", "status", "--offline"]);
    assert!(status.status.success());
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(out.contains("…1234"), "status must show last 4: {out}");
    assert!(!out.contains("sk-test-1234"), "status must not leak the key: {out}");

    let logout = run(&home, &["--config", cfg_str, "auth", "logout"]);
    assert!(logout.status.success());
    assert!(
        !home.join("agentj/auth.json").exists(),
        "logout must delete the emptied credential file"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn keyless_real_provider_fails_startup() {
    let home = e2e_temp_dir("keyless");
    let cfg = write_cfg(
        &home,
        "[model]\nprovider = \"deepseek\"\n[[model.providers]]\nname = \"deepseek\"\n\
         kind = \"openai-compat\"\nbase_url = \"https://example.invalid\"\n\
         api_key_env = \"AGENTJ_MISSING_KEY\"\n",
    );
    let out = run(&home, &["--config", cfg.to_str().unwrap(), "serve"]);
    assert!(!out.status.success(), "keyless startup must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no usable model provider"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&home);
}
