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
/// todo/ask=Allow (side-effect-free builtins, design 决策 #2), everything else
/// (MCP/plugin/unknown) = Ask (conservative).
pub fn builtin_default_mode(tool: &str) -> PermissionMode {
    match tool {
        "read" | "glob" | "grep" | "todo" | "ask" => PermissionMode::Allow,
        _ => PermissionMode::Ask,
    }
}

/// Top-level [permission] section: per-tool policies keyed by tool name.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionConfig {
    /// Approval wait budget in milliseconds before an `ask` auto-denies.
    /// `None` keeps the design default (60s); e2e fixtures lower it so the
    /// auto-deny path does not dominate a workspace test run. Signed, so an
    /// out-of-range negative (`-7`) deserializes and reaches the CLI clamp
    /// (100..=600_000) instead of failing the whole config parse.
    #[serde(default)]
    pub timeout_ms: Option<i64>,
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

    /// A negative `[permission] timeout_ms` must survive deserialization so the CLI
    /// clamp can report and clamp it; it must NOT fail the whole config parse.
    /// (TOML hands `-7` to serde as an i64 exactly like JSON does, so this pins the
    /// same u64-visitor rejection behind the reported `expected u64` TOML error.)
    #[test]
    fn timeout_ms_deserializes_a_negative_signed_value() {
        let parsed: Result<PermissionConfig, serde_json::Error> =
            serde_json::from_str(r#"{"timeout_ms": -7}"#);
        let cfg = parsed.unwrap_or_else(|e| {
            panic!("a negative timeout_ms must deserialize, not fail the parse: {e}")
        });
        // The raw signed value must survive so the CLI warning can print what the
        // user wrote (`-7 clamped to 100`), not a silently zeroed `0`.
        assert_eq!(cfg.timeout_ms.map(|ms| ms.to_string()), Some("-7".into()));
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
        assert_eq!(builtin_default_mode("todo"), Allow);
        assert_eq!(builtin_default_mode("ask"), Allow);
        assert_eq!(builtin_default_mode("unknown_tool"), Ask); // conservative unknown
    }
}
