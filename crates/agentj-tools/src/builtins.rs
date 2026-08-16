use agentj_core::error::ToolError;
use agentj_core::module::Tool;
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
        let cmd = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "bash".into(),
                reason: "missing 'command'".into(),
            })?;
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| ToolError::ExecutionFailed {
                name: "bash".into(),
                reason: e.to_string(),
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(serde_json::Value::String(stdout))
    }
}

pub struct GrepTool;
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
        let pattern = input["pattern"].as_str().unwrap_or(".*");
        let include = input["include"].as_str().unwrap_or("*");
        let output = std::process::Command::new("rg")
            .arg("--no-heading")
            .arg("--with-filename")
            .arg(pattern)
            .arg("--glob")
            .arg(include)
            .output()
            .map_err(|e| ToolError::ExecutionFailed {
                name: "grep".into(),
                reason: e.to_string(),
            })?;
        Ok(serde_json::Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

pub struct GlobTool;
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
        let pat = input["pattern"].as_str().unwrap_or("*");
        let output = std::process::Command::new("find")
            .arg(".")
            .arg("-name")
            .arg(pat)
            .arg("-type")
            .arg("f")
            .output()
            .map_err(|e| ToolError::ExecutionFailed {
                name: "glob".into(),
                reason: e.to_string(),
            })?;
        Ok(serde_json::Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

pub fn register_builtins(registry: &agentj_core::module::ToolRegistry) {
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(GlobTool));
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
    use agentj_core::module::Tool;
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
}
