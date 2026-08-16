//! Decision engine: pure rule evaluation (design #17 — Deny > Allow > Mode).
//! Path rules match RELATIVE glob patterns (user-specified relative to the
//! workspace root). Escapes are detected lexically (no filesystem access):
//! resolve the argument against workspace_root, strip `..` segments, and if
//! the result leaves the root — Deny.

use globset::{Glob, GlobSetBuilder};
use std::path::{Component, Path, PathBuf};

use crate::policy::{PermissionEffect, PermissionMode, ToolPolicy};

/// Result of evaluating a single tool invocation against a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalOutcome {
    Allow,
    Deny,
    /// No rule hit — fall back to the tool's default mode.
    Mode(PermissionMode),
}

/// Stateless rule evaluator. (Design keeps it pure so it is trivially testable;
/// PermissionModule owns state like pending approvals and session allow-lists.)
#[derive(Debug, Default)]
pub struct PermissionEngine;

impl PermissionEngine {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a bash-style command against command rules (prefix match),
    /// then fall back to the tool's default mode.
    pub fn eval_command(&self, policy: &ToolPolicy, command: &str) -> EvalOutcome {
        let mut any_allow = false;
        for rule in &policy.command_rules {
            if command.starts_with(&rule.prefix) {
                match rule.effect {
                    PermissionEffect::Deny => return EvalOutcome::Deny,
                    PermissionEffect::Allow => any_allow = true,
                }
            }
        }
        if any_allow {
            EvalOutcome::Allow
        } else {
            EvalOutcome::Mode(policy.mode)
        }
    }

    /// Evaluate a file tool path argument against path rules.
    /// 1. Lexically resolve against workspace_root; if the result escapes the
    ///    root → Deny (unconditional — outside the workspace is never allowed).
    /// 2. Match the RELATIVE argument against relative glob patterns.
    ///    Deny > Allow > Mode.
    pub fn eval_path(&self, policy: &ToolPolicy, workspace_root: &Path, path: &str) -> EvalOutcome {
        if self.escapes_root(workspace_root, path) {
            return EvalOutcome::Deny;
        }
        let mut any_allow = false;
        for rule in &policy.path_rules {
            if self.glob_matches(&rule.pattern, path) {
                match rule.effect {
                    PermissionEffect::Deny => return EvalOutcome::Deny,
                    PermissionEffect::Allow => any_allow = true,
                }
            }
        }
        if any_allow {
            EvalOutcome::Allow
        } else {
            EvalOutcome::Mode(policy.mode)
        }
    }

    /// Lexical (no filesystem) check: does `arg` resolve outside `root`?
    fn escapes_root(&self, root: &Path, arg: &str) -> bool {
        let joined = root.join(arg);
        let normalized = lexical_normalize(&joined);
        !normalized.starts_with(root)
    }

    fn glob_matches(&self, pattern: &str, relative_path: &str) -> bool {
        // Patterns are relative globs (e.g. "src/**", ".env"); match against
        // the relative argument. Build a set per rule — rules are few.
        Glob::new(pattern)
            .and_then(|g| {
                let mut b = GlobSetBuilder::new();
                b.add(g);
                b.build()
            })
            .map(|set| set.is_match(relative_path))
            .unwrap_or(false)
    }
}

