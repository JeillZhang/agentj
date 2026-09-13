//! Git-state sampling around a `bash` tool call (design 2026-09-13, decision #7).
//!
//! The session workspace root is snapshotted before and after a bash command via
//! `git status --porcelain` and `git diff HEAD --numstat`; changed paths become
//! [`FileChangeArtifact`]s with git-authoritative counts. Non-git directories and
//! any git failure degrade to `None` / an empty artifact list (debug log), never
//! an error — bash results must not depend on sampling.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::artifacts::{DIFF_MAX_BYTES, FileChangeArtifact, FileChangeOp};

/// Maximum number of file-change artifacts emitted for one bash call.
pub const MAX_CHANGES: usize = 64;

/// Git state of the workspace root captured around a bash call.
///
/// `statuses` and `numstat` keys are relative to the sampled root; paths that
/// live outside the root are dropped (the design's out-of-root blind spot).
/// `top` is the repository top level reported by git.
#[derive(Debug, Clone, PartialEq)]
pub struct GitSnapshot {
    /// Repository top level (`rev-parse --show-toplevel`).
    pub top: PathBuf,
    /// `status --porcelain -z --no-renames`: path → (index X, worktree Y).
    pub statuses: HashMap<String, (char, char)>,
    /// `diff HEAD --numstat -z`: path → (additions, deletions).
    pub numstat: HashMap<String, (u64, u64)>,
}

/// Sample the git state under `root`.
///
/// Returns `None` for a non-git directory or any git failure (missing binary,
/// spawn error, non-zero exit); the reason is logged at debug level.
pub async fn sample(root: &Path) -> Option<GitSnapshot> {
    let top_raw = git_output(root, &["rev-parse", "--show-toplevel"]).await?;
    let top = PathBuf::from(String::from_utf8_lossy(&top_raw).trim());
    let status_raw = git_output(root, &["status", "--porcelain", "-z", "--no-renames"]).await?;
    let numstat_raw =
        git_output(root, &["diff", "HEAD", "--numstat", "-z", "--no-renames"]).await?;
    // `top` is canonical while `root` may carry symlinks; align them for mapping.
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    Some(GitSnapshot {
        statuses: parse_statuses(&status_raw, &top, &base),
        numstat: parse_numstat(&numstat_raw, &top, &base),
        top,
    })
}

/// Diff two samples into file-change artifacts, capped at [`MAX_CHANGES`].
///
/// `before == None` treats every path in `after` as changed; callers that could
/// not sample before a bash call should not call this at all.
pub async fn changes_between(
    before: Option<&GitSnapshot>,
    after: &GitSnapshot,
    root: &Path,
    call_id: &str,
) -> Vec<FileChangeArtifact> {
    let paths = changed_paths(before, after);
    if paths.len() > MAX_CHANGES {
        tracing::debug!(
            total = paths.len(),
            kept = MAX_CHANGES,
            "bash file-change artifacts truncated"
        );
    }
    let mut artifacts = Vec::new();
    for path in paths.iter().take(MAX_CHANGES) {
        if let Some(change) = change_for(path, before, after, root, call_id).await {
            artifacts.push(change);
        }
    }
    artifacts
}

/// Paths whose status or numstat differs between the two samples.
fn changed_paths(before: Option<&GitSnapshot>, after: &GitSnapshot) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for (path, state) in &after.statuses {
        if before.and_then(|snapshot| snapshot.statuses.get(path)) != Some(state) {
            paths.insert(path.clone());
        }
    }
    for (path, counts) in &after.numstat {
        if before.and_then(|snapshot| snapshot.numstat.get(path)) != Some(counts) {
            paths.insert(path.clone());
        }
    }
    if let Some(before) = before {
        let gone = before
            .statuses
            .keys()
            .chain(before.numstat.keys())
            .filter(|path| {
                !after.statuses.contains_key(*path) && !after.numstat.contains_key(*path)
            });
        paths.extend(gone.cloned());
    }
    paths
}

/// Build the artifact for one changed path; `None` when nothing can be reported.
async fn change_for(
    path: &str,
    before: Option<&GitSnapshot>,
    after: &GitSnapshot,
    root: &Path,
    call_id: &str,
) -> Option<FileChangeArtifact> {
    let status = after.statuses.get(path).copied();
    let untracked = matches!(status, Some((x, y)) if x == '?' || y == '?');
    let op = if untracked {
        FileChangeOp::Add
    } else {
        match status {
            Some(('A', _)) => FileChangeOp::Add,
            Some((x, y)) if x == 'D' || y == 'D' => FileChangeOp::Delete,
            Some(_) => FileChangeOp::Modify,
            None if is_missing(root, path).await => FileChangeOp::Delete,
            None => return None,
        }
    };
    let (additions, deletions, diff, diff_truncated) = if untracked {
        (line_count(root, path).await, 0, None, false)
    } else {
        let (additions, deletions) = counts_for(path, before, after, root, op).await;
        let (diff, truncated) = tracked_diff(root, path).await;
        (additions, deletions, diff, truncated)
    };
    Some(FileChangeArtifact {
        path: path.to_string(),
        op,
        additions,
        deletions,
        tool_call_id: Some(call_id.to_string()),
        diff,
        diff_truncated,
    })
}

