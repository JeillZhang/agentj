//! AgentJ kernel — shared types, traits, and event loop.

pub mod agent_loop_module;
pub mod artifacts;
pub mod budget;
pub mod dispatcher;
pub mod error;
pub mod git_sample;
pub mod hooks;
pub mod kernel;
pub mod module;
pub mod pending;
pub mod permission;
pub mod resolver;
pub mod sync;
pub mod text_tool_call;
pub mod tool_executor;
pub mod tool_paths;
pub mod turn_runner;
pub mod types;

pub use resolver::{KernelService, ModuleResolver, ServiceRegistry};

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(add(2, 2), 4);
    }
}
