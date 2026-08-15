//! Unified tool-execution lifecycle (design decision #15): process-group isolation,
//! timeout kills the WHOLE group, drop reclaims orphans.

use agentj_core::error::ToolError;
use std::time::Duration;

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
///
/// The pid is cached separately from the tokio `Child` because `wait_with_output`
/// takes the Child out of the guard; tokio's default `kill_on_drop` is false, so a
/// bare dropped Child leaves the process running. The cached pid lets the guard kill
/// the group even after the Child handle is gone (timeout path).
#[derive(Debug)]
pub struct ChildGuard {
    child: Option<tokio::process::Child>,
    pid: Option<u32>,
    kill_on_drop: bool,
}

impl ChildGuard {
    pub fn pid(&self) -> u32 {
        self.pid.unwrap_or(0)
    }

    /// Wait for completion (normal path). The Child is taken out; the guard keeps its
    /// cached pid so a timeout path can still `kill_group()` the whole tree.
    pub async fn wait_with_output(&mut self) -> Result<ExecOutput, ToolError> {
        let child = self
            .child
            .take()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "child".into(),
                reason: "child already consumed".into(),
            })?;
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                name: "child".into(),
                reason: e.to_string(),
            })?;
        self.kill_on_drop = false; // already exited, nothing to kill
        Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }

    /// Kill the whole process group (children included) and reap the direct child.
    /// Safe to call after `wait_with_output` consumed the Child: the pid is cached and
    /// reaping falls back to a raw `waitpid` when no Child handle remains.
    pub fn kill_group(&mut self) {
        let Some(pid) = self.pid else {
            return;
        };
        #[cfg(unix)]
        unsafe {
            // negative pid = process group kill (child spawned with process_group(0))
            let _ = libc::kill(-(pid as i32), libc::SIGKILL);
        }
        #[cfg(not(unix))]
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        // SIGKILL delivery is async; retry until the child is reaped (waitpid) or the
        // budget expires. Without this, an unreaped zombie would still "exist" to
        // kill(pid, 0) — breaking the no-orphan guarantee.
        for _ in 0..50 {
            if self.reaped(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.kill_on_drop = false; // group already terminated; nothing left to reclaim
    }

    /// Is the direct child reaped? Uses the Child handle's waitpid when still present;
    /// otherwise falls back to a raw `waitpid(pid, WNOHANG)`.
    fn reaped(&mut self, pid: u32) -> bool {
        if let Some(child) = self.child.as_mut() {
            return matches!(child.try_wait(), Ok(Some(_)));
        }
        #[cfg(unix)]
        unsafe {
            let mut status: libc::c_int = 0;
            let ret = libc::waitpid(pid as i32, &mut status, libc::WNOHANG);
            // ECHILD = already reaped elsewhere (tokio reaper); treat as done.
            if ret == pid as i32 {
                return true;
            }
            ret == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.kill_on_drop {
            return;
        }
        self.kill_group();
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
        Self {
            timeout,
            kill_on_drop,
        }
    }

    /// Spawn a child into its own process group, wrapped in a kill_on_drop guard.
    pub async fn spawn_guarded(&self, spec: CommandSpec) -> Result<ChildGuard, ToolError> {
        use std::process::Stdio;
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            cmd.process_group(0); // new process group with pgid = child pid
        }
        let child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            name: spec.program.clone(),
            reason: e.to_string(),
        })?;
        let pid = child.id();
        Ok(ChildGuard {
            child: Some(child),
            pid,
            kill_on_drop: self.kill_on_drop,
        })
    }

    /// Run to completion with a timeout; on timeout, kill the whole group.
    pub async fn run(&self, spec: CommandSpec) -> Result<ExecOutput, ToolError> {
        let guard = self.spawn_guarded(spec).await?;
        self.run_guarded(guard).await
    }

    /// Timeout loop over an already-spawned guard (private; lets tests capture the pid
    /// before the timeout fires). On timeout the `wait_with_output` future is dropped —
    /// the tokio Child is gone and its kill_on_drop is false — so the group is reclaimed
    /// explicitly by pid.
    async fn run_guarded(&self, mut guard: ChildGuard) -> Result<ExecOutput, ToolError> {
        let result = tokio::time::timeout(self.timeout, guard.wait_with_output()).await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                guard.kill_group(); // kill the whole tree before returning
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
        let out = executor
            .run(CommandSpec {
                program: "echo".into(),
                args: vec!["hi".into()],
            })
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "hi");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_times_out_and_kills_process_group() {
        // Spawn through the guard so the pid is observable, then drive the real
        // run()-style timeout path via run_guarded; the whole process group must be
        // killed and reaped — no orphan may survive the timeout.
        let executor = ToolExecutor::new(Duration::from_millis(100), true);
        let guard = executor
            .spawn_guarded(CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 30".into()],
            })
            .await
            .unwrap();
        let pid = guard.pid();
        let start = std::time::Instant::now();
        let result = executor.run_guarded(guard).await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout must fire quickly"
        );
        for _ in 0..50 {
            if !process_alive(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !process_alive(pid),
            "child pid {pid} must be reclaimed on timeout"
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn run_times_out_and_kills_process_group() {
        let executor = ToolExecutor::new(Duration::from_millis(100), true);
        let start = std::time::Instant::now();
        let result = executor
            .run(CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 30".into()],
            })
            .await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout must fire quickly"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn drop_reclaims_process_group_no_orphan() {
        // Spawn a long-running child via the executor guard, drop it WITHOUT awaiting,
        // then verify the child pid is gone (kill(-pgid) reclaims the whole group).
        let executor = ToolExecutor::new(Duration::from_secs(30), true);
        let child = executor
            .spawn_guarded(CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 30".into()],
            })
            .await
            .unwrap();
        let pid = child.pid();
        drop(child); // kill_on_drop guard: kill(-pgid) + reap
        // give the OS a moment to reap, then confirm the process is gone
        for _ in 0..50 {
            if !process_alive(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !process_alive(pid),
            "child pid {pid} must be reclaimed after drop"
        );
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        // kill(pid, 0) returns Ok if the process exists (no signal sent).
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}
