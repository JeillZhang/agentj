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