/// Git-authoritative counts, falling back to file inspection when absent.
async fn counts_for(
    path: &str,
    before: Option<&GitSnapshot>,
    after: &GitSnapshot,
    root: &Path,
    op: FileChangeOp,
) -> (u64, u64) {
    if let Some(counts) = after.numstat.get(path) {
        return *counts;
    }
    match op {
        FileChangeOp::Add => (line_count(root, path).await, 0),
        FileChangeOp::Delete => (
            0,
            before
                .and_then(|snapshot| snapshot.numstat.get(path))
                .map_or(0, |(_, deletions)| *deletions),
        ),
        FileChangeOp::Modify => (0, 0),
    }
}

/// True when `root/path` does not exist (metadata failure counts as missing).
async fn is_missing(root: &Path, path: &str) -> bool {
    tokio::fs::metadata(root.join(path)).await.is_err()
}

/// Line count of a file (newly created or otherwise untracked); failures → 0.
async fn line_count(root: &Path, path: &str) -> u64 {
    match tokio::fs::read_to_string(root.join(path)).await {
        Ok(content) => content.lines().count() as u64,
        Err(err) => {
            tracing::debug!(path, error = %err, "bash file-change line count unavailable");
            0
        }
    }
}

/// `git diff HEAD -- <path>` for a tracked change, capped at [`DIFF_MAX_BYTES`].
async fn tracked_diff(root: &Path, path: &str) -> (Option<String>, bool) {
    let Some(bytes) = git_output(root, &["diff", "HEAD", "--no-renames", "--", path]).await else {
        return (None, false);
    };
    if bytes.len() > DIFF_MAX_BYTES {
        tracing::debug!(
            path,
            bytes = bytes.len(),
            "bash file-change diff over cap, dropped"
        );
        return (None, true);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.is_empty() {
        (None, false)
    } else {
        (Some(text), false)
    }
}

/// Run a git command in `root`; any failure degrades to `None` with a debug log.
async fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            tracing::debug!(?args, error = %err, "git sampling command could not run");
            return None;
        }
    };
    if !output.status.success() {
        tracing::debug!(
            ?args,
            stderr = %String::from_utf8_lossy(&output.stderr),
            "git sampling command failed"
        );
        return None;
    }
    Some(output.stdout)
}

/// Parse `status --porcelain -z --no-renames` into root-relative states.
fn parse_statuses(raw: &[u8], top: &Path, root: &Path) -> HashMap<String, (char, char)> {
    let mut statuses = HashMap::new();
    for entry in raw.split(|byte| *byte == 0) {
        // Entry: `XY SP PATH` (rename records are disabled by --no-renames).
        if entry.len() < 4 || entry[2] != b' ' {
            continue;
        }
        let state = (entry[0] as char, entry[1] as char);
        let path = String::from_utf8_lossy(&entry[3..]);
        if let Some(rel) = root_relative(&path, top, root) {
            statuses.insert(rel, state);
        }
    }
    statuses
}

