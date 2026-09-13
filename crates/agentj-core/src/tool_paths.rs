//! Relative-path rebasing for file tools (design 2026-09-13 §3, 决策 #3):
//! file-tool arguments are rebased onto the per-session workspace root unless
//! already absolute. Centralised here so `ToolRegistry::execute_async` can
//! apply it uniformly before dispatching to `Tool::execute_with_context`.

use std::path::Path;

/// Rebase a tool's path-like argument onto `root` when it is a relative string.
///
/// Mapping: `read`/`write`/`grep` → `path`, `glob` → `pattern`. Tools without a
/// path argument (`bash`, ...), absolute paths, and missing keys pass through
/// untouched.
pub fn rebase_tool_args(name: &str, args: serde_json::Value, root: &Path) -> serde_json::Value {
    let key = match name {
        "read" | "write" | "grep" => "path",
        "glob" => "pattern",
        _ => return args,
    };
    let Some(rel) = args.get(key).and_then(serde_json::Value::as_str) else {
        return args;
    };
    if Path::new(rel).is_absolute() {
        return args;
    }
    let joined = root.join(rel).to_string_lossy().into_owned();
    let mut rebased = args;
    if let Some(obj) = rebased.as_object_mut() {
        obj.insert(key.to_string(), serde_json::Value::String(joined));
    }
    rebased
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_relative_paths_under_root() {
        let args = serde_json::json!({"path": "src/main.rs"});
        let out = rebase_tool_args("read", args, Path::new("/root"));
        assert_eq!(out["path"], "/root/src/main.rs");

        // Absolute paths are left untouched.
        let args = serde_json::json!({"path": "/other/x"});
        let out = rebase_tool_args("write", args, Path::new("/root"));
        assert_eq!(out["path"], "/other/x");

        // Non-path tools (bash command) are unchanged.
        let args = serde_json::json!({"command": "ls -la"});
        let out = rebase_tool_args("bash", args.clone(), Path::new("/root"));
        assert_eq!(out, args);

        // glob uses the "pattern" key.
        let args = serde_json::json!({"pattern": "**/*.rs"});
        let out = rebase_tool_args("glob", args, Path::new("/root"));
        assert_eq!(out["pattern"], "/root/**/*.rs");
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
