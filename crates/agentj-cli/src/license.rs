//! License gate: resolve the key (flag > env > config), validate the Ed25519
//! signature, and check the per-command entitlement (design D1/D2/D5/D6).

use agentj_license::LicenseValidator;
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;

/// Production verifying key, embedded at build time. The placeholder value is
/// replaced when the commercial key ships; AGENTJ_LICENSE_PUBKEY overrides it
/// for tests and operator-driven rotation (D5).
const AGENTJ_LICENSE_PUBKEY: [u8; 32] = [0u8; 32];

pub fn resolve_validator() -> Result<LicenseValidator, anyhow::Error> {
    let key_bytes: [u8; 32] = match std::env::var("AGENTJ_LICENSE_PUBKEY") {
        Ok(b64) if !b64.is_empty() => {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .map_err(|e| anyhow::anyhow!("license pubkey override invalid base64: {e}"))?;
            raw.as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("license pubkey override must be 32 bytes"))?
        }
        _ => AGENTJ_LICENSE_PUBKEY,
    };
    let vk = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| anyhow::anyhow!("license pubkey invalid: {e}"))?;
    Ok(LicenseValidator::new(vk))
}

pub fn check_license_gate(
    cfg: &agentj_config::Config,
    cli_key: Option<&str>,
    entitlement: &str,
    command_name: &str,
) -> Result<(), anyhow::Error> {
    // D6: flag > env (already folded into config by apply_env) > [license] key.
    let key = cli_key.or_else(|| cfg.inner().license.as_ref().and_then(|l| l.key.as_deref()));
    let Some(key) = key else {
        return Ok(());
    }; // D1: no key = free mode
    let validator = resolve_validator()?;
    let license = validator
        .validate(key)
        .map_err(|e| anyhow::anyhow!("license: {e}"))?;
    if !validator.check_entitlement(&license, entitlement) {
        anyhow::bail!("license missing entitlement: {command_name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::OnceLock;

    static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn signed_key(entitlements: &[&str]) -> String {
        let signing = signing_key();
        let payload =
            serde_json::json!({"entitlements": entitlements, "expires_at": 0}).to_string();
        let sig = signing.sign(payload.as_bytes());
        format!(
            "{}.{}",
            base64::engine::general_purpose::STANDARD.encode(payload.as_bytes()),
            base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
        )
    }

    fn cfg_with(toml: &str) -> agentj_config::Config {
        let dir = std::env::temp_dir().join(format!(
            "agentj-cli-lic-{:?}-{}",
            std::thread::current().id(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.toml");
        std::fs::write(&path, toml).unwrap();
        agentj_config::Config::load(Some(path)).unwrap()
    }

    fn set_test_pubkey() {
        unsafe {
            std::env::set_var(
                "AGENTJ_LICENSE_PUBKEY",
                base64::engine::general_purpose::STANDARD
                    .encode(signing_key().verifying_key().to_bytes()),
            );
        }
    }

    #[test]
    fn free_mode_without_key_is_ok() {
        let cfg = cfg_with("");
        assert!(check_license_gate(&cfg, None, "run", "run").is_ok());
    }

    #[test]
    fn invalid_key_rejected() {
        let _g = env_lock();
        set_test_pubkey();
        let cfg = cfg_with("");
        let err = check_license_gate(&cfg, Some("not-a-key"), "run", "run").unwrap_err();
        unsafe { std::env::remove_var("AGENTJ_LICENSE_PUBKEY") };
        assert!(err.to_string().contains("license"));
    }

    #[test]
    fn missing_entitlement_rejected() {
        let _g = env_lock();
        set_test_pubkey();
        let key = signed_key(&["dashboard"]);
        let cfg = cfg_with("");
        let err = check_license_gate(&cfg, Some(&key), "run", "run").unwrap_err();
        unsafe { std::env::remove_var("AGENTJ_LICENSE_PUBKEY") };
        assert!(err.to_string().contains("entitlement"));
    }

    #[test]
    fn valid_entitlement_allowed() {
        let _g = env_lock();
        set_test_pubkey();
        let key = signed_key(&["run"]);
        let cfg = cfg_with("");
        let res = check_license_gate(&cfg, Some(&key), "run", "run");
        unsafe { std::env::remove_var("AGENTJ_LICENSE_PUBKEY") };
        assert!(res.is_ok());
    }
}
