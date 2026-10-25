//! `file/list` — sandboxed directory enumeration (design 2026-09-13, contract item 1).
//!
//! Requests resolve against the session `project_root` through
//! `agentj_core::paths::resolve_within_root` — the same primitive the permission
//! engine uses, so `file/list` cannot disagree with it about symlinks or about
//! paths whose parents do not exist yet (`-32807` when it reports an escape).
//! Enumeration is a BFS capped at [`MAX_DEPTH`]
//! levels and [`ENTRY_LIMIT`] entries; `.git` is never returned or descended,
//! escaping symlink entries are silently skipped, and per-level ordering is
//! directories-first then name-ascending.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use agentj_core::git_sample::GitSnapshot;
use agentj_core::paths::{Resolved, resolve_within_root};
use agentj_protocol::file::{FileEntry, FileKind, FileListResult, GitFileStatus};

/// Enumeration depth cap (design contract item 1: `depth` 1..=8, default 1).
pub const MAX_DEPTH: u32 = 8;
/// Entry cap; exceeding it truncates the result and sets `truncated`.
pub const ENTRY_LIMIT: usize = 2000;
/// Query-mode visit budget; exhausting it sets `truncated` (`total` is a lower bound).
pub const SEARCH_VISIT_LIMIT: usize = 20_000;
/// Maximum ranked matches returned in query mode (Top-100).
pub const SEARCH_RESULT_LIMIT: usize = 100;

const GIT_DIR: &str = ".git";

/// `file/list` failure kinds (mapped onto JSON-RPC codes + `error.data` by the
/// dispatch layer).
#[derive(Debug, PartialEq, Eq)]
pub enum FileListError {
    /// Requested path does not exist (→ `-32602`, reason `not_found`).
    NotFound,
    /// Requested path exists but is not a directory (→ `-32602`, `not_a_directory`).
    NotADirectory,
    /// Requested path is unresolvable inside the session project root — either
    /// it escapes the root (lexically or via a symlink) or an ancestor cannot be
    /// stat'ed/canonicalized (e.g. EACCES), so the sandbox cannot prove it is
    /// inside (→ `-32807`).
    OutsideRoot { path: String },
    /// `depth` outside 1..=8 (→ `-32602`, `invalid_depth`).
    InvalidDepth,
    /// Filesystem failure while listing (→ `-32603`).
    Io { path: String, reason: String },
}

/// Enumerate `path` (relative to `root`, empty/None = root) up to `depth` levels.
pub fn list(
    root: &Path,
    path: Option<&str>,
    depth: Option<u32>,
) -> Result<FileListResult, FileListError> {
    let depth = depth.unwrap_or(1);
    if !(1..=MAX_DEPTH).contains(&depth) {
        return Err(FileListError::InvalidDepth);
    }
    let scope = resolve_scope(root, path)?;
    let mut walk = Walk::new(scope.root, depth);
    walk.run(&scope.target, &scope.label, &scope.prefix)?;
    Ok(walk.finish())
}

/// Canonicalized sandbox root plus the resolved target directory, its error
/// label and its root-relative prefix (shared by listing and search).
struct Scope {
    root: PathBuf,
    target: PathBuf,
    label: String,
    prefix: String,
}

/// Resolve `path` inside `root` with the sandbox rules of the listing walk.
fn resolve_scope(root: &Path, path: Option<&str>) -> Result<Scope, FileListError> {
    let root = root.canonicalize().map_err(|e| FileListError::Io {
        path: root.display().to_string(),
        reason: e.to_string(),
    })?;
    let requested = path.unwrap_or("");
    let label = if requested.is_empty() {
        root.display().to_string()
    } else {
        requested.to_string()
    };
    let target = resolve_target(&root, requested)?;
    let metadata = std::fs::metadata(&target).map_err(|e| io_or_not_found(&label, &e))?;
    if !metadata.is_dir() {
        return Err(FileListError::NotADirectory);
    }
    let prefix = requested_prefix(&root, requested, &target);
    Ok(Scope {
        root,
        target,
        label,
        prefix,
    })
}