/// Parse `diff HEAD --numstat -z --no-renames` into root-relative counts.
fn parse_numstat(raw: &[u8], top: &Path, root: &Path) -> HashMap<String, (u64, u64)> {
    let mut numstat = HashMap::new();
    for record in raw.split(|byte| *byte == 0) {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let (Some(additions), Some(deletions), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = String::from_utf8_lossy(path);
        if let Some(rel) = root_relative(&path, top, root) {
            numstat.insert(rel, (parse_count(additions), parse_count(deletions)));
        }
    }
    numstat
}

/// Numstat line count; `-` (binary) and malformed values degrade to 0.
fn parse_count(bytes: &[u8]) -> u64 {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Map a git-top-relative path to a root-relative path; `None` when outside.
fn root_relative(path: &str, top: &Path, root: &Path) -> Option<String> {
    top.join(path)
        .strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FileChangeOp;
    use std::process::Command as StdCommand;

    /// Unique temp directory for one test (best-effort cleanup by the caller).
    fn temp_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("agentj-t14-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp dir is creatable");
        root
    }

    /// Run a git command in `root`, panicking when git fails.
    fn git(root: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git binary is available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Initialize a git repo with a committed `A.txt` fixture.
    fn init_repo(root: &Path) {
        git(root, &["init"]);
        std::fs::write(root.join("A.txt"), "1\n2\n3\n").expect("fixture file");
        git(root, &["add", "A.txt"]);
        git(
            root,
            &[
                "-c",
                "user.name=AgentJ",
                "-c",
                "user.email=agentj@example.com",
                "commit",
                "-m",
                "init",
            ],
        );
    }

    /// Run a shell command inside `root`, panicking when it fails.
    async fn sh(root: &Path, script: &str) {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(root)
            .output()
            .await
            .expect("sh binary is available");
        assert!(
            output.status.success(),
            "sh {script} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn bash_git_sampling_detects_modify_add_delete() {
        let root = temp_dir("modify-add-delete");
        init_repo(&root);

        let before = sample(&root).await.expect("clean repo samples");
        sh(&root, "echo x >> A.txt").await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-1").await;
        assert_eq!(changes.len(), 1, "one modify: {changes:?}");
        assert_eq!(changes[0].path, "A.txt");
        assert_eq!(changes[0].op, FileChangeOp::Modify);
        assert_eq!(changes[0].additions, 1);
        assert_eq!(changes[0].deletions, 0);
        assert_eq!(changes[0].tool_call_id.as_deref(), Some("call-1"));
        let diff = changes[0].diff.as_deref().expect("small diff is kept");
        assert!(diff.contains("+x"), "diff: {diff}");

        let before = after;
        sh(&root, "printf 'n1\\nn2\\n' > B.txt").await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-2").await;
        assert_eq!(changes.len(), 1, "one add: {changes:?}");
        assert_eq!(changes[0].path, "B.txt");
        assert_eq!(changes[0].op, FileChangeOp::Add);
        assert_eq!(changes[0].additions, 2, "line count of the new file");
        assert_eq!(changes[0].deletions, 0);
        assert!(changes[0].diff.is_none(), "untracked files keep diff=None");

        let before = after;
        sh(&root, "rm -f A.txt").await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-3").await;
        assert_eq!(changes.len(), 1, "one delete: {changes:?}");
        assert_eq!(changes[0].path, "A.txt");
        assert_eq!(changes[0].op, FileChangeOp::Delete);
        assert_eq!(changes[0].additions, 0);
        assert_eq!(changes[0].deletions, 3);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_outside_git_emits_nothing() {
        let root = temp_dir("non-git");
        assert!(sample(&root).await.is_none(), "non-repo must not sample");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_failure_and_rename() {
        // Sampling failure (missing root) degrades to `None`, never an error.
        assert!(
            sample(Path::new("/nonexistent-agentj-t14-root"))
                .await
                .is_none(),
            "failed sampling must yield None"
        );

        // Rename with `--no-renames` reports Delete(A) + Add(B).
        let root = temp_dir("rename");
        init_repo(&root);
        let before = sample(&root).await.expect("repo samples");
        sh(&root, "git mv A.txt B.txt").await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-r").await;
        assert_eq!(changes.len(), 2, "rename is delete+add: {changes:?}");
        assert_eq!(changes[0].path, "A.txt");
        assert_eq!(changes[0].op, FileChangeOp::Delete);
        assert_eq!(changes[1].path, "B.txt");
        assert_eq!(changes[1].op, FileChangeOp::Add);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_truncates_at_max_changes() {
        let root = temp_dir("cap");
        init_repo(&root);
        let before = sample(&root).await.expect("repo samples");
        sh(
            &root,
            "for i in $(seq 1 70); do printf 'l\\n' > \"f$i.txt\"; done",
        )
        .await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-cap").await;
        assert_eq!(changes.len(), MAX_CHANGES, "truncated to the cap");
        assert!(
            changes.iter().all(|change| change.op == FileChangeOp::Add),
            "all 70 files are untracked adds"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_tracked_diff_over_cap_is_dropped() {
        let root = temp_dir("diff-cap");
        init_repo(&root);
        std::fs::write(root.join("big.txt"), "seed\n").expect("fixture file");
        git(&root, &["add", "big.txt"]);
        git(
            &root,
            &[
                "-c",
                "user.name=AgentJ",
                "-c",
                "user.email=agentj@example.com",
                "commit",
                "-m",
                "big",
            ],
        );
        let before = sample(&root).await.expect("repo samples");
        let line = "x".repeat(100);
        let script = format!("for i in $(seq 1 800); do echo {line} >> big.txt; done");
        sh(&root, &script).await;
        let after = sample(&root).await.expect("repo samples");
        let changes = changes_between(Some(&before), &after, &root, "call-big").await;
        assert_eq!(changes.len(), 1, "one modify: {changes:?}");
        assert_eq!(changes[0].op, FileChangeOp::Modify);
        assert_eq!(changes[0].additions, 800);
        assert!(changes[0].diff.is_none(), "over-cap diff is dropped");
        assert!(changes[0].diff_truncated);
        let _ = std::fs::remove_dir_all(&root);
    }
}
