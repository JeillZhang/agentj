//! AgentJ IPC protocol contract — canonical wire types + schema generation.
pub mod base;
pub mod lifecycle;
pub mod permission;
pub mod schema;
pub mod session;
pub mod streaming;
pub mod types;

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