/// Prefix used to spell every listed row, relative to the project root.
///
/// Rows are advertised under the path the client asked for (BUG-002): clients
/// derive a row's parent from `entry.path` (`parentPath()` in the SDK, used for
/// "jump to parent" and for matching rows to the expanded node), so a prefix
/// that switched to the canonical target whenever the requested directory was a
/// symlink inside the root stopped matching the node it was rendered under.
///
/// Two request shapes deliberately keep the canonical spelling:
///
/// - an **absolute** `path` inside the root keeps reporting rows relative to
///   the root — that is the existing contract, and inventing a spelling for an
///   absolute request would only trade one confusion for another;
/// - a request containing `..` also falls back to the canonical prefix, because
///   a `..` after a symlink pops the *target*'s parent (BUG-001), so the
///   requested spelling can name a directory that was never read.
///
/// Everything else — `src`, `./src`, `src/`, `src_alias` — keeps the requested
/// spelling with `.`/empty segments normalized away. Containment is unaffected:
/// `Scope.target` stays the canonical, sandbox-checked directory that is
/// actually read, and only this display prefix is spelled.
fn requested_prefix(root: &Path, requested: &str, target: &Path) -> String {
    if requested.is_empty() {
        return String::new();
    }
    let path = Path::new(requested);
    let spelled_by_client = !path.is_absolute()
        && !path
            .components()
            .any(|component| component == Component::ParentDir);
    if !spelled_by_client {
        return canonical_prefix(root, target);
    }
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Canonical root-relative prefix of an already-resolved target (`""` at the root).
fn canonical_prefix(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolve the requested path against the canonical `root`. The verdict comes
/// from the shared fail-closed primitive: a path that is *unresolvable* inside
/// the root — an escape (lexical or via symlink), or a trail under an ancestor
/// that cannot be stat'ed (EACCES) — is `OutsideRoot`, and a nonexistent path is
/// still returned so the caller maps it to `NotFound`.
fn resolve_target(root: &Path, requested: &str) -> Result<PathBuf, FileListError> {
    if requested.is_empty() {
        return Ok(root.to_path_buf());
    }
    match resolve_within_root(root, requested) {
        Resolved::Inside(target) => Ok(target),
        Resolved::Outside { .. } => Err(outside_or_enotdir(root, requested)),
    }
}

/// Map an `Outside` verdict onto the error shape `file/list` documents.
///
/// The shared resolver fails closed, and `ENOTDIR` (a trail under a path
/// component that is a regular file, e.g. `a.txt/sub`) is one of the failures it
/// cannot tell apart from an escape. For `file/list` that collapse is a
/// client-visible drift (BUG-003): the base behaviour was `-32603` + the real
/// io reason, not `-32807 outside_root`. Disambiguate on the error path only —
/// never on the happy path — and only when the descent tripped over a regular
/// file that really lives inside the root, so genuine escapes and unstatable
/// ancestors (EACCES) keep failing closed. A spelling with any `..` also keeps
/// failing closed: once symlinks are followed it can leave the root (BUG-006).
fn outside_or_enotdir(root: &Path, requested: &str) -> FileListError {
    if let Some(reason) = enotdir_in_root(root, requested) {
        return FileListError::Io {
            path: requested.to_string(),
            reason,
        };
    }
    outside_root(requested)
}

/// Real `ENOTDIR` reason when `requested` descends past a regular file **inside**
/// the root; `None` keeps the fail-closed `OutsideRoot` verdict.
fn enotdir_in_root(root: &Path, requested: &str) -> Option<String> {
    let inside = spelled_in_root(root, requested)?;
    let error = std::fs::metadata(&inside).err()?;
    if error.kind() != std::io::ErrorKind::NotADirectory {
        return None;
    }
    // The offending component must canonicalize inside the root: a symlink that
    // leaves it is an escape, not an io failure.
    let canonical = deepest_stat(&inside)?.canonicalize().ok()?;
    canonical.starts_with(root).then(|| error.to_string())
}

/// Deepest prefix of `path` that still stats — for an `ENOTDIR` request that is
/// the regular file the descent tripped over.
fn deepest_stat(path: &Path) -> Option<PathBuf> {
    let mut probe = path;
    loop {
        if std::fs::metadata(probe).is_ok() {
            return Some(probe.to_path_buf());
        }
        probe = probe.parent()?;
    }
}

/// Root-anchored form of `requested` when its spelling cannot climb above the
/// root (`None` keeps the caller fail-closed).
///
/// Anything carrying a `..` component is refused outright: the OS resolves `..`
/// *after* following symlinks, so `link/../a.txt` can land outside the root
/// (BUG-006) and no lexical flattening may pretend otherwise. A genuine
/// `ENOTDIR` spelling (`a.txt/sub`) never needs a `..`.
fn spelled_in_root(root: &Path, requested: &str) -> Option<PathBuf> {
    let path = Path::new(requested);
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return None;
    }
    let inside = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    inside.starts_with(root).then_some(inside)
}

/// Map a metadata/canonicalization failure: missing → `NotFound`, else `Io`.
fn io_or_not_found(label: &str, error: &std::io::Error) -> FileListError {
    if error.kind() == std::io::ErrorKind::NotFound {
        FileListError::NotFound
    } else {
        FileListError::Io {
            path: label.to_string(),
            reason: error.to_string(),
        }
    }
}

fn outside_root(path: &str) -> FileListError {
    FileListError::OutsideRoot {
        path: path.to_string(),
    }
}

/// Raw porcelain `(index, worktree)` pair lookup for a root-relative path.
pub fn git_statuses(snapshot: &GitSnapshot, rel: &str) -> Option<(char, char)> {
    snapshot.statuses.get(rel).copied()
}

/// Map a porcelain `(X, Y)` pair to the wire status (design contract item 1):
/// `??`/`A ` → `A`; ` D`/`D ` → `D`; conflicts (`UU/AA/DD/AU/UA/DU/UD`) → `U`;
/// other non-clean states → `M`; clean → `None`.
pub fn map_git_status(x: char, y: char) -> Option<GitFileStatus> {
    match (x, y) {
        (' ', ' ') => None,
        ('?', '?') => Some(GitFileStatus::A),
        ('A', ' ') => Some(GitFileStatus::A),
        (' ', 'D') | ('D', ' ') => Some(GitFileStatus::D),
        ('U', 'U' | 'A' | 'D') | ('A', 'U' | 'A') | ('D', 'U' | 'D') => Some(GitFileStatus::U),
        _ => Some(GitFileStatus::M),
    }
}

/// Aggregate the highest-priority git status among the descendants of `dir`.
/// Only root-relative statuses strictly under `"{dir}/"` count (so `srcx/...`
/// never affects `src`); the ladder is `U > D > M > A` (then `Unknown`), and a
/// directory with no status-bearing descendants yields `None`.
pub fn aggregate_dir_status(
    dir: &str,
    statuses: &std::collections::HashMap<String, (char, char)>,
) -> Option<GitFileStatus> {
    if dir.is_empty() {
        return None;
    }
    let prefix = format!("{dir}/");
    let mut best: Option<GitFileStatus> = None;
    for (path, (x, y)) in statuses {
        if !path.starts_with(&prefix) {
            continue;
        }
        if let Some(status) = map_git_status(*x, *y)
            && best
                .as_ref()
                .is_none_or(|current| status_priority(&status) < status_priority(current))
        {
            best = Some(status);
        }
    }
    best
}

/// Priority ladder for [`aggregate_dir_status`] (`U=0` best … `Unknown=4`).
fn status_priority(status: &GitFileStatus) -> u8 {
    match status {
        GitFileStatus::U => 0,
        GitFileStatus::D => 1,
        GitFileStatus::M => 2,
        GitFileStatus::A => 3,
        GitFileStatus::Unknown => 4,
    }
}

/// BFS state: each queue item is (absolute directory, root-relative prefix,
/// levels still allowed below it).
struct Walk {
    root: PathBuf,
    max_depth: u32,
    entries: Vec<FileEntry>,
    queue: VecDeque<(PathBuf, String, u32)>,
    truncated: bool,
}

impl Walk {
    fn new(root: PathBuf, max_depth: u32) -> Self {
        Self {
            root,
            max_depth,
            entries: Vec::new(),
            queue: VecDeque::new(),
            truncated: false,
        }
    }

    /// Enumerate `target`; an unreadable target fails the request, while an
    /// unreadable nested directory is skipped with a debug log.
    fn run(&mut self, target: &Path, label: &str, prefix: &str) -> Result<(), FileListError> {
        let children = read_sorted(target).map_err(|e| FileListError::Io {
            path: label.to_string(),
            reason: e.to_string(),
        })?;
        self.process(target, prefix, &children, self.max_depth);
        while let Some((dir, prefix, remaining)) = self.queue.pop_front() {
            if self.truncated {
                break;
            }
            match read_sorted(&dir) {
                Ok(children) => self.process(&dir, &prefix, &children, remaining),
                Err(e) => tracing::debug!(
                    dir = %dir.display(),
                    error = %e,
                    "file/list skips unreadable subdirectory"
                ),
            }
        }
        Ok(())
    }

    /// Append one level directories-first then name-ascending, queueing child
    /// directories for the next level while the entry cap is not reached.
    fn process(
        &mut self,
        _dir: &Path,
        prefix: &str,
        children: &[std::fs::DirEntry],
        remaining: u32,
    ) {
        let mut dirs: Vec<(FileEntry, PathBuf)> = Vec::new();
        let mut files: Vec<FileEntry> = Vec::new();
        for child in children {
            if child.file_name() == GIT_DIR {
                continue;
            }
            let abs = child.path();
            let Some(entry) = self.entry_for(&abs, prefix, &child.file_name()) else {
                continue;
            };
            if self.entries.len() + dirs.len() + files.len() >= ENTRY_LIMIT {
                self.truncated = true;
                break;
            }
            if entry.kind == FileKind::Dir {
                dirs.push((entry, abs));
            } else {
                files.push(entry);
            }
        }
        for (entry, abs) in dirs {
            if remaining > 1 {
                self.queue
                    .push_back((abs, entry.path.clone(), remaining - 1));
            }
            self.entries.push(entry);
        }
        self.entries.extend(files);
    }

    /// Build one entry through the shared constructor below.
    fn entry_for(&self, abs: &Path, prefix: &str, name: &std::ffi::OsStr) -> Option<FileEntry> {
        entry_for(&self.root, abs, prefix, name)
    }

    fn finish(self) -> FileListResult {
        FileListResult {
            entries: self.entries,
            truncated: self.truncated,
            total: None,
        }
    }
}

/// Build one entry; `None` skips entries whose metadata fails or whose
/// canonical path escapes `root` (out-of-root symlinks).
fn entry_for(root: &Path, abs: &Path, prefix: &str, name: &std::ffi::OsStr) -> Option<FileEntry> {
    let canonical = abs.canonicalize().ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    let metadata = std::fs::metadata(abs).ok()?;
    let name = name.to_string_lossy().into_owned();
    let path = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{prefix}/{name}")
    };
    Some(FileEntry {
        name,
        path,
        kind: if metadata.is_dir() {
            FileKind::Dir
        } else {
            FileKind::File
        },
        size: metadata.is_file().then_some(metadata.len()),
        mtime_ms: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64),
        git_status: None,
    })
}

