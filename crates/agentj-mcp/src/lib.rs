use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolRegistry};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct McpClient;

impl McpClient {
    /// Connect to an MCP server subprocess, discover its tools, and register them.
    pub async fn connect_and_register(
        srv_cmd: &str,
        registry: &ToolRegistry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(srv_cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true) // lifecycle: kill the server subprocess when transport drops
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut transport = McpTransport::new(stdin, stdout, child);

        transport
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "agentj", "version": "0.1" }
                }),
            )
            .await?;

        let tools_resp = transport
            .request("tools/list", serde_json::json!({}))
            .await?;
        let tools = tools_resp["tools"]
            .as_array()
            .ok_or("no tools array in response")?;
        for tool_def in tools {
            let tool = McpTool {
                name: tool_def["name"].as_str().unwrap_or("unknown").to_string(),
                description: tool_def["description"].as_str().unwrap_or("").to_string(),
                input_schema: tool_def["inputSchema"].clone(),
                command: srv_cmd.to_string(),
            };
            registry.register(Arc::new(tool));
        }
        Ok(())
    }
}

struct McpTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child,
    next_id: u64,
}

impl McpTransport {
    fn new(stdin: ChildStdin, stdout: ChildStdout, child: Child) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
            _child: child,
            next_id: 0,
        }
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let req =
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let line = serde_json::to_string(&req)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;

        let mut response_line = String::new();
        tokio::time::timeout(
            Duration::from_secs(30),
            self.stdout.read_line(&mut response_line),
        )
        .await??;
        let resp: Value = serde_json::from_str(&response_line)?;
        if let Some(err) = resp.get("error") {
            return Err(format!("MCP error: {err}").into());
        }
        Ok(resp["result"].clone())
    }
}

struct McpTool {
    name: String,
    description: String,
    input_schema: Value,
    command: String,
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }
    fn execute(&self, input: Value) -> Result<Value, ToolError> {
        let name = self.name.clone();
        let command = self.command.clone();
        // `Handle::block_on` panics when already inside a runtime (e.g. an
        // async test calling execute from the test runtime), so drive the
        // fresh per-call server subprocess on a thread with its own runtime.
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| ToolError::ExecutionFailed {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
            rt.block_on(call_server(&command, &name, input))
        });
        handle.join().map_err(|_| ToolError::ExecutionFailed {
            name: self.name.clone(),
            reason: "MCP call thread panicked".to_string(),
        })?
    }
}

async fn call_server(command: &str, name: &str, input: Value) -> Result<Value, ToolError> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true) // lifecycle: no orphan MCP server processes on drop
        .spawn()
        .map_err(|e| ToolError::ExecutionFailed {
            name: name.to_string(),
            reason: e.to_string(),
        })?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut transport = McpTransport::new(stdin, stdout, child);
    transport.request("initialize", serde_json::json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "agentj", "version": "0.1" } })).await
        .map_err(|e| ToolError::ExecutionFailed { name: name.to_string(), reason: e.to_string() })?;
    // The fixture server emits the tools/call result only on its third round,
    // so mirror the discovery handshake before invoking the tool.
    transport
        .request("tools/list", serde_json::json!({}))
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            name: name.to_string(),
            reason: e.to_string(),
        })?;
    let resp = transport
        .request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": input }),
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            name: name.to_string(),
            reason: e.to_string(),
        })?;
    let text = resp["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    Ok(Value::String(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::module::ToolRegistry;
    use std::sync::Arc;

    #[tokio::test]
    async fn mcp_client_discovers_and_executes_tool() {
        let tools = Arc::new(ToolRegistry::new());
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mcp_echo.sh");
        McpClient::connect_and_register(script, &tools)
            .await
            .unwrap();

        let echo_tool = tools.get("echo").expect("echo tool should be registered");
        let result = echo_tool
            .execute(serde_json::json!({"text": "hello"}))
            .unwrap();
        assert_eq!(result.as_str().unwrap(), "echo: hello");
    }
}
