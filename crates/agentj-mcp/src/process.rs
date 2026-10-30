//! Process-group helpers for MCP server subprocesses (design item ㊷).
//!
//! Every spawn site puts its server into a fresh process group. `kill_on_drop` on the
//! direct child is not enough: two of the three sites launch `sh -c <command>`, so the
//! actual server — and anything it starts — is a *grandchild* that would survive the
//! shell being killed. Killing the group reaps the whole tree.

use tokio::process::Command;

/// Put the command into a new process group whose id is the child's pid.
pub fn isolate_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        // Windows has no process groups here; `kill_on_drop` remains the only lever.
        let _ = cmd;
    }
}

/// SIGKILL the process group led by `pid` (a negative pid addresses the group).
/// Failures are ignored: the process (or group) may already be gone.
pub fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tokio::process::Command;

    /// Alive *and* not a zombie: a finished-but-unreaped child still answers `kill(pid, 0)`.
    fn running(pid: u32) -> bool {
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            return false;
        }
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                let state = stat
                    .rsplit(')')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next();
                state != Some("Z")
            }
            Err(_) => false,
        }
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
