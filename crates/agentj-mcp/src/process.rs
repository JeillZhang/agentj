//! Process-group helpers for MCP server subprocesses (design item ㊷).
//!
//! Thin re-export of `agentj_core::process_group`: the single implementation lives in
//! the kernel so `agentj-tools` and `agentj-mcp` cannot drift. Do not add logic here —
//! it exists only to keep `agentj_mcp::process::{isolate_group, kill_group}` and its
//! regression test in place.

pub use agentj_core::process_group::{isolate as isolate_group, ProcessGroup};

/// SIGKILL the process group led by `pid` (a negative pid addresses the group).
pub fn kill_group(pid: u32) {
    ProcessGroup::of(pid).kill();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tokio::process::Command;

    /// Alive *and* not a zombie: a finished-but-unreaped child still answers `kill(pid, 0)`,
    /// which is why the check reads the state character instead of trusting the signal probe.
    fn running(pid: u32) -> bool {
        ProcessGroup::is_running(pid)
    }

    async fn pid_from(path: &Path) -> u32 {
        for _ in 0..100 {
            if let Ok(text) = std::fs::read_to_string(path)
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the grandchild never wrote {}", path.display());
    }

    async fn assert_gone(pid: u32) {
        for _ in 0..150 {
            if !running(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("grandchild {pid} survived its process group being killed");
    }

    #[tokio::test]
    async fn transport_drop_leaves_no_zombie_behind() {
        let dir = std::env::temp_dir().join(format!("agentj-mcp-zombie-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("leader.pid");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!("echo $$ > '{}'; sleep 300", pidfile.display()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        isolate_group(&mut cmd);
        let mut child = cmd.spawn().expect("sh spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let transport = crate::transport::McpTransport::new(stdin, stdout, child);
        let leader = pid_from(&pidfile).await;
        drop(transport);
        // `running` already treats `Z` as dead: this is the zombie assertion.
        assert_gone(leader).await;
        std::fs::remove_file(&pidfile).ok();
    }

    #[tokio::test]
    async fn dropping_the_transport_reaps_the_sh_grandchild() {
        let dir = std::env::temp_dir().join(format!("agentj-mcp-group-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join(format!("{:?}.pid", std::thread::current().id()));
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!(
                "sleep 300 & echo $! > '{}'; wait",
                pidfile.display()
            ))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        isolate_group(&mut cmd);
        let mut child = cmd.spawn().expect("sh spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let transport = crate::transport::McpTransport::new(stdin, stdout, child);
        let grandchild = pid_from(&pidfile).await;
        assert!(running(grandchild), "the grandchild must start alive");
        drop(transport);
        assert_gone(grandchild).await;
        std::fs::remove_file(&pidfile).ok();
    }
}
