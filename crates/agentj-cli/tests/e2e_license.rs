use std::process::{Command, Stdio};

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};

fn signed_key(entitlements: &[&str]) -> String {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let payload = serde_json::json!({"entitlements": entitlements, "expires_at": 0}).to_string();
    let sig = signing.sign(payload.as_bytes());
    format!(
        "{}.{}",
        base64::engine::general_purpose::STANDARD.encode(payload.as_bytes()),
        base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
    )
}

fn pubkey_b64() -> String {
    base64::engine::general_purpose::STANDARD.encode(
        SigningKey::from_bytes(&[7u8; 32])
            .verifying_key()
            .to_bytes(),
    )
}

fn temp_config() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agentj-e2e-license-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_dir = dir.join("sessions");
    std::fs::create_dir_all(&session_dir).unwrap();
    let cfg_path = dir.join("config.toml");
    let base = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/license_run.toml"
    ))
    .unwrap();
    let content = format!("{base}\n[session]\ndir = \"{}\"\n", session_dir.display());
    std::fs::write(&cfg_path, content).unwrap();
    cfg_path
}

fn run_agentj(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn free_mode_run_succeeds_without_key() {
    let cfg = temp_config();
    let out = run_agentj(&["--config", cfg.to_str().unwrap(), "run", "hi"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("done"));
}

#[test]
fn empty_license_key_flag_treated_as_free_mode() {
    let cfg = temp_config();
    let out = run_agentj(&[
        "--config",
        cfg.to_str().unwrap(),
        "--license-key",
        "",
        "run",
        "hi",
    ]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn empty_license_key_env_treated_as_free_mode() {
    let cfg = temp_config();
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_KEY", "")
        .args(["--config", cfg.to_str().unwrap(), "run", "hi"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn invalid_key_exits_nonzero_with_license_error() {
    let cfg = temp_config();
    let out = run_agentj(&[
        "--config",
        cfg.to_str().unwrap(),
        "--license-key",
        "not-a-key",
        "run",
        "hi",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("license"));
}

#[test]
fn key_without_entitlement_exits_nonzero() {
    let cfg = temp_config();
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_PUBKEY", pubkey_b64())
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--license-key",
            &signed_key(&["dashboard"]),
            "run",
            "hi",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("entitlement"));
}

#[test]
fn key_with_entitlement_run_succeeds() {
    let cfg = temp_config();
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_PUBKEY", pubkey_b64())
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--license-key",
            &signed_key(&["run"]),
            "run",
            "hi",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
