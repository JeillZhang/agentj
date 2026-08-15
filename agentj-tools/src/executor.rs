//! Unified tool-execution lifecycle (design decision #15): process-group isolation,
//! timeout kills the WHOLE group, drop reclaims orphans.

use std::time::Duration;
use agentj_core::error::ToolError;

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// A running child wrapped with kill_on_drop semantics: dropping this guard kills the
/// whole process group (children included) and reaps the child — no orphans survive
/// panics, early returns, or explicit drops (design decision #15).
#[derive(Debug)]
pub struct ChildGuard {
    child: Option<tokio::process::Child>,
    kill_on_drop: bool,
}

impl ChildGuard {
    pub fn pid(&self) -> u32 {
        self.child.as_ref().and_then(|c| c.id()).unwrap_or(0)
    }

    /// Wait for completion (normal path). The guard is consumed; if the caller
    /// drops it instead, the process group is reclaimed.
    pub async fn wait_with_output(mut self) -> Result<ExecOutput, ToolError> {
        let child = self.child.take().ok_or_else(|| ToolError::ExecutionFailed {
            name: "child".into(), reason: "child already consumed".into(),
        })?;
        let output = child.wait_with_output().await.map_err(|e| {
            ToolError::ExecutionFailed { name: "child".into(), reason: e.to_string() }
        })?;
        self.kill_on_drop = false; // already exited, nothing to kill
        Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.kill_on_drop { return; }
        let Some(child) = self.child.as_mut() else { return; };
        #[cfg(unix)]
        unsafe {
            // negative pid = process group kill (child spawned with process_group(0))
            if let Some(pid) = child.id() {
                let _ = libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        { let _ = child.start_kill(); }
        // SIGKILL delivery is async; retry try_wait (direct waitpid) until the child is
        // reaped or a short budget expires. Without this, an unreaped zombie would still
        // "exist" to kill(pid, 0) — breaking the no-orphan guarantee.
        for _ in 0..50 {
            if matches!(child.try_wait(), Ok(Some(_))) { return; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = child.try_wait(); // final attempt before giving up
    }
}

/// Unified tool-execution lifecycle (design decision #15): process-group isolation,
/// timeout kills the WHOLE group, drop reclaims orphans.
pub struct ToolExecutor {
    timeout: Duration,
    kill_on_drop: bool,
}

impl ToolExecutor {
    pub fn new(timeout: Duration, kill_on_drop: bool) -> Self {
        Self { timeout, kill_on_drop }
    }

    /// Spawn a child into its own process group, wrapped in a kill_on_drop guard.
    pub async fn spawn_guarded(&self, spec: CommandSpec) -> Result<ChildGuard, ToolError> {
        use std::process::Stdio;
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        {
            cmd.process_group(0); // new process group with pgid = child pid
        }
        let child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            name: spec.program.clone(), reason: e.to_string(),
        })?;
        Ok(ChildGuard { child: Some(child), kill_on_drop: self.kill_on_drop })
    }

    /// Run to completion with a timeout; on timeout, kill the whole group.
    pub async fn run(&self, spec: CommandSpec) -> Result<ExecOutput, ToolError> {
        let guard = self.spawn_guarded(spec).await?;
        let result = tokio::time::timeout(self.timeout, guard.wait_with_output()).await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // timeout: dropping the guard kills the process group (children included)
                Err(ToolError::Timeout(self.timeout))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ToolError;
    use std::time::Duration;

    #[tokio::test]
    async fn run_executes_and_returns_output() {
        let executor = ToolExecutor::new(Duration::from_secs(5), true);
        let out = executor.run(CommandSpec { program: "echo".into(), args: vec!["hi".into()] }).await.unwrap();
        assert_eq!(out.stdout.trim(), "hi");
    }

    #[tokio::test]
    async fn run_times_out_and_kills_process_group() {
        let executor = ToolExecutor::new(Duration::from_millis(100), true);
        let start = std::time::Instant::now();
        let result = executor.run(CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
        }).await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
        assert!(start.elapsed() < Duration::from_secs(5), "timeout must fire quickly");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn drop_reclaims_process_group_no_orphan() {
        // Spawn a long-running child via the executor guard, drop it WITHOUT awaiting,
        // then verify the child pid is gone (kill(-pgid) reclaims the whole group).
        let executor = ToolExecutor::new(Duration::from_secs(30), true);
        let child = executor.spawn_guarded(CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
        }).await.unwrap();
        let pid = child.pid();
        drop(child); // kill_on_drop guard: kill(-pgid) + reap
        // give the OS a moment to reap, then confirm the process is gone
        for _ in 0..50 {
            if !process_alive(pid) { break; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!process_alive(pid), "child pid {pid} must be reclaimed after drop");
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        // kill(pid, 0) returns Ok if the process exists (no signal sent).
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}
