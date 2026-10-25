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
//! - The path is walked **component by component**, canonicalizing every
//!   component that exists. That matters for `..`: it must pop the parent of
//!   what the link points AT, not the link component itself (`link/../x` is
//!   `<link target>/../x`, i.e. usually outside the root — BUG-001). A lexical
//!   pre-pass turns it into a plausible-looking root-relative path instead.
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
    if requested.is_empty() {
        return Resolved::Inside(root_canon);
    }
    let requested_path = Path::new(requested);
    // Relative args walk FROM the canonical root, so `..` can never pop above
    // it; absolute args walk from the filesystem root and the containment
    // check below decides.
    let walked = if requested_path.is_absolute() {
        walk(Path::new(""), requested_path, None)
    } else {
        walk(&root_canon, requested_path, Some(&root_canon))
    };
    match walked {
        Some(path) if path.starts_with(&root_canon) => Resolved::Inside(path),
        _ => Resolved::Outside {
            requested: requested.to_string(),
        },
    }
}

/// Walk `requested` from `start` (already canonical), canonicalizing every
/// component that exists so symlinks are followed *before* a later `..` is
/// applied. Once a component is missing the rest of the tail is appended
/// lexically (a nonexistent target is not an escape). `floor` is the directory
/// the walk must not pop above (the workspace root for relative args). `None`
/// means the path cannot be verified (fail closed).
fn walk(start: &Path, requested: &Path, floor: Option<&Path>) -> Option<PathBuf> {
    let mut acc = start.to_path_buf();
    let mut missing: Option<Vec<OsString>> = None;
    for component in requested.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !pop_component(&mut acc, &mut missing, floor) {
                    return None;
                }
            }
            Component::Normal(name) => match missing.as_mut() {
                Some(tail) => tail.push(name.to_os_string()),
                None => match acc.join(name).canonicalize() {
                    Ok(target) => acc = target,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing = Some(vec![name.to_os_string()]);
                    }
                    Err(error) => {
                        tracing::debug!(component = %name.to_string_lossy(), %error, "component not canonicalizable");
                        return None;
                    }
                },
            },
            // Only reachable for absolute args: (re)seed the filesystem root.
            Component::Prefix(_) | Component::RootDir => {
                acc.push(component.as_os_str());
                missing = None;
            }
        }
    }
    let tail = missing.unwrap_or_default();
    Some(tail.iter().fold(acc, |base, name| base.join(name)))
}

/// Undo one path component: drop the deepest missing name, else pop `acc`.
/// `false` when the walk would leave `floor` (or pop past the filesystem root).
fn pop_component(
    acc: &mut PathBuf,
    missing: &mut Option<Vec<OsString>>,
    floor: Option<&Path>,
) -> bool {
    if let Some(tail) = missing.as_mut() {
        if tail.pop().is_some() {
            if tail.is_empty() {
                *missing = None;
            }
            return true;
        }
        *missing = None;
    }
    if floor.is_some_and(|floor| acc == floor) {
        return false;
    }
    acc.pop()
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

    /// BUG-001: `..` after a symlink must pop the LINK TARGET's directory, not
    /// the link component — `<root>/link/../secret` names `/tmp/secret` for the
    /// OS, so a lexical pre-pass reported it as `<root>/secret` (inside).
    #[cfg(unix)]
    #[test]
    fn symlink_then_dotdot_escape_is_outside() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(outside.path().join("secret"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        for shape in ["link/../secret", "link/./../secret"] {
            assert_eq!(
                resolve_within_root(&root, shape),
                Resolved::Outside {
                    requested: shape.to_string()
                },
                "{shape} escapes through the symlink"
            );
        }
        let absolute = format!("{}/link/../secret", root.display());
        assert_eq!(
            resolve_within_root(&root, &absolute),
            Resolved::Outside {
                requested: absolute.clone()
            },
            "{absolute} escapes through the symlink"
        );
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