/// Directory children sorted by file name; entry classification re-orders
/// each level directories-first afterwards.
fn read_sorted(dir: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    let mut children: Vec<std::fs::DirEntry> =
        std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    children.sort_by_key(|entry| entry.file_name());
    Ok(children)
}

/// Query-mode search over the `path` subtree (design item 3): `.git` is
/// skipped, escaping symlinks are dropped and the sandbox rules match [`list`].
pub fn search(
    root: &Path,
    path: Option<&str>,
    query: &str,
) -> Result<FileListResult, FileListError> {
    search_with_budget(root, path, query, SEARCH_VISIT_LIMIT)
}

/// [`search`] with an injectable visit budget; the scan stops after
/// `visit_limit` entries, sets `truncated` and reports `total` as a lower bound.
pub(crate) fn search_with_budget(
    root: &Path,
    path: Option<&str>,
    query: &str,
    visit_limit: usize,
) -> Result<FileListResult, FileListError> {
    let scope = resolve_scope(root, path)?;
    let mut scan = SearchScan {
        root: &scope.root,
        needle: query.to_lowercase(),
        visit_limit,
        visited: 0,
        budget_hit: false,
        matches: Vec::new(),
    };
    scan.run(&scope.target, &scope.label, &scope.prefix)?;
    Ok(scan.finish())
}

