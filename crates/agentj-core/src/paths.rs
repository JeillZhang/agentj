//! One place decides whether a requested path stays inside a workspace root.
//!
//! Both the permission engine (`eval_path`), the tool-argument rebaser
//! (`tool_paths`) and `file/list` scope resolution need the same verdict, and
//! the naive lexical `starts_with` they used to duplicate got two cases wrong:
//! it followed no symlinks (so `link/secret` pointing outside the root read as
//! inside) and it rejected paths whose parents do not exist yet (so writing
//! `new/dir/file.rs` was denied).
//!
//! Contract:
//! - `canonicalize` whenever the filesystem can answer; `Inside` carries the
//!   canonical absolute path so callers get link resolution for free.
//! - A nonexistent target is *not* an escape: canonicalize the nearest existing
//!   ancestor and append the missing tail lexically.
//! - Any other canonicalize failure (`EACCES`, `ELOOP`, ...) fails **closed**:
//!   `Outside`. A path we cannot verify must never read as allowed.
//! - A root that cannot be canonicalized itself (deleted/renamed project root)
//!   degrades to the lexical verdict instead of denying everything, so the
//!   operator still gets a usable answer.
//!
//! Never panics: no unwrap, no expect, no indexing.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Verdict for one requested path against a workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// Stays inside the root; carries the canonical absolute target.
    Inside(PathBuf),
    /// Escapes the root; carries the path as the caller spelled it.
    Outside { requested: String },
}

/// Resolve `requested` (relative to `root`, or absolute) and report whether it
/// stays inside `root`. See the module docs for the failure policy.
pub fn resolve_within_root(root: &Path, requested: &str) -> Resolved {
    let Ok(root_canon) = root.canonicalize() else {
        tracing::debug!(root = %root.display(), "root not resolvable; lexical fallback");
        return lexical_verdict(root, requested);
    };
    let raw = if requested.is_empty() {
        root_canon.clone()
    } else {
        root_canon.join(requested)
    };
    let joined = lexical_normalize(&raw);
    let resolved = match canonical_target(&joined) {
        Some(resolved) => resolved,
        None => {
            return Resolved::Outside {
                requested: requested.to_string(),
            };
        }
    };
    if resolved.starts_with(&root_canon) {
        Resolved::Inside(resolved)
    } else {
        Resolved::Outside {
            requested: requested.to_string(),
        }
    }
}

/// Canonical form of `joined`, resolving a not-yet-existing tail lexically.
/// Returns `None` when the path cannot be verified (fail closed).
fn canonical_target(joined: &Path) -> Option<PathBuf> {
    match joined.canonicalize() {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => resolve_missing_tail(joined),
        Err(error) => {
            tracing::debug!(target = %joined.display(), error = %error, "target not canonicalizable");
            None
        }
    }
}

/// Canonicalize the nearest existing ancestor of a nonexistent path and append
/// the missing components. `None` when no ancestor can be canonicalized.
fn resolve_missing_tail(joined: &Path) -> Option<PathBuf> {
    let (anchor, mut tail) = split_existing_ancestor(joined)?;
    let base = match anchor.canonicalize() {
        Ok(base) => base,
        Err(error) => {
            tracing::debug!(ancestor = %anchor.display(), error = %error, "ancestor not canonicalizable");
            return None;
        }
    };
    tail.reverse();
    Some(tail.iter().fold(base, |acc, name| acc.join(name)))
}

/// Walk up from `path` to the nearest component the filesystem knows about.
/// Returns that ancestor plus the missing names (deepest last), or `None` when
/// the parent chain looks up with an error other than "not found" (fail closed).
fn split_existing_ancestor(path: &Path) -> Option<(PathBuf, Vec<OsString>)> {
    let mut tail: Vec<OsString> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        match std::fs::symlink_metadata(&current) {
            Ok(_) => return Some((current, tail)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::debug!(ancestor = %current.display(), error = %error, "ancestor not statable");
                return None;
            }
        }
        let name = current.file_name()?.to_os_string();
        let parent = current.parent()?.to_path_buf();
        tail.push(name);
        current = parent;
    }
}

/// Verdict when the root itself cannot be canonicalized: compare lexically so a
/// deleted project root still allows escape-free paths (and still blocks
/// `..` escapes).
fn lexical_verdict(root: &Path, requested: &str) -> Resolved {
    let normalized = lexical_normalize(&root.join(requested));
    if normalized.starts_with(root) {
        Resolved::Inside(normalized)
    } else {
        Resolved::Outside {
            requested: requested.to_string(),
        }
    }
}

/// Lexically strip `.`/`..` components without touching the filesystem.
/// `/ws/a/../../x` → `/x`; `a/b/c` stays under `/ws`.
pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_of_relative_and_dotdot_cases() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(
            resolve_within_root(&root, ""),
            Resolved::Inside(root.clone())
        );
        assert_eq!(
            resolve_within_root(&root, "."),
            Resolved::Inside(root.clone())
        );
        assert!(matches!(
            resolve_within_root(&root, "/etc/passwd"),
            Resolved::Outside { .. }
        ));
        assert_eq!(
            resolve_within_root(&root, "src/main.rs"),
            Resolved::Inside(root.join("src/main.rs"))
        );
        assert!(matches!(
            resolve_within_root(&root, "../etc/passwd"),
            Resolved::Outside { .. }
        ));
        assert!(matches!(
            resolve_within_root(&root, "a/../../x"),
            Resolved::Outside { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn root_that_is_itself_a_symlink_still_resolves_inside() {
        let real = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(real.path(), parent.path().join("ws")).unwrap();
        let root = parent.path().join("ws");
        assert_eq!(
            resolve_within_root(&root, "src/main.rs"),
            Resolved::Inside(real.path().canonicalize().unwrap().join("src/main.rs"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_outside() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        assert!(matches!(
            resolve_within_root(&root, "link/secret"),
            Resolved::Outside { .. }
        ));
    }

    #[test]
    fn nonexistent_target_resolves_inside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve_within_root(&root, "new/dir/file.rs"),
            Resolved::Inside(root.join("new/dir/file.rs"))
        );
    }

    #[test]
    fn missing_root_falls_back_to_lexical() {
        let missing = std::path::Path::new("/definitely/not/here/agentj");
        assert!(matches!(
            resolve_within_root(missing, "../x"),
            Resolved::Outside { .. }
        ));
        assert!(matches!(
            resolve_within_root(missing, "src/main.rs"),
            Resolved::Inside(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_ancestor_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let verdict = resolve_within_root(&root, "locked/secret");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(verdict, Resolved::Outside { .. }));
    }
}
