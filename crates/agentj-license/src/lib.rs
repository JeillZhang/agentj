//! License validation for AgentJ (Ed25519-signed license keys).
//!
//! [design-pending: key format, payload schema, expiry policy land with the v0.4 design.]

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("license key is invalid")]
    InvalidKey,
    #[error("license expired")]
    Expired,
    #[error("license payload malformed: {0}")]
    Malformed(String),
}

#[derive(Debug, Clone)]
pub struct License {
    pub entitlements: Vec<String>,
    pub expires_at: u64, // unix seconds; 0 = never
}

/// Ed25519-signed license key: `base64(payload_json).base64(signature)`.
/// [design-pending: key format, payload schema, expiry policy land with the v0.4 design.]
pub struct LicenseValidator {
    verifying_key: VerifyingKey,
}

impl LicenseValidator {
    pub fn new(verifying_key: VerifyingKey) -> Self {
        Self { verifying_key }
    }

    pub fn validate(&self, key: &str) -> Result<License, LicenseError> {
        let (payload_b64, sig_b64) = key.split_once('.').ok_or(LicenseError::InvalidKey)?;
        let payload = base64::engine::general_purpose::STANDARD
            .decode(payload_b64)
            .map_err(|_| LicenseError::InvalidKey)?;
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(sig_b64)
            .map_err(|_| LicenseError::InvalidKey)?;
        let signature = Signature::from_slice(&sig_bytes).map_err(|_| LicenseError::InvalidKey)?;
        self.verifying_key
            .verify(&payload, &signature)
            .map_err(|_| LicenseError::InvalidKey)?;

        let json: serde_json::Value = serde_json::from_slice(&payload)
            .map_err(|e| LicenseError::Malformed(e.to_string()))?;
        let entitlements = json["entitlements"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let expires_at = json["expires_at"].as_u64().unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| LicenseError::Malformed(e.to_string()))?
            .as_secs();
        if expires_at != 0 && now > expires_at {
            return Err(LicenseError::Expired);
        }
        Ok(License { entitlements, expires_at })
    }

    pub fn check_entitlement(&self, license: &License, feature: &str) -> bool {
        license.entitlements.iter().any(|e| e == feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn test_validator() -> LicenseValidator {
        LicenseValidator::new(test_signing_key().verifying_key())
    }

    fn test_signed_key(feature: &str, expires_at: u64) -> String {
        let signing = test_signing_key();
        let payload =
            serde_json::json!({"entitlements": [feature], "expires_at": expires_at}).to_string();
        let sig = signing.sign(payload.as_bytes());
        format!(
            "{}.{}",
            base64::engine::general_purpose::STANDARD.encode(payload.as_bytes()),
            base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
        )
    }

    #[test]
    fn valid_key_validates_and_exposes_entitlements() {
        let validator = test_validator();
        let key = test_signed_key("dashboard", now() + 1_000_000); // far future (offset, not year*secs)
        let license = validator.validate(&key).unwrap();
        assert!(license.entitlements.iter().any(|e| e.as_str() == "dashboard"));
        assert!(validator.check_entitlement(&license, "dashboard"));
        assert!(!validator.check_entitlement(&license, "billing"));
    }

    #[test]
    fn malformed_key_is_invalid() {
        let validator = test_validator();
        assert!(matches!(validator.validate("not-a-key"), Err(LicenseError::InvalidKey)));
    }

    #[test]
    fn expired_license_is_rejected() {
        let validator = test_validator();
        let key = test_signed_key("dashboard", now() - 1_000_000); // already past
        assert!(matches!(validator.validate(&key), Err(LicenseError::Expired)));
    }

    #[test]
    fn never_expiring_license_is_valid() {
        let validator = test_validator();
        let key = test_signed_key("dashboard", 0); // 0 = never expires
        assert!(validator.validate(&key).is_ok());
    }
}
