//! `file/list` — sandboxed directory enumeration (design 2026-09-13, contract item 1).
//!
//! Requests resolve against the session `project_root`: the requested path is
//! lexically normalized, canonicalized, and must stay under the canonicalized
//! root (`-32807` otherwise). Enumeration is a BFS capped at [`MAX_DEPTH`]
//! levels and [`ENTRY_LIMIT`] entries; `.git` is never returned or descended,
//! escaping symlink entries are silently skipped, and per-level ordering is
//! directories-first then name-ascending.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use agentj_core::git_sample::GitSnapshot;
use agentj_protocol::file::{FileEntry, FileKind, FileListResult, GitFileStatus};

/// Enumeration depth cap (design contract item 1: `depth` 1..=8, default 1).
pub const MAX_DEPTH: u32 = 8;
/// Entry cap; exceeding it truncates the result and sets `truncated`.
pub const ENTRY_LIMIT: usize = 2000;

const GIT_DIR: &str = ".git";

/// `file/list` failure kinds (mapped onto JSON-RPC codes + `error.data` by the
/// dispatch layer).
#[derive(Debug, PartialEq, Eq)]
pub enum FileListError {
    /// Requested path does not exist (→ `-32602`, reason `not_found`).
    NotFound,
    /// Requested path exists but is not a directory (→ `-32602`, `not_a_directory`).
    NotADirectory,
    /// Requested path escapes the session project root (→ `-32807`).
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
    let mut walk = Walk::new(root, depth);
    walk.run(&target, &label, &prefix)?;
    Ok(walk.finish())
}

/// Root-relative prefix of the requested directory (`""` for the root itself),
/// so subdirectory listing entries keep paths relative to the project root.
fn requested_prefix(root: &Path, requested: &str, target: &Path) -> String {
    if requested.is_empty() {
        return String::new();
    }
    let relative = if Path::new(requested).is_absolute() {
        target.strip_prefix(root).ok().map(Path::to_path_buf)
    } else {
        lexical_join(root, Path::new(requested))
            .and_then(|joined| joined.strip_prefix(root).ok().map(Path::to_path_buf))
    };
    relative
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolve the requested path against `root`: lexical normalization rejects
/// `..` escapes before canonicalization; the canonical result must stay under
/// the canonical root.
fn resolve_target(root: &Path, requested: &str) -> Result<PathBuf, FileListError> {
    if requested.is_empty() {
        return Ok(root.to_path_buf());
    }
    let raw = Path::new(requested);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        lexical_join(root, raw).ok_or_else(|| outside_root(requested))?
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|e| io_or_not_found(requested, &e))?;
    if canonical.starts_with(root) {
        Ok(canonical)
    } else {
        Err(outside_root(requested))
    }
}

/// Join `requested` onto `root` after removing `.` and resolving `..`;
/// `None` when the path climbs above the root (sandbox escape).
fn lexical_join(root: &Path, requested: &Path) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in requested.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !relative.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(root.join(relative))
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

    /// Build one entry; `None` skips entries whose metadata fails or whose
    /// canonical path escapes the root (out-of-root symlinks).
    fn entry_for(&self, abs: &Path, prefix: &str, name: &std::ffi::OsStr) -> Option<FileEntry> {
        let canonical = abs.canonicalize().ok()?;
        if !canonical.starts_with(&self.root) {
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

    fn finish(self) -> FileListResult {
        FileListResult {
            entries: self.entries,
            truncated: self.truncated,
            total: None,
        }
    }
}

/// Directory children sorted by file name; entry classification re-orders
/// each level directories-first afterwards.
fn read_sorted(dir: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    let mut children: Vec<std::fs::DirEntry> =
        std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    children.sort_by_key(|entry| entry.file_name());
    Ok(children)
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
}
