//! AgentJ kernel — shared types, traits, and event loop.

pub mod types;
pub mod error;
pub mod module;

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
