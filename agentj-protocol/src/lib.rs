//! AgentJ IPC protocol contract — canonical wire types + schema generation.
pub mod base;
pub mod types;
pub mod lifecycle;
pub mod session;
pub mod streaming;
pub mod schema;

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
