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
    let out = run_agentj(&[
        "--config",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/license_run.toml"
        ),
        "run",
        "hi",
    ]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("done"));
}

#[test]
fn empty_license_key_flag_treated_as_free_mode() {
    let out = run_agentj(&[
        "--config",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/license_run.toml"
        ),
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
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_KEY", "")
        .args([
            "--config",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/license_run.toml"
            ),
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

#[test]
fn invalid_key_exits_nonzero_with_license_error() {
    let out = run_agentj(&[
        "--config",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/license_run.toml"
        ),
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
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_PUBKEY", pubkey_b64())
        .args([
            "--config",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/license_run.toml"
            ),
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
    let out = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .env("AGENTJ_LICENSE_PUBKEY", pubkey_b64())
        .args([
            "--config",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/license_run.toml"
            ),
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
