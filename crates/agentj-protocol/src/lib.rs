//! AgentJ IPC protocol contract — canonical wire types + schema generation.

/// Canonical wire protocol version — single source of truth for the `initialize`
/// handshake `protocol_version` field (mirrors `protocol/methods.json` + sdk `config.ts`).
pub const PROTOCOL_VERSION: &str = "0.14.0";

pub mod base;
pub mod file;
pub mod lifecycle;
pub mod permission;
pub mod question;
pub mod schema;
pub mod session;
pub mod streaming;
pub mod trajectory;
pub mod types;

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
