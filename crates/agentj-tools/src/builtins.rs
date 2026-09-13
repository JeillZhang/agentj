use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolCallContext};
use std::sync::Arc;

pub struct ReadTool;
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read file contents from the filesystem"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File path to read" } },
            "required": ["path"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "read".into(),
                reason: "missing 'path'".into(),
            })?;
        let content = std::fs::read_to_string(path).map_err(|e| ToolError::ExecutionFailed {
            name: "read".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String(content))
    }
}

pub struct WriteTool;
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }
    fn description(&self) -> &str {
        "Write content to a file"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
            "required": ["path", "content"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: "missing 'path'".into(),
            })?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: "missing 'content'".into(),
            })?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: e.to_string(),
            })?;
        }
        std::fs::write(path, content).map_err(|e| ToolError::ExecutionFailed {
            name: "write".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String("ok".into()))
    }
}

pub struct BashTool;

impl BashTool {
    fn command_arg(input: &serde_json::Value) -> Result<&str, ToolError> {
        input["command"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "bash".into(),
                reason: "missing 'command'".into(),
            })
    }

    fn run(command: &str, cwd: Option<&std::path::Path>) -> Result<serde_json::Value, ToolError> {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output().map_err(|e| ToolError::ExecutionFailed {
            name: "bash".into(),
            reason: e.to_string(),
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(serde_json::Value::String(stdout))
    }
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Execute a shell command"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string", "description": "Command to execute" } },
            "required": ["command"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Self::run(Self::command_arg(&input)?, None)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        Self::run(Self::command_arg(&input)?, Some(&ctx.workspace_root))
    }
}

pub struct GrepTool;

impl GrepTool {
    fn args(input: &serde_json::Value) -> (&str, &str) {
        (
            input["pattern"].as_str().unwrap_or(".*"),
            input["include"].as_str().unwrap_or("*"),
        )
    }

