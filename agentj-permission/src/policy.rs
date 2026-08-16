//! Tool policy model: per-tool default mode + path/command rules (design #17).
use std::collections::HashMap;

use crate::error::PermissionError;
use serde::{Deserialize, Serialize};

/// Per-tool default mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Allow,
    Ask,
    Deny,
}

/// Rule effect (priority Deny > Allow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionEffect {
    Allow,
    Deny,
}

/// Path rule for file tools (write/read/glob/grep); glob relative to workspace root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathRule {
    pub effect: PermissionEffect,
    pub pattern: String,
}

/// Command rule for bash; prefix matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandRule {
    pub effect: PermissionEffect,
    pub prefix: String,
}

/// Single-tool policy: default mode + rule tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPolicy {
    #[serde(default = "default_mode_ask")]
    pub mode: PermissionMode,
    #[serde(default)]
    pub path_rules: Vec<PathRule>,
    #[serde(default)]
    pub command_rules: Vec<CommandRule>,
}

fn default_mode_ask() -> PermissionMode {
    PermissionMode::Ask
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        }
    }
}

/// Design 决策 #17 factory defaults: bash=Ask, write=Ask, read/glob/grep=Allow,
/// everything else (MCP/plugin/unknown) = Ask (conservative).
pub fn builtin_default_mode(tool: &str) -> PermissionMode {
    match tool {
        "read" | "glob" | "grep" => PermissionMode::Allow,
        _ => PermissionMode::Ask,
    }
}

/// Top-level [permission] section: per-tool policies keyed by tool name.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionConfig {
    #[serde(default)]
    pub tools: HashMap<String, ToolPolicy>,
}

impl PermissionConfig {
    /// Validate all rule patterns compile as globs (fail-fast on config load).
    pub fn validate(&self) -> Result<(), PermissionError> {
        for (tool, policy) in &self.tools {
            for rule in &policy.path_rules {
                globset::Glob::new(&rule.pattern).map_err(|e| PermissionError::InvalidRule {
                    tool: tool.clone(),
                    reason: format!("invalid glob '{}': {e}", rule.pattern),
                })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_policy_default_mode_is_ask() {
        let p = ToolPolicy::default();
        assert_eq!(p.mode, PermissionMode::Ask);
        assert!(p.path_rules.is_empty());
        assert!(p.command_rules.is_empty());
    }

    #[test]
    fn path_rule_constructs() {
        let r = PathRule {
            effect: PermissionEffect::Deny,
            pattern: "../**".into(),
        };
        assert_eq!(r.effect, PermissionEffect::Deny);
        assert_eq!(r.pattern, "../**");
    }

    #[test]
    fn command_rule_constructs() {
        let r = CommandRule {
            effect: PermissionEffect::Allow,
            prefix: "npm test".into(),
        };
        assert_eq!(r.effect, PermissionEffect::Allow);
        assert_eq!(r.prefix, "npm test");
    }

    #[test]
    fn builtin_default_modes_match_design() {
        use crate::policy::PermissionMode::*;
        assert_eq!(builtin_default_mode("bash"), Ask);
        assert_eq!(builtin_default_mode("write"), Ask);
        assert_eq!(builtin_default_mode("read"), Allow);
        assert_eq!(builtin_default_mode("glob"), Allow);
        assert_eq!(builtin_default_mode("grep"), Allow);
        assert_eq!(builtin_default_mode("unknown_tool"), Ask); // conservative unknown
    }
}
