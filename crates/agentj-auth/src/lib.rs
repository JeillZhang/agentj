//! Credential storage for subscription providers (design D1/D2).
//!
//! Phase 1 supports workspace API keys only; the on-disk format is frozen with an
//! `OAuth` variant reserved for phase 2.

pub mod credential;
pub mod resolve;
pub mod store;

pub use credential::{AuthError, AuthFile, Credential, redact};
pub use resolve::{KeySource, ResolvedKey, resolve_credential};
pub use store::AuthStore;
