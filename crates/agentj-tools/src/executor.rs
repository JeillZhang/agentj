//! Unified tool-execution lifecycle (design decision #15): process-group isolation,
//! timeout kills the WHOLE group, drop reclaims orphans.

use agentj_core::error::ToolError;
use agentj_core::process_group::{ProcessGroup, isolate};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
}

/// Why a guarded run failed at the executor level. Kept separate from `ToolError`
/// so the "program is not installed" case stays type-safe: `grep` uses it to decide
/// whether to fall back to the in-process walker (no string matching).
#[derive(Debug)]
pub enum RunFailure {
    /// `tool` is the tool the model asked for (`bash`, `grep`), `program` the binary that
    /// could not be spawned. The model sees the tool: "which binary do you use" is ours.
    ProgramMissing {
        tool: Option<String>,
        program: String,
        reason: String,
    },
    Timeout(Duration),
    Failed(ToolError),
}

impl From<RunFailure> for ToolError {
    fn from(failure: RunFailure) -> Self {
        match failure {
            RunFailure::ProgramMissing {
                tool,
                program,
                reason,
            } => ToolError::ExecutionFailed {
                name: tool.unwrap_or_else(|| program.clone()),
                reason: format!("{program}: {reason}"),
            },
            RunFailure::Timeout(timeout) => ToolError::Timeout(timeout),
            RunFailure::Failed(error) => error,
        }
    }
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
    group: Option<ProcessGroup>,
    kill_on_drop: bool,
}