/// Lexically strip `.`/`..` components without touching the filesystem.
/// `/ws/a/../../x` → `/x`; `a/b/c` stays under `/ws`. (`..` at the root is
/// kept — the caller compares against `root` to decide escape.)
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::*;

    fn bash_policy() -> ToolPolicy {
        ToolPolicy {
            mode: PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![
                CommandRule {
                    effect: PermissionEffect::Allow,
                    prefix: "npm test".into(),
                },
                CommandRule {
                    effect: PermissionEffect::Deny,
                    prefix: "rm -rf".into(),
                },
            ],
        }
    }

    #[test]
    fn allow_rule_hit_returns_allow() {
        let engine = PermissionEngine::new();
        let cmd = "npm test -- --watch";
        assert_eq!(engine.eval_command(&bash_policy(), cmd), EvalOutcome::Allow);
    }

    #[test]
    fn deny_rule_short_circuits_over_allow() {
        let engine = PermissionEngine::new();
        // Even if an allow rule matched first, deny wins.
        let p = ToolPolicy {
            mode: PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![
                CommandRule {
                    effect: PermissionEffect::Allow,
                    prefix: "npm".into(),
                },
                CommandRule {
                    effect: PermissionEffect::Deny,
                    prefix: "npm run evil".into(),
                },
            ],
        };
        assert_eq!(engine.eval_command(&p, "npm run evil"), EvalOutcome::Deny);
    }

    #[test]
    fn unmatched_command_falls_back_to_mode() {
        let engine = PermissionEngine::new();
        let cmd = "curl http://example.com";
        assert_eq!(
            engine.eval_command(&bash_policy(), cmd),
            EvalOutcome::Mode(PermissionMode::Ask)
        );
    }

    #[test]
    fn prefix_match_is_literal_prefix_not_substring() {
        let engine = PermissionEngine::new();
        // Literal-prefix semantics: a rule "npm test" matches a command that
        // STARTS with it ("npm testx" → Allow), but NOT one where it appears
        // mid-string ("echo npm test" → fall back to mode).
        let p = ToolPolicy {
            mode: PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![CommandRule {
                effect: PermissionEffect::Allow,
                prefix: "npm test".into(),
            }],
        };
        assert_eq!(engine.eval_command(&p, "npm testx"), EvalOutcome::Allow);
        assert_eq!(
            engine.eval_command(&p, "echo npm test"),
            EvalOutcome::Mode(PermissionMode::Ask)
        );
    }

    fn write_policy() -> ToolPolicy {
        ToolPolicy {
            mode: PermissionMode::Ask,
            path_rules: vec![
                PathRule {
                    effect: PermissionEffect::Allow,
                    pattern: "src/**".into(),
                },
                PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: "../**".into(),
                },
                PathRule {
                    effect: PermissionEffect::Deny,
                    pattern: ".env".into(),
                },
            ],
            command_rules: vec![],
        }
    }

    #[test]
    fn relative_path_matches_relative_allow_glob() {
        let engine = PermissionEngine::new();
        let root = std::path::Path::new("/ws");
        let path = "src/main.rs"; // relative to workspace root
        assert_eq!(
            engine.eval_path(&write_policy(), root, path),
            EvalOutcome::Allow
        );
    }

    #[test]
    fn dotdot_escape_rejected_by_deny() {
        let engine = PermissionEngine::new();
        let root = std::path::Path::new("/ws");
        // Lexically escapes the workspace root — denied regardless of rules.
        let path = "../secret.key";
        assert_eq!(
            engine.eval_path(&write_policy(), root, path),
            EvalOutcome::Deny
        );
    }

    #[test]
    fn dotfile_deny_rule_matches() {
        let engine = PermissionEngine::new();
        let root = std::path::Path::new("/ws");
        assert_eq!(
            engine.eval_path(&write_policy(), root, ".env"),
            EvalOutcome::Deny
        );
    }

    #[test]
    fn unmatched_path_falls_back_to_mode() {
        let engine = PermissionEngine::new();
        let root = std::path::Path::new("/ws");
        assert_eq!(
            engine.eval_path(&write_policy(), root, "docs/x.md"),
            EvalOutcome::Mode(PermissionMode::Ask)
        );
    }

    #[test]
    fn path_escapes_workspace_detected() {
        let engine = PermissionEngine::new();
        // Even with no deny rule, an escape must be Deny (lexical, no fs access).
        let p = ToolPolicy::default(); // no rules, mode=Ask
        assert_eq!(
            engine.eval_path(&p, std::path::Path::new("/ws"), "../x"),
            EvalOutcome::Deny
        );
        assert_eq!(
            engine.eval_path(&p, std::path::Path::new("/ws"), "a/../../x"),
            EvalOutcome::Deny
        );
        assert_eq!(
            engine.eval_path(&p, std::path::Path::new("/ws"), "a/b/c"),
            EvalOutcome::Mode(PermissionMode::Ask)
        );
    }
}
