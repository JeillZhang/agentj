//! One process-group abstraction for every subprocess this kernel spawns.
//!
//! `kill_on_drop` on the direct child is not enough: several spawn sites launch
//! `sh -c <command>`, so the real work — and anything it starts — is a *grandchild*
//! that survives the shell being killed. Every spawn site therefore isolates the
//! child into its own group (`isolate`) and kills the group (`kill`), then waits for
//! the leader to disappear (`reap`) so the kernel does not keep a zombie.
//!
//! Unix implements all three. Elsewhere `isolate`/`kill`/`reap` are no-ops: Windows
//! has no `kill(-pid)` group semantics and no portable "probe without reaping" call,
//! so `alive` answers `false`, `reap` answers `false` and a timed-out child can leave
//! descendants behind. Callers that still hold the `Child` handle fall back to
//! `Child::start_kill`; once the handle is consumed the tree is simply unreachable —
//! a documented platform gap, not something a caller can satisfy. Keeping the no-op
//! explicit (rather than `cfg`-ing whole call sites) keeps one code path everywhere.

use std::time::{Duration, Instant};

/// Put `cmd` into a fresh process group whose id is the child's pid.
pub fn isolate(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

/// Is `pid` still running (and not a zombie)? Used by tests and by callers that
/// need to observe a group they do not own a `Child` handle for.
///
/// Answers `false` on non-unix: there is no portable "probe without reaping" call,
/// and no caller may depend on it for correctness.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            return false;
        }
        // A finished-but-unreaped child still answers kill(pid, 0): read the state.
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
            // No /proc (macOS): kill(0) answered, that is as good as it gets.
            Err(_) => true,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// A spawned child's process group, addressed by the leader's pid.
///
/// Dropping the value kills and reclaims the group, mirroring
/// `tokio::process::Command::kill_on_drop` but for the whole tree.
#[derive(Debug)]
pub struct ProcessGroup {
    pid: u32,
    /// `false` once the group must outlive this value — a successful run keeps the
    /// background children its command started (a dev server, a watcher).
    armed: bool,
}

impl ProcessGroup {
    pub fn of(pid: u32) -> Self {
        Self { pid, armed: true }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Stop `Drop` from killing the group: the leader already exited on its own and
    /// whatever it left running is the command's business, not ours.
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// SIGKILL every member of the group. Failures are ignored: the group may be gone.
    pub fn kill(&self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
        #[cfg(not(unix))]
        {
            let _ = self.pid;
        }
    }

    /// Alive *and* not a zombie — same judgement as [`alive`], reachable from the group.
    pub fn is_running(pid: u32) -> bool {
        alive(pid)
    }

    /// Wait up to `budget` for the group leader to be reaped. `true` once it is gone
    /// (this call consumed it, or it was already reaped — the kernel answers `ECHILD`).
    pub fn reap(&self, budget: Duration) -> bool {
        #[cfg(unix)]
        {
            let deadline = Instant::now() + budget;
            loop {
                let mut status = 0;
                let ret = unsafe { libc::waitpid(self.pid as i32, &mut status, libc::WNOHANG) };
                if ret == self.pid as i32 {
                    return true; // reaped now
                }
                if ret < 0 {
                    match std::io::Error::last_os_error().raw_os_error() {
                        Some(libc::ECHILD) => return true, // already reaped: nothing left
                        Some(libc::EINTR) => {}            // unrelated signal: retry
                        _ => return false,
                    }
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = budget;
            false
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.kill();
        if !self.reap(Duration::from_millis(250)) {
            tracing::warn!(pid = self.pid, "process group not reclaimed within 250ms");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

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

    /// `sh -c 'sleep 300 & ...'` puts the sleeper in the *group*, so killing the group
    /// must take it down even though it is a grandchild.
    async fn spawn_sleeper_group(pidfile: &Path) -> (tokio::process::Child, ProcessGroup) {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(format!(
                "sleep 300 & echo $! > '{}'; wait",
                pidfile.display()
            ))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        isolate(&mut cmd);
        let child = cmd.spawn().expect("sh spawns");
        let group = ProcessGroup::of(child.id().expect("child has a pid"));
        (child, group)
    }

    #[tokio::test]
    async fn kill_takes_down_the_grandchild_in_the_group() {
        let dir = std::env::temp_dir().join(format!("aj-core-group-kill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("kill.pid");
        let (mut child, group) = spawn_sleeper_group(&pidfile).await;
        let grandchild = pid_from(&pidfile).await;
        assert!(running(grandchild), "the grandchild must start alive");
        group.kill();
        assert_gone(grandchild).await;
        let _ = child.wait().await;
        std::fs::remove_file(&pidfile).ok();
    }

    #[tokio::test]
    async fn drop_reclaims_the_group_and_reports_done() {
        let dir = std::env::temp_dir().join(format!("aj-core-group-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("drop.pid");
        let (mut child, group) = spawn_sleeper_group(&pidfile).await;
        let grandchild = pid_from(&pidfile).await;
        drop(group);
        assert_gone(grandchild).await;
        let _ = child.wait().await;
        std::fs::remove_file(&pidfile).ok();
    }

    /// `waitpid` answers `ECHILD` for a pid that was never our child: nothing to reap,
    /// which is a success for the caller (review finding F3 — only `ECHILD` counts).
    #[test]
    fn reap_answers_true_for_a_pid_that_never_existed() {
        let absent = 0x7fff_fffe_u32;
        let started = Instant::now();
        assert!(ProcessGroup::of(absent).reap(Duration::from_millis(500)));
        assert!(started.elapsed() < Duration::from_millis(500), "no polling");
    }

    /// A disarmed group outlives its value: a successful run keeps what its command
    /// started (review finding F1 — the old `Drop` killed the tree on success).
    #[tokio::test]
    async fn a_disarmed_group_survives_its_drop() {
        let dir = std::env::temp_dir().join(format!("aj-core-group-keep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("keep.pid");
        let (mut child, mut group) = spawn_sleeper_group(&pidfile).await;
        let leader = group.pid();
        let grandchild = pid_from(&pidfile).await;
        group.disarm();
        drop(group);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(running(grandchild), "a disarmed group must keep running");
        ProcessGroup::of(leader).kill(); // cleanup: a fresh, still-armed group
        assert_gone(grandchild).await;
        let _ = child.wait().await;
        std::fs::remove_file(&pidfile).ok();
    }

    /// A live leader is never "reaped": the call gives up at its budget instead of
    /// blocking, and must not overshoot it by more than one polling step.
    #[tokio::test]
    async fn reap_gives_up_at_its_budget_while_the_leader_lives() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg("sleep 5");
        let mut child = cmd.spawn().unwrap();
        let group = ProcessGroup::of(child.id().unwrap());
        let started = Instant::now();
        assert!(!group.reap(Duration::from_millis(100)));
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(100) && elapsed < Duration::from_millis(300),
            "reap must respect its budget, took {elapsed:?}"
        );
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn reap_is_true_once_the_leader_is_gone() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg("exit 0");
        let mut child = cmd.spawn().unwrap();
        let pid = child.id().unwrap();
        child.wait().await.unwrap(); // tokio reaped it → waitpid answers ECHILD
        let started = Instant::now();
        assert!(ProcessGroup::of(pid).reap(Duration::from_millis(500)));
        assert!(started.elapsed() < Duration::from_millis(500));
    }
}