    fn run(
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<serde_json::Value, ToolError> {
        let mut cmd = std::process::Command::new("rg");
        cmd.arg("--no-heading")
            .arg("--with-filename")
            .arg(pattern)
            .arg("--glob")
            .arg(include);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output().map_err(|e| ToolError::ExecutionFailed {
            name: "grep".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search file contents with regex"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "include": { "type": "string", "description": "File pattern (e.g. *.rs)" }
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        Self::run(pattern, include, None)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        Self::run(pattern, include, Some(&ctx.workspace_root))
    }
}

pub struct GlobTool;

impl GlobTool {
    fn run(pattern: &str, cwd: Option<&std::path::Path>) -> Result<serde_json::Value, ToolError> {
        let mut cmd = std::process::Command::new("find");
        cmd.arg(".").arg("-name").arg(pattern).arg("-type").arg("f");
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output().map_err(|e| ToolError::ExecutionFailed {
            name: "glob".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "pattern": { "type": "string" } },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Self::run(input["pattern"].as_str().unwrap_or("*"), None)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        Self::run(
            input["pattern"].as_str().unwrap_or("*"),
            Some(&ctx.workspace_root),
        )
    }
}

pub struct TodoTool;

impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }
    fn description(&self) -> &str {
        "Submit the full TODO snapshot for the current task list (empty array clears it)"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "description": "Full TODO snapshot; [] clears the list",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "Task text (non-empty)" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["items"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let bad = |reason: String| ToolError::ExecutionFailed {
            name: "todo".into(),
            reason,
        };
        let items = input["items"]
            .as_array()
            .ok_or_else(|| bad("'items' must be a JSON array".into()))?;
        for (i, item) in items.iter().enumerate() {
            if item["content"].as_str().is_none_or(|c| c.trim().is_empty()) {
                return Err(bad(format!(
                    "items[{i}].content must be a non-empty string"
                )));
            }
            let status = item["status"].as_str().unwrap_or_default();
            if !matches!(
                status,
                "pending" | "in_progress" | "completed" | "cancelled"
            ) {
                return Err(bad(format!(
                    "items[{i}].status must be one of pending|in_progress|completed|cancelled"
                )));
            }
        }
        Ok(serde_json::json!({ "ok": true, "count": items.len() }))
    }
}

pub fn register_builtins(registry: &agentj_core::module::ToolRegistry) {
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(TodoTool));
}

/// v0.1: timeout wrapper — spawns execution, returns Timeout if it exceeds `timeout_dur`.
/// Uses recv_timeout so the caller is never blocked by a hung tool.
pub fn execute_with_timeout(
    tool: &Arc<dyn agentj_core::module::Tool>,
    input: serde_json::Value,
    timeout_dur: std::time::Duration,
) -> Result<serde_json::Value, ToolError> {
    let tool = tool.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = tool.execute(input);
        let _ = tx.send(result);
    });
    match rx.recv_timeout(timeout_dur) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(ToolError::Timeout(timeout_dur)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ToolError;
    use agentj_core::module::{Tool, ToolCallContext};
    use std::sync::Arc;

    #[test]
    fn read_tool_reads_file() -> Result<(), ToolError> {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = ReadTool;
        let result = tool.execute(serde_json::json!({"path": file_path.to_str().unwrap()}))?;
        assert_eq!(result.as_str().unwrap(), "hello world");
        Ok(())
    }

    #[test]
    fn bash_tool_timeout_detected() {
        let tool: Arc<dyn Tool> = Arc::new(BashTool);
        let started = std::time::Instant::now();
        let result = execute_with_timeout(
            &tool,
            serde_json::json!({"command": "sleep 5"}),
            std::time::Duration::from_millis(100),
        );
        assert!(matches!(result, Err(ToolError::Timeout(_))));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn todo_tool_accepts_valid_items_and_counts() {
        let out = TodoTool
            .execute(serde_json::json!({"items": [
                {"content": "a", "status": "pending"},
                {"content": "b", "status": "in_progress"},
                {"content": "c", "status": "completed"},
                {"content": "d", "status": "cancelled"}
            ]}))
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "count": 4}));
    }

    #[test]
    fn todo_tool_accepts_empty_items_as_snapshot_clear() {
        let out = TodoTool.execute(serde_json::json!({"items": []})).unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "count": 0}));
    }

    #[test]
    fn todo_tool_rejects_empty_content() {
        let err = TodoTool
            .execute(serde_json::json!({"items": [{"content": "", "status": "pending"}]}))
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("content")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn todo_tool_rejects_missing_items() {
        let err = TodoTool.execute(serde_json::json!({})).unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("items")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn todo_tool_rejects_bad_status() {
        let err = TodoTool
            .execute(serde_json::json!({"items": [{"content": "x", "status": "doing"}]}))
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("status")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn register_builtins_registers_six_tools_including_todo() {
        let registry = agentj_core::module::ToolRegistry::new();
        register_builtins(&registry);
        assert_eq!(registry.definitions().len(), 6);
        assert!(registry.get("todo").is_some());
    }

    fn ctx_for(root: &std::path::Path) -> ToolCallContext {
        ToolCallContext {
            session_id: "s1".into(),
            workspace_root: root.to_path_buf(),
        }
    }

    #[test]
    fn glob_tool_scopes_to_context_workspace_root() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(outside.path().join("outside.rs"), "").unwrap();

        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "*.rs"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        let stdout = out.as_str().unwrap();
        assert!(
            stdout.contains("./src/main.rs"),
            "missing root file: {stdout}"
        );
        assert!(
            !stdout.contains("outside.rs"),
            "leaked outside root: {stdout}"
        );
    }

    #[test]
    fn glob_tool_survives_registry_rebase_of_basename_pattern() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();

        let args = agentj_core::tool_paths::rebase_tool_args(
            "glob",
            serde_json::json!({"pattern": "*.rs"}),
            root.path(),
        );
        assert_eq!(args, serde_json::json!({"pattern": "*.rs"}));
        let out = GlobTool
            .execute_with_context(args, &ctx_for(root.path()))
            .unwrap();
        assert!(out.as_str().unwrap().contains("./src/main.rs"));
    }

    #[test]
    fn bash_tool_uses_context_workspace_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolCallContext {
            session_id: "s1".into(),
            workspace_root: dir.path().to_path_buf(),
        };
        let out = BashTool
            .execute_with_context(serde_json::json!({"command": "pwd"}), &ctx)
            .unwrap();
        let stdout = out.as_str().unwrap().trim();
        assert_eq!(
            std::fs::canonicalize(stdout).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn bash_tool_execute_without_context_keeps_process_cwd() {
        let out = BashTool
            .execute(serde_json::json!({"command": "pwd"}))
            .unwrap();
        let stdout = out.as_str().unwrap().trim();
        let expected = std::env::current_dir().unwrap();
        assert_eq!(
            std::fs::canonicalize(stdout).unwrap(),
            std::fs::canonicalize(expected).unwrap()
        );
    }
}
