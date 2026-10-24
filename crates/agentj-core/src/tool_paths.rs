//! Relative-path rebasing for file tools (design 2026-09-13 §3, 决策 #3):
//! file-tool arguments are rebased onto the per-session workspace root unless
//! already absolute. Centralised here so `ToolRegistry::execute_async` can
//! apply it uniformly before dispatching to `Tool::execute_with_context`.
//!
//! Rebase only when the path really lands inside the root (`paths::resolve_within_root`).
//! An escaping argument is handed to the permission layer exactly as the model
//! spelled it, so the escape is still visible there; baking
//! `<root>/../../etc/passwd` into the args would turn a Deny into a plausible
//! looking absolute path.

use std::path::Path;

use crate::paths::{Resolved, resolve_within_root};

/// Rebase a tool's path-like argument onto `root` when it is a relative string.
///
/// Mapping: `read`/`write`/`grep` → `path`. `glob` arguments pass through
/// untouched: rewriting the pattern into an absolute path regressed the tool
/// (`find -name` matches basenames only), so `GlobTool` resolves the session
/// root itself via `ToolCallContext.workspace_root` instead. Tools without a
/// path argument (`bash`, ...), absolute paths, and missing keys pass through
/// untouched.
pub fn rebase_tool_args(name: &str, args: serde_json::Value, root: &Path) -> serde_json::Value {
    let key = match name {
        "read" | "write" | "grep" => "path",
        _ => return args,
    };
    let Some(rel) = args.get(key).and_then(serde_json::Value::as_str) else {
        return args;
    };
    if Path::new(rel).is_absolute() {
        return args;
    }
    let Resolved::Inside(target) = resolve_within_root(root, rel) else {
        return args;
    };
    let mut rebased = args;
    if let Some(obj) = rebased.as_object_mut() {
        obj.insert(
            key.to_string(),
            serde_json::Value::String(target.to_string_lossy().into_owned()),
        );
    }
    rebased
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_relative_paths_under_root() {
        // The root must be a real, readable directory: rebasing now asks
        // `resolve_within_root`, which fails closed for unverifiable roots.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let args = serde_json::json!({"path": "src/main.rs"});
        let out = rebase_tool_args("read", args, &root);
        assert_eq!(
            out["path"],
            serde_json::Value::String(root.join("src/main.rs").to_string_lossy().into_owned())
        );

        // Absolute paths are left untouched.
        let args = serde_json::json!({"path": "/other/x"});
        let out = rebase_tool_args("write", args, Path::new("/root"));
        assert_eq!(out["path"], "/other/x");

        // Non-path tools (bash command) are unchanged.
        let args = serde_json::json!({"command": "ls -la"});
        let out = rebase_tool_args("bash", args.clone(), Path::new("/root"));
        assert_eq!(out, args);

        // glob patterns are intentionally NOT rewritten: the tool scopes
        // itself with the session root as cwd (`find . -name <pattern>`).
        // Rewriting to `/root/**/*.rs` used to break `find -name` (basename
        // matching only) and silently return empty for every session.
        let args = serde_json::json!({"pattern": "**/*.rs"});
        let out = rebase_tool_args("glob", args.clone(), Path::new("/root"));
        assert_eq!(out, args);
    }

    /// An escaping relative path is left exactly as the caller spelled it: the
    /// permission layer produces the Deny. Rewriting it into
    /// `<root>/../../etc/passwd` hid the escape from every later check.
    #[test]
    fn escaping_paths_are_not_rewritten() {
        let args = serde_json::json!({"path": "../../etc/passwd"});
        let out = rebase_tool_args("write", args.clone(), Path::new("/tmp/ws"));
        assert_eq!(out, args);
    }

    #[test]
    fn inside_paths_become_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let out = rebase_tool_args("read", serde_json::json!({"path": "src/main.rs"}), &root);
        assert_eq!(
            out["path"],
            serde_json::Value::String(root.join("src/main.rs").to_string_lossy().into_owned())
        );
    }

    #[test]
    fn missing_or_non_string_keys_pass_through() {
        let args = serde_json::json!({});
        assert_eq!(
            rebase_tool_args("read", args.clone(), Path::new("/root")),
            args
        );
        let args = serde_json::json!({"path": 7});
        assert_eq!(
            rebase_tool_args("read", args.clone(), Path::new("/root")),
            args
        );
    }
}