/// Rank a search hit for an already-lowercased `needle`:
/// `0` = name prefix, `1` = name substring, `2` = path substring, `None` = no match.
///
/// **Single source of truth for file-search ranking.** The cross-language
/// conformance contract lives in `protocol/search-rank-cases.json`; this crate's
/// tests and the TUI mock kernel are both pinned to it.
fn search_rank(name: &str, path: &str, needle: &str) -> Option<u8> {
    let name = name.to_lowercase();
    if name.starts_with(needle) {
        return Some(0);
    }
    if name.contains(needle) {
        return Some(1);
    }
    path.to_lowercase().contains(needle).then_some(2)
}

/// BFS scan for [`search`]: visits entries (directories included) without a
/// depth cap, descends into directories and keeps matching files only.
struct SearchScan<'a> {
    root: &'a Path,
    needle: String,
    visit_limit: usize,
    visited: usize,
    budget_hit: bool,
    matches: Vec<(u8, FileEntry)>,
}

impl SearchScan<'_> {
    /// Scan `target`; an unreadable target fails the request, while an
    /// unreadable nested directory is skipped with a debug log.
    fn run(&mut self, target: &Path, label: &str, prefix: &str) -> Result<(), FileListError> {
        let children = read_sorted(target).map_err(|e| FileListError::Io {
            path: label.to_string(),
            reason: e.to_string(),
        })?;
        let mut queue: VecDeque<(PathBuf, String)> = VecDeque::new();
        self.process(prefix, &children, &mut queue);
        while let Some((dir, prefix)) = queue.pop_front() {
            if self.budget_hit {
                break;
            }
            match read_sorted(&dir) {
                Ok(children) => self.process(&prefix, &children, &mut queue),
                Err(e) => tracing::debug!(
                    dir = %dir.display(),
                    error = %e,
                    "file/list search skips unreadable subdirectory"
                ),
            }
        }
        Ok(())
    }

    /// Examine one level in name order; each non-`.git` child costs one visit
    /// and an exhausted budget stops the scan (`truncated`).
    fn process(
        &mut self,
        prefix: &str,
        children: &[std::fs::DirEntry],
        queue: &mut VecDeque<(PathBuf, String)>,
    ) {
        for child in children {
            if child.file_name() == GIT_DIR {
                continue;
            }
            if self.visited >= self.visit_limit {
                self.budget_hit = true;
                return;
            }
            self.visited += 1;
            let abs = child.path();
            let Some(entry) = entry_for(self.root, &abs, prefix, &child.file_name()) else {
                continue;
            };
            if entry.kind == FileKind::Dir {
                queue.push_back((abs, entry.path.clone()));
            } else if let Some(rank) = search_rank(&entry.name, &entry.path, &self.needle) {
                self.matches.push((rank, entry));
            }
        }
    }

    /// Rank-then-path order, Top-[`SEARCH_RESULT_LIMIT`] cap; `total` counts
    /// every match before the cap and `truncated` reflects the visit budget.
    fn finish(mut self) -> FileListResult {
        self.matches
            .sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
        let total = self.matches.len() as u64;
        self.matches.truncate(SEARCH_RESULT_LIMIT);
        FileListResult {
            entries: self.matches.into_iter().map(|(_, entry)| entry).collect(),
            truncated: self.budget_hit,
            total: Some(total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    /// Unique per-test temp dir (std-only: the crate has no tempfile dependency).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aj-ipc-filelist-{tag}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn paths(result: &FileListResult) -> Vec<String> {
        result.entries.iter().map(|e| e.path.clone()).collect()
    }

    #[test]
    fn list_root_is_sorted_and_excludes_git() {
        let root = temp_dir("sorted");
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("HEAD"), "x").unwrap();
        fs::create_dir(root.join("zeta")).unwrap();
        fs::write(root.join("a.rs"), "hello").unwrap();
        let result = list(&root, None, None).unwrap();
        let names: Vec<&str> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["src", "zeta", "a.rs"], "dirs first, then name asc");
        assert!(
            result.entries.iter().all(|e| e.name != ".git"),
            ".git must be excluded"
        );
        assert_eq!(result.entries[0].kind, agentj_protocol::file::FileKind::Dir);
        assert_eq!(result.entries[2].path, "a.rs");
        assert_eq!(result.entries[2].size, Some(5));
        assert!(result.entries[2].mtime_ms.is_some(), "mtime from metadata");
        assert!(!result.truncated);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn escape_via_dotdot_is_rejected() {
        let root = temp_dir("dotdot");
        for bad in ["..", "../x", "a/../../x", "src/../../../etc"] {
            match list(&root, Some(bad), None).unwrap_err() {
                FileListError::OutsideRoot { path } => assert_eq!(path, bad),
                other => panic!("{bad} must be OutsideRoot, got {other:?}"),
            }
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absolute_path_outside_root_is_rejected() {
        let root = temp_dir("abs");
        let outside = temp_dir("abs-outside");
        let bad = outside.to_string_lossy().into_owned();
        match list(&root, Some(&bad), None).unwrap_err() {
            FileListError::OutsideRoot { path } => assert_eq!(path, bad),
            other => panic!("absolute outside must be OutsideRoot, got {other:?}"),
        }
        // An absolute path inside the root is not a sandbox violation.
        fs::write(root.join("in.txt"), "x").unwrap();
        let inside = root.join("in.txt").to_string_lossy().into_owned();
        assert!(matches!(
            list(&root, Some(&inside), None),
            Err(FileListError::NotADirectory)
        ));
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_entry_is_skipped() {
        let root = temp_dir("symlink");
        fs::write(root.join("ok.txt"), "x").unwrap();
        std::os::unix::fs::symlink("/etc", root.join("link_out")).unwrap();
        let result = list(&root, None, None).unwrap();
        assert!(
            result.entries.iter().all(|e| e.name != "link_out"),
            "escaping symlink must be skipped"
        );
        assert!(
            result.entries.iter().any(|e| e.name == "ok.txt"),
            "regular entries must survive"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn metadata_failure_skips_entry() {
        let root = temp_dir("meta");
        fs::write(root.join("ok.txt"), "x").unwrap();
        std::os::unix::fs::symlink(root.join("gone"), root.join("broken")).unwrap();
        let result = list(&root, None, None).unwrap();
        assert!(
            result.entries.iter().all(|e| e.name != "broken"),
            "unreadable metadata entry must be skipped"
        );
        assert!(result.entries.iter().any(|e| e.name == "ok.txt"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn depth_controls_recursion() {
        let root = temp_dir("depth");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/c.txt"), "x").unwrap();
        assert_eq!(paths(&list(&root, None, None).unwrap()), ["a"]);
        assert_eq!(
            paths(&list(&root, None, Some(2)).unwrap()),
            ["a", "a/b"],
            "depth 2 reaches one level below the target children"
        );
        assert_eq!(
            paths(&list(&root, None, Some(3)).unwrap()),
            ["a", "a/b", "a/b/c.txt"]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn subdirectory_paths_stay_root_relative() {
        let root = temp_dir("subdir-prefix");
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/main.rs"), "x").unwrap();
        assert_eq!(
            paths(&list(&root, Some("src"), Some(2)).unwrap()),
            ["src/nested", "src/main.rs"],
            "subdirectory entries keep the root-relative prefix"
        );
        assert_eq!(
            paths(&list(&root, None, None).unwrap()),
            ["src"],
            "the parent listing returns the directory itself"
        );
        for variant in ["./src", "src/"] {
            assert_eq!(
                paths(&list(&root, Some(variant), Some(1)).unwrap()),
                ["src/nested", "src/main.rs"],
                "prefix normalization for {variant}"
            );
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn depth_and_truncation_respected() {
        let root = temp_dir("trunc");
        for i in 0..ENTRY_LIMIT + 5 {
            fs::write(root.join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let result = list(&root, None, Some(1)).unwrap();
        assert_eq!(result.entries.len(), ENTRY_LIMIT, "capped at the limit");
        assert!(result.truncated, "limit exceeded → truncated=true");
        // Exactly at the limit is not truncation.
        let exact = temp_dir("trunc-exact");
        for i in 0..ENTRY_LIMIT {
            fs::write(exact.join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let result = list(&exact, None, Some(1)).unwrap();
        assert_eq!(result.entries.len(), ENTRY_LIMIT);
        assert!(!result.truncated, "exactly at the limit is not truncation");
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&exact).ok();
    }

    #[test]
    fn depth_over_eight_rejected() {
        let root = temp_dir("depth-bad");
        for bad in [0, 9, u32::MAX] {
            assert!(
                matches!(
                    list(&root, None, Some(bad)),
                    Err(FileListError::InvalidDepth)
                ),
                "depth {bad} must be InvalidDepth"
            );
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_path_is_not_found() {
        let root = temp_dir("missing");
        assert!(matches!(
            list(&root, Some("nope"), None),
            Err(FileListError::NotFound)
        ));
        assert!(matches!(
            list(&root, Some("nested/nope"), None),
            Err(FileListError::NotFound)
        ));
        fs::remove_dir_all(&root).ok();
    }

    /// A symlink inside the root that points outside it is still an escape.
    /// Pinned as a characterization test before `resolve_scope` switched to
    /// `agentj_core::paths::resolve_within_root`.
    #[cfg(unix)]
    #[test]
    fn symlinked_scope_escape_is_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        assert!(matches!(
            list(&root, Some("link"), Some(1)),
            Err(FileListError::OutsideRoot { .. })
        ));
    }

    /// Rows listed under a symlinked directory keep the spelling the client
    /// asked for (BUG-002): clients derive parents from `entry.path`, so a
    /// canonicalized row under `src/sub` would stop matching the `src_alias`
    /// node they expanded. Safety is unchanged — the walk still reads the
    /// canonical target gated by `resolve_within_root`.
    #[cfg(unix)]
    #[test]
    fn symlinked_directory_keeps_requested_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("src/sub")).unwrap();
        fs::write(root.join("src/sub/a.rs"), "x").unwrap();
        std::os::unix::fs::symlink(root.join("src"), root.join("src_alias")).unwrap();
        assert_eq!(
            paths(&list(&root, Some("src_alias"), Some(2)).unwrap()),
            ["src_alias/sub", "src_alias/sub/a.rs"],
            "rows keep the spelling the client expanded"
        );
        assert_eq!(
            paths(&list(&root, Some("src_alias/sub"), Some(1)).unwrap()),
            ["src_alias/sub/a.rs"],
            "a nested request keeps its own spelling"
        );
        assert!(
            matches!(
                list(&root, Some("src_alias/../.."), Some(1)),
                Err(FileListError::OutsideRoot { .. })
            ),
            "the alias must not become an escape hatch"
        );
    }

    /// A path that simply does not exist stays `NotFound` — it must not be
    /// promoted to `OutsideRoot` by the shared resolver.
    #[test]
    fn missing_scope_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert!(matches!(
            list(&root, Some("missing"), Some(1)),
            Err(FileListError::NotFound)
        ));
    }

    #[test]
    fn file_path_is_not_a_directory() {
        let root = temp_dir("file");
        fs::write(root.join("a.txt"), "x").unwrap();
        assert!(matches!(
            list(&root, Some("a.txt"), None),
            Err(FileListError::NotADirectory)
        ));
        fs::remove_dir_all(&root).ok();
    }

    /// BUG-003: a trail under a **regular file** is an io failure, not a sandbox
    /// verdict. The base commit canonicalized the request directly and reported
    /// `-32603` + the real `ENOTDIR` reason; the shared fail-closed resolver
    /// answers `Outside` for anything it cannot canonicalize, which silently
    /// moved this shape to `-32807 outside_root`. The documented `file/list`
    /// error split (io vs. outside) must not drift.
    #[test]
    fn path_under_a_regular_file_is_io_not_outside_root() {
        let root = temp_dir("file-sub");
        fs::write(root.join("a.txt"), "x").unwrap();
        match list(&root, Some("a.txt/sub"), None).unwrap_err() {
            FileListError::Io { path, reason } => {
                assert_eq!(path, "a.txt/sub", "io label is the requested spelling");
                assert!(!reason.is_empty(), "io reason must be carried");
                #[cfg(unix)]
                assert!(
                    reason.contains("Not a directory"),
                    "reason must report ENOTDIR, got {reason:?}"
                );
            }
            other => panic!("a path under a regular file must stay Io, got {other:?}"),
        }
        fs::remove_dir_all(&root).ok();
    }

    /// The BUG-003 disambiguation must only widen io for `ENOTDIR`: real escapes
    /// stay `OutsideRoot` and a missing path stays `NotFound`.
    #[test]
    fn escape_and_missing_verdicts_are_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(root.join("a.txt"), "x").unwrap();
        assert!(matches!(
            list(&root, Some(".."), None),
            Err(FileListError::OutsideRoot { .. })
        ));
        assert!(matches!(
            list(&root, Some("a.txt/../.."), None),
            Err(FileListError::OutsideRoot { .. })
        ));
        assert!(matches!(
            list(&root, Some("nope"), None),
            Err(FileListError::NotFound)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_with_file_tail_is_still_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(outside.path().join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        assert!(matches!(
            list(&root, Some("link/secret.txt/sub"), None),
            Err(FileListError::OutsideRoot { .. })
        ));
    }

    /// BUG-006: `link/../a.txt/sub` resolves to `<outside>/a.txt/sub`, i.e. the
    /// request leaves the root, so the verdict must be `OutsideRoot` — the same
    /// verdict `link/..` already gets. Lexically flattening the `..` inside the
    /// root made the ENOTDIR mapping fire and report `Io` instead.
    #[cfg(unix)]
    #[test]
    fn dotdot_through_a_symlink_stays_outside_root() {
        let base = temp_dir("dotdot-symlink");
        let root = base.join("ws");
        let outside = base.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("a.txt"), "inside").unwrap();
        fs::write(outside.join("a.txt"), "outside").unwrap();
        std::os::unix::fs::symlink("../outside", root.join("link")).unwrap();

        for escaping in ["link/..", "link/../a.txt/sub", "sub/../a.txt/sub"] {
            assert!(
                matches!(
                    list(&root, Some(escaping), None),
                    Err(FileListError::OutsideRoot { .. })
                ),
                "{escaping} escapes the root and must stay OutsideRoot, got {:?}",
                list(&root, Some(escaping), None).unwrap_err()
            );
        }

        // Genuine ENOTDIR keeps the io mapping (BUG-003 must not regress).
        match list(&root, Some("a.txt/sub"), None).unwrap_err() {
            FileListError::Io { reason, .. } => assert!(
                reason.contains("Not a directory"),
                "reason must report ENOTDIR, got {reason:?}"
            ),
            other => panic!("a.txt/sub must stay Io, got {other:?}"),
        }
        fs::remove_dir_all(&base).ok();
    }

    /// An ancestor that cannot be stat'ed (EACCES) is *not* an `ENOTDIR` io
    /// failure: the sandbox cannot prove containment, so it keeps failing closed.
    #[cfg(unix)]
    #[test]
    fn unsearchable_ancestor_stays_outside_root() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let locked = root.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::metadata(locked.join("sub")).is_ok() {
            // Privileged runner: permission bits are not enforced → unexercisable.
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let verdict = list(&root, Some("locked/sub"), None).unwrap_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(verdict, FileListError::OutsideRoot { .. }),
            "EACCES must stay OutsideRoot, got {verdict:?}"
        );
    }

    #[test]
    fn git_status_mapping_follows_porcelain() {
        for (xy, want) in [
            (('?', '?'), Some(GitFileStatus::A)),
            (('A', ' '), Some(GitFileStatus::A)),
            ((' ', 'D'), Some(GitFileStatus::D)),
            (('D', ' '), Some(GitFileStatus::D)),
            (('U', 'U'), Some(GitFileStatus::U)),
            (('A', 'A'), Some(GitFileStatus::U)),
            (('D', 'D'), Some(GitFileStatus::U)),
            (('A', 'U'), Some(GitFileStatus::U)),
            (('U', 'A'), Some(GitFileStatus::U)),
            (('D', 'U'), Some(GitFileStatus::U)),
            (('U', 'D'), Some(GitFileStatus::U)),
            ((' ', 'M'), Some(GitFileStatus::M)),
            (('M', ' '), Some(GitFileStatus::M)),
            (('A', 'M'), Some(GitFileStatus::M)),
            (('R', ' '), Some(GitFileStatus::M)),
            ((' ', ' '), None),
        ] {
            assert_eq!(map_git_status(xy.0, xy.1), want, "porcelain {xy:?}");
        }
        let mut statuses = HashMap::new();
        statuses.insert("a.rs".to_string(), (' ', 'M'));
        let snapshot = GitSnapshot {
            top: PathBuf::from("/r"),
            statuses,
            numstat: HashMap::new(),
        };
        assert_eq!(git_statuses(&snapshot, "a.rs"), Some((' ', 'M')));
        assert_eq!(git_statuses(&snapshot, "absent"), None);
    }

    #[test]
    fn dir_status_aggregates_descendants_by_priority() {
        let mut statuses = HashMap::new();
        statuses.insert("src/m.rs".to_string(), (' ', 'M'));
        statuses.insert("src/deep/d.rs".to_string(), (' ', 'D'));
        statuses.insert("src/add.rs".to_string(), ('?', '?'));
        assert_eq!(
            aggregate_dir_status("src", &statuses),
            Some(GitFileStatus::D)
        );
        statuses.insert("src/conflict.rs".to_string(), ('U', 'U'));
        assert_eq!(
            aggregate_dir_status("src", &statuses),
            Some(GitFileStatus::U)
        );
        statuses.remove("src/conflict.rs");
        statuses.remove("src/deep/d.rs");
        assert_eq!(
            aggregate_dir_status("src", &statuses),
            Some(GitFileStatus::M)
        );
        assert_eq!(aggregate_dir_status("srcx", &statuses), None);
        assert_eq!(aggregate_dir_status("src/deep", &statuses), None);
        assert_eq!(
            aggregate_dir_status("src", &Default::default()),
            None,
            "no statuses = no dir status"
        );
    }

    fn search_tree(tag: &str, files: &[&str]) -> PathBuf {
        let root = temp_dir(tag);
        for rel in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x").unwrap();
        }
        root
    }

    #[test]
    fn search_ranks_prefix_over_substring_over_path() {
        let root = search_tree(
            "search-rank",
            &[
                "src/main.rs",
                "src/domain/domain_main.rs",
                "docs/main-guide/readme.md",
            ],
        );
        let result = search(&root, None, "main").unwrap();
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "src/main.rs",
                "src/domain/domain_main.rs",
                "docs/main-guide/readme.md",
            ],
            "rank: name prefix > name substring > path substring"
        );
        assert_eq!(result.total, Some(3));
        assert!(!result.truncated);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_is_case_insensitive_and_path_scoped() {
        let root = search_tree("search-scope", &["src/MAIN.rs", "other/main.rs"]);
        let scoped = search(&root, Some("src"), "main").unwrap();
        assert_eq!(scoped.total, Some(1));
        assert_eq!(scoped.entries[0].path, "src/MAIN.rs");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_reports_truncation_when_the_visit_budget_runs_out() {
        let root = search_tree("search-budget", &["a/x.rs", "b/y.rs", "c/z.rs"]);
        let result = search_with_budget(&root, None, "rs", 2).unwrap();
        assert!(
            result.truncated,
            "exhausted budget must mark the scan partial"
        );
        assert!(result.total.is_some(), "total is reported as a lower bound");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_caps_entries_at_the_limit_and_keeps_total() {
        let root = temp_dir("search-cap");
        for i in 0..SEARCH_RESULT_LIMIT + 5 {
            fs::write(root.join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let result = search(&root, None, "f").unwrap();
        assert_eq!(result.entries.len(), SEARCH_RESULT_LIMIT);
        assert_eq!(result.total, Some((SEARCH_RESULT_LIMIT + 5) as u64));
        assert!(!result.truncated, "the result cap is not a scan truncation");
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn search_skips_git_and_escaping_symlinks() {
        let root = temp_dir("search-skip");
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("config.rs"), "x").unwrap();
        fs::write(root.join("keep.rs"), "x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd.rs", root.join("escape.rs")).unwrap();
        let result = search(&root, None, "rs").unwrap();
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["keep.rs"]);
        assert_eq!(result.total, Some(1));
        fs::remove_dir_all(&root).ok();
    }

    #[derive(serde::Deserialize)]
    struct RankCases {
        version: u32,
        ranks: Vec<RankCase>,
    }

    #[derive(serde::Deserialize)]
    struct RankCase {
        name: String,
        path: String,
        query: String,
        expected_rank: Option<u8>,
    }

    /// `protocol/` is the repo's cross-language contract directory; anchor on
    /// CARGO_MANIFEST_DIR because cargo runs tests with cwd = the crate dir.
    fn search_rank_cases_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/search-rank-cases.json")
    }

    fn load_rank_cases() -> RankCases {
        let path = search_rank_cases_path();
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let cases: RankCases =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        assert_eq!(cases.version, 1, "unexpected fixture version");
        cases
    }

    #[test]
    fn search_rank_matches_the_shared_conformance_fixture() {
        let cases = load_rank_cases();
        assert!(
            !cases.ranks.is_empty(),
            "fixture declares no ranks[] entries"
        );
        for case in &cases.ranks {
            assert_eq!(
                search_rank(&case.name, &case.path, &case.query.to_lowercase()),
                case.expected_rank,
                "rank mismatch for name={:?} path={:?} query={:?}",
                case.name,
                case.path,
                case.query
            );
        }
        let mut seen: Vec<Option<u8>> = cases.ranks.iter().map(|c| c.expected_rank).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen,
            vec![None, Some(0), Some(1), Some(2)],
            "the fixture must pin all four rank outcomes"
        );
        let raw = fs::read_to_string(search_rank_cases_path()).unwrap();
        for token in [
            "name-prefix",
            "name-substring",
            "path-substring",
            "case-insensitive",
            "path byte-ascending",
        ] {
            assert!(raw.contains(token), "fixture rule string omits {token:?}");
        }
    }

    #[derive(serde::Deserialize)]
    struct SearchCases {
        version: u32,
        searches: Vec<SearchCase>,
    }

    #[derive(serde::Deserialize)]
    struct SearchCase {
        query: String,
        entries: Vec<FixtureEntry>,
        expected_ranked_paths: Vec<String>,
        expected_total: u64,
    }

    #[derive(serde::Deserialize)]
    struct FixtureEntry {
        path: String,
        kind: String,
    }

    fn load_search_cases() -> SearchCases {
        let path = search_rank_cases_path();
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let cases: SearchCases =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        assert_eq!(cases.version, 1, "unexpected fixture version");
        cases
    }

    /// Materialize a fixture case as a real tree. Directories become dirs so the
    /// "directories are never ranked" contract is exercised by the fixture itself.
    fn materialize_fixture_entries(tag: &str, entries: &[FixtureEntry]) -> PathBuf {
        let root = temp_dir(tag);
        for entry in entries {
            let full = root.join(&entry.path);
            if entry.kind == "directory" {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, "x").unwrap();
            }
        }
        root
    }

    #[test]
    fn search_ordering_matches_the_shared_conformance_fixture() {
        let cases = load_search_cases();
        assert!(
            !cases.searches.is_empty(),
            "fixture declares no searches[] entries"
        );
        assert!(
            cases.searches.iter().any(|c| {
                let declared: Vec<&str> = c.entries.iter().map(|e| e.path.as_str()).collect();
                let expected: Vec<&str> =
                    c.expected_ranked_paths.iter().map(String::as_str).collect();
                declared != expected
            }),
            "every fixture case is already pre-sorted; the assertion cannot detect an echo"
        );
        for (i, case) in cases.searches.iter().enumerate() {
            let root = materialize_fixture_entries(&format!("search-fixture-{i}"), &case.entries);
            let result = search(&root, None, &case.query).unwrap();
            assert_eq!(
                paths(&result),
                case.expected_ranked_paths,
                "query={:?}",
                case.query
            );
            assert_eq!(
                result.total,
                Some(case.expected_total),
                "query={:?}",
                case.query
            );
            fs::remove_dir_all(&root).ok();
        }
    }
}
