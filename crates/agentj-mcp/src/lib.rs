pub mod connection_manager;
pub mod transport;

pub use connection_manager::{ConnectionState, McpConnectionManager, McpServerSpec};

use crate::transport::McpTransport;
use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolRegistry};
use serde_json::Value;
use std::sync::Arc;
pub mod process;

use tokio::process::Command;

pub struct McpClient;
impl McpClient {
    /// Connect to an MCP server subprocess, discover its tools, and register them.
    pub async fn connect_and_register(
        srv_cmd: &str,
        registry: &ToolRegistry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(srv_cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true); // lifecycle: kill the server subprocess when transport drops
        process::isolate_group(&mut cmd);
        let mut child = cmd.spawn()?;
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
    let mut transport = prepare_call(command, name).await?;
    let resp = transport
        .request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": input }),
        )
        .await
        .map_err(|e| call_failed(name, e.to_string()))?;
    Ok(extract_result(&resp))
}

/// Spawn one MCP server over `sh -c <command>` and finish the discovery handshake.
async fn prepare_call(command: &str, name: &str) -> Result<McpTransport, ToolError> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true); // lifecycle: no orphan MCP server processes on drop
    process::isolate_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| call_failed(name, e.to_string()))?;
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
        .await
        .map_err(|e| call_failed(name, e.to_string()))?;
    // The fixture server emits the tools/call result only on its third round,
    // so mirror the discovery handshake before invoking the tool.
    transport
        .request("tools/list", serde_json::json!({}))
        .await
        .map_err(|e| call_failed(name, e.to_string()))?;
    Ok(transport)
}

/// `content[0].text` of a `tools/call` response, as a JSON string.
fn extract_result(resp: &Value) -> Value {
    let text = resp["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    Value::String(text)
}

/// The single error shape every `call_server` step reports.
fn call_failed(name: &str, reason: String) -> ToolError {
    ToolError::ExecutionFailed {
        name: name.to_string(),
        reason,
    }
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
