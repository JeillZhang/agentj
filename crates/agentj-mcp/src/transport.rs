//! JSON-RPC 2.0 stdio transport for MCP server subprocesses (extracted from v0.1 lib.rs).
//! Shared by the v0.1 `McpClient` (per-call) and the v0.2 `McpConnectionManager`
//! (long-lived, wrapped in `Arc<Mutex<McpTransport>>` for concurrent use).

use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

pub struct McpTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child,
    next_id: u64,
}

impl McpTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, child: Child) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
            _child: child,
            next_id: 0,
        }
    }

    pub async fn request(
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

    /// Wait for the server subprocess to exit; graceful-close confirmation
    /// (design §MCP 连接生命周期「优雅关闭」). Returns false on timeout — the
    /// child's kill_on_drop is the force-kill fallback.
    pub async fn wait_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self._child.try_wait() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }
}
