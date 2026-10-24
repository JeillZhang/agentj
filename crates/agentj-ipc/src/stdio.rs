//! Stdio transport: `StdioConnection` and the `IpcServer::serve_stdio` entry.
//!
//! Verbatim move out of `lib.rs`: the serve loop, its graceful-teardown order and
//! the sync stdout writer are unchanged; only the struct stays crate-internal.

use crate::{Connection, ConnectionWriter, IpcServer, TransportKind};
use agentj_core::error::IpcError;
use agentj_core::module::KernelMessage;
use async_trait::async_trait;
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

impl IpcServer {
    /// Run the server in stdio mode via the unified Connection serve loop (design 决策 #2).
    /// On EOF or after a shutdown request, signal the kernel and await its graceful shutdown.
    /// The read loop is interrupted when the kernel finishes (design #13: SIGINT/SIGTERM
    /// from the CLI sends Shutdown → kernel exits → serve loop returns even though stdin
    /// is still open).
    /// Register the single stdio connection and subscribe the two notifiers:
    /// `$/permissionRequest` and `$/askQuestion` route to the session owner through
    /// the registry (决策 #5).
    fn register_stdio_connection(&self) -> String {
        let conn = StdioConnection::new();
        let conn_id = conn.id();
        if let Ok(mut reg) = self.registry.write() {
            reg.register(Box::new(conn));
        }
        self.attach_permission_notifier();
        self.attach_question_notifier();
        conn_id
    }

    pub async fn serve_stdio(&mut self) -> Result<(), IpcError> {
        // v0.1: a single StdioConnection implements the Connection trait; the serve loop
        // is transport-agnostic so v0.3's TcpConnection slots in with zero rework.
        let conn_id = self.register_stdio_connection();
        // Poll the kernel handle by `&mut` (taken out so the future needs no `self` borrow
        // while the loop also reads the registry); it is restored below for await_shutdown.
        let mut kernel_handle = self.kernel_handle.take();
        let mut kernel_finished = false;
        let kernel_done = async {
            if let Some(handle) = &mut kernel_handle {
                let _ = handle.await;
            }
            true
        };
        tokio::pin!(kernel_done);
        loop {
            match self.stdio_step(&conn_id, &mut kernel_done).await? {
                StdioStep::Continue => {}
                StdioStep::KernelFinished => {
                    kernel_finished = true;
                    break;
                }
                StdioStep::Eof | StdioStep::Shutdown => break,
            }
        }
        // Graceful teardown. When the loop exited via kernel_done the kernel already ran
        // its shutdown chain — re-awaiting the completed handle would panic, so skip. On
        // the EOF/request paths, signal the kernel (idempotent — the shutdown RPC may have
        // already sent it), then wait for the modules' shutdown() chain to finish.
        if !kernel_finished {
            self.kernel_handle = kernel_handle;
            if !self.shutdown_requested() {
                let _ = self.kernel.sender().send(KernelMessage::Shutdown);
            }
            self.await_shutdown()
                .await
                .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
        }
        Ok(())
    }

    /// One read-loop iteration: detach the connection so `recv`'s await never runs under
    /// the registry lock (std guards are !Send and must not be held across await points),
    /// then re-attach it for the response write.
    async fn stdio_step<F>(
        &mut self,
        conn_id: &str,
        kernel_done: &mut F,
    ) -> Result<StdioStep, IpcError>
    where
        F: std::future::Future<Output = bool> + Unpin,
    {
        let mut conn = Self::detach_stdio_conn(&self.registry, conn_id)?;
        let line = tokio::select! {
            res = conn.recv() => {
                match res {
                    Some(Ok(line)) => line,
                    Some(Err(e)) => return Err(e),
                    None => return Ok(StdioStep::Eof), // EOF (client closed stdin)
                }
            }
            _ = kernel_done => return Ok(StdioStep::KernelFinished), // signal-triggered shutdown
        };
        if let Ok(mut reg) = self.registry.write() {
            reg.conns.insert(conn_id.to_string(), conn);
        }
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            return Ok(StdioStep::Continue);
        }
        self.respond_stdio(conn_id, &trimmed).await?;
        Ok(if self.shutdown_requested() {
            StdioStep::Shutdown
        } else {
            StdioStep::Continue
        })
    }

    /// Take the connection out of the registry so the following await holds no guard.
    fn detach_stdio_conn(
        registry: &Arc<std::sync::RwLock<crate::ConnectionRegistry>>,
        conn_id: &str,
    ) -> Result<Box<dyn Connection>, IpcError> {
        registry
            .write()
            .map_err(|_| IpcError::InvalidRequest("connection registry poisoned".into()))?
            .conns
            .remove(conn_id)
            .ok_or_else(|| IpcError::InvalidRequest("stdio connection missing".into()))
    }

    /// Dispatch one request line and write the reply (a dispatch failure becomes the
    /// -32603 frame; an empty reply writes nothing).
    async fn respond_stdio(&mut self, conn_id: &str, line: &str) -> Result<(), IpcError> {
        let response = self.handle_request(line).await.unwrap_or_else(|e| {
            format!(r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"{e}"}}}}"#)
        });
        if response.is_empty() {
            return Ok(());
        }
        let mut delivered = false;
        if let Ok(reg) = self.registry.read()
            && let Some(conn) = reg.conns.get(conn_id)
        {
            conn.send(&response)?;
            delivered = true;
        }
        if delivered {
            return Ok(());
        }
        Err(IpcError::InvalidRequest("stdio connection missing".into()))
    }
}

/// Why one stdio read-loop iteration stopped.
enum StdioStep {
    /// Handled (or blank line) — keep looping.
    Continue,
    /// Client closed stdin.
    Eof,
    /// A shutdown request arrived through the RPC surface.
    Shutdown,
    /// The kernel's shutdown chain finished first (signal-triggered).
    KernelFinished,
}

/// Stdio transport — implements `Connection` for stdin/stdout NDJSON (design 决策 #2).
pub(crate) struct StdioConnection {
    id: String,
    reader: BufReader<tokio::io::Stdin>,
}

impl StdioConnection {
    pub fn new() -> Self {
        Self {
            // Sentinel id matches handle_request_from's stdio conn_id so
            // owner-directed routing (send_to) reaches the stdout writer.
            id: "stdio".into(),
            reader: BufReader::new(tokio::io::stdin()),
        }
    }
}

impl Default for StdioConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Connection for StdioConnection {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Stdio
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        // Sync trait method — std blocking stdout; tokio stdout's block_on panics
        // when called from within the serve loop's async context.
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(msg.as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => Some(Ok(line)),
            Err(e) => Some(Err(IpcError::InvalidRequest(e.to_string()))),
        }
    }
    fn peer_desc(&self) -> String {
        "stdio".into()
    }
    fn writer(&self) -> ConnectionWriter {
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(|msg: &str| {
                // Sync trait method — std blocking stdout; tokio stdout's
                // block_on panics inside an async context.
                use std::io::Write;
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(msg.as_bytes())?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                Ok(())
            }),
        )
    }
}