impl ChildGuard {
    pub fn pid(&self) -> u32 {
        self.group.as_ref().map(ProcessGroup::pid).unwrap_or(0)
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
        // The leader exited on its own: keep whatever it left running in the group
        // (a command may start a server on purpose) and stop `Drop` from killing it.
        if let Some(group) = self.group.as_mut() {
            group.disarm();
        }
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
        let Some(pid) = self.group.as_ref().map(ProcessGroup::pid) else {
            return;
        };
        // `ProcessGroup::kill` is a no-op off unix, where the Child handle is the only
        // lever left — hence the explicit `start_kill` on that path.
        #[cfg(not(unix))]
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(group) = self.group.as_ref() {
            group.kill();
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
        let _ = pid;
        self.group
            .as_ref()
            .map(|group| group.reap(Duration::ZERO))
            .unwrap_or(true)
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
    tool: Option<String>,
}

impl ToolExecutor {
    pub fn new(timeout: Duration, kill_on_drop: bool) -> Self {
        Self {
            timeout,
            kill_on_drop,
            tool: None,
        }
    }

    /// Name the tool this executor runs for. Spawn failures are reported under this name
    /// so the model never sees "sh"/"rg" instead of the tool it actually called.
    pub fn with_tool(mut self, tool: &str) -> Self {
        self.tool = Some(tool.to_string());
        self
    }

    /// Spawn a child into its own process group, wrapped in a kill_on_drop guard.
    pub async fn spawn_guarded(&self, spec: CommandSpec) -> Result<ChildGuard, ToolError> {
        self.spawn_guarded_typed(spec)
            .await
            .map_err(ToolError::from)
    }

    /// Typed spawn: a missing program is reported as `RunFailure::ProgramMissing`
    /// (not an opaque error string) so `grep` can decide to fall back.
    async fn spawn_guarded_typed(&self, spec: CommandSpec) -> Result<ChildGuard, RunFailure> {
        use std::process::Stdio;
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &spec.cwd {
            cmd.current_dir(dir);
        }
        isolate(&mut cmd); // new process group with pgid = child pid
        let child = cmd.spawn().map_err(|e| {
            let program = spec.program.clone();
            if e.kind() == std::io::ErrorKind::NotFound {
                RunFailure::ProgramMissing {
                    tool: self.tool.clone(),
                    program,
                    reason: e.to_string(),
                }
            } else {
                // Same contract as `ProgramMissing`: the tool owns the error, the
                // program is detail — otherwise one tool reports two names.
                RunFailure::Failed(ToolError::ExecutionFailed {
                    name: self.tool.clone().unwrap_or_else(|| program.clone()),
                    reason: format!("{program}: {e}"),
                })
            }
        })?;
        let group = child.id().map(ProcessGroup::of);
        Ok(ChildGuard {
            child: Some(child),
            group,
            kill_on_drop: self.kill_on_drop,
        })
    }

    /// Run to completion with a timeout; on timeout, kill the whole group.
    pub async fn run(&self, spec: CommandSpec) -> Result<ExecOutput, ToolError> {
        let guard = self.spawn_guarded(spec).await?;
        self.run_guarded(guard).await
    }

    /// Synchronous entry point for the sync `Tool` trait: build a dedicated
    /// current-thread runtime and drive the guarded run to completion.
    ///
    /// Safe on a blocking-pool thread: `spawn_blocking` workers only call
    /// `Handle::enter` (tokio blocking/pool.rs), they never set the
    /// `EnterRuntime::Entered` flag that makes `Runtime::block_on` panic.
    pub fn run_blocking(&self, spec: CommandSpec) -> Result<ExecOutput, RunFailure> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all() // io driver is required by the process reaper (design D2)
            .build()
            .map_err(|e| {
                RunFailure::Failed(ToolError::ExecutionFailed {
                    name: "runtime".into(),
                    reason: e.to_string(),
                })
            })?;
        runtime.block_on(async {
            let guard = self.spawn_guarded_typed(spec).await?;
            self.run_guarded_typed(guard).await
        })
    }

    /// Timeout loop over an already-spawned guard (private; lets tests capture the pid
    /// before the timeout fires). On timeout the `wait_with_output` future is dropped —
    /// the tokio Child is gone and its kill_on_drop is false — so the group is reclaimed
    /// explicitly by pid.
    async fn run_guarded(&self, guard: ChildGuard) -> Result<ExecOutput, ToolError> {
        self.run_guarded_typed(guard).await.map_err(ToolError::from)
    }

    async fn run_guarded_typed(&self, mut guard: ChildGuard) -> Result<ExecOutput, RunFailure> {
        let result = tokio::time::timeout(self.timeout, guard.wait_with_output()).await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(RunFailure::Failed(e)),
            Err(_) => {
                guard.kill_group(); // kill the whole tree before returning
                Err(RunFailure::Timeout(self.timeout))
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
                cwd: None,
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
                cwd: None,
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
                cwd: None,
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
                cwd: None,
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

    #[test]
    fn run_blocking_returns_output_on_success() {
        // Sync #[test] on purpose: no ambient runtime, exactly like the
        // spawn_blocking thread that calls this in production.
        let executor = ToolExecutor::new(Duration::from_secs(5), true);
        let out = executor
            .run_blocking(CommandSpec {
                program: "echo".into(),
                args: vec!["hi".into()],
                cwd: None,
            })
            .unwrap();
        assert_eq!(out.stdout.trim(), "hi");
    }

    #[cfg(unix)]
    #[test]
    fn run_blocking_times_out_and_reclaims_the_group() {
        let executor = ToolExecutor::new(Duration::from_millis(100), true);
        let start = std::time::Instant::now();
        let err = executor
            .run_blocking(CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 30".into()],
                cwd: None,
            })
            .unwrap_err();
        assert!(matches!(err, RunFailure::Timeout(_)), "unexpected: {err:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout must fire quickly"
        );
    }

    #[test]
    fn run_blocking_reports_a_missing_program() {
        let executor = ToolExecutor::new(Duration::from_secs(5), true);
        let err = executor
            .run_blocking(CommandSpec {
                program: "agentj-test-no-such-binary".into(),
                args: vec![],
                cwd: None,
            })
            .unwrap_err();
        assert!(
            matches!(&err, RunFailure::ProgramMissing { program, reason, .. }
                if program == "agentj-test-no-such-binary" && reason.contains("No such file")),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn program_missing_names_the_tool_and_keeps_the_program() {
        let err = ToolError::from(RunFailure::ProgramMissing {
            tool: Some("bash".to_string()),
            program: "agentj-test-no-such-binary".to_string(),
            reason: "No such file or directory (os error 2)".to_string(),
        });
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, .. } if name == "bash"),
            "{err:?}"
        );
        let text = err.to_string();
        assert!(text.contains("tool 'bash' execution failed"), "{text}");
        assert!(text.contains("agentj-test-no-such-binary"), "{text}");
    }

    #[test]
    fn program_missing_without_a_tool_falls_back_to_the_program() {
        let err = ToolError::from(RunFailure::ProgramMissing {
            tool: None,
            program: "rg".to_string(),
            reason: "No such file".to_string(),
        });
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, .. } if name == "rg"),
            "{err:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generic_spawn_failure_also_names_the_tool() {
        // `ProgramMissing` is only the `NotFound` branch; every other spawn error goes
        // through the generic branch, which today reports the *program*.
        let dir = std::env::temp_dir().join(format!("aj-tool-spawn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let executor = ToolExecutor::new(Duration::from_secs(5), true).with_tool("bash");
        let err = executor
            .spawn_guarded(CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "true".into()],
                cwd: Some(dir.join("missing-cwd")), // ENOENT from chdir, not from exec
            })
            .await
            .expect_err("a missing cwd must fail the spawn");
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, .. } if name == "bash"),
            "{err:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_failure_converts_back_to_tool_error() {
        let missing: ToolError = RunFailure::ProgramMissing {
            tool: None,
            program: "rg".into(),
            reason: "No such file or directory (os error 2)".into(),
        }
        .into();
        assert!(
            matches!(missing, ToolError::ExecutionFailed { ref name, .. } if name == "rg"),
            "{missing:?}"
        );

        // Also pins the model-visible wording for the timeout path (Review Focus #3).
        let timeout: ToolError = RunFailure::Timeout(Duration::from_millis(1500)).into();
        assert!(matches!(timeout, ToolError::Timeout(_)));
        assert_eq!(timeout.to_string(), "tool timeout after 1.5s");
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        agentj_core::process_group::alive(pid)
    }

    #[cfg(unix)] // `process_alive` itself is `#[cfg(unix)]`; without the cfg this breaks on windows
    #[tokio::test]
    async fn guard_drop_kills_the_group_leader_and_is_idempotent() {
        let executor = ToolExecutor::new(Duration::from_secs(5), true);
        let spec = CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "sleep 30 & wait".into()],
            cwd: None,
        };
        let guard = executor.spawn_guarded(spec).await.expect("sh spawns");
        let pid = guard.pid();
        assert!(process_alive(pid), "the leader must start alive");
        drop(guard); // Drop → kill the group + reap
        let mut gone = false;
        for _ in 0..150 {
            if !process_alive(pid) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gone, "Drop must reclaim the group leader {pid}");
    }

    /// A successful run must not kill what its command left running: `bash` may start a
    /// dev server on purpose, so the shared process group only kills on failure,
    /// timeout and drop (review finding F1).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_successful_run_leaves_background_children_alive() {
        let executor = ToolExecutor::new(Duration::from_secs(5), true);
        let spec = CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "sleep 30 >/dev/null 2>&1 & echo $!".into()],
            cwd: None,
        };
        let out = executor.run(spec).await.expect("sh runs");
        let background: u32 = out.stdout.trim().parse().expect("pid on stdout");
        assert!(
            process_alive(background),
            "the background child survives {out:?}"
        );
        // Cleanup: the executor no longer owns it.
        agentj_core::process_group::ProcessGroup::of(background).kill();
    }
}
