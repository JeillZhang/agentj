//! In-process `glob` matcher and walker.
//!
//! This module is the single definition of `glob` pattern semantics: no second
//! matcher and no second copy of the skip list elsewhere in the crate.
//!
//! | pattern shape       | matched against                          | compiled with             |
//! |---------------------|------------------------------------------|---------------------------|
//! | contains `/` (path) | whole root-relative path (no `./` prefix)| `literal_separator(true)` |
//! | no `/` (basename)   | basename of every file                   | `literal_separator(true)` |
//!
//! `literal_separator(true)` is the key knob: `*` never crosses `/`, so `**` is
//! the only segment-crossing wildcard, in both modes. A basename pattern
//! therefore matches at any depth (the old `find -name` behaviour), while
//! `src/*.rs` does not match `src/a/b.rs`.

use std::path::Path;
use std::time::{Duration, Instant};

use agentj_core::error::ToolError;

use crate::builtins::GrepTool;

/// Mirrors `GrepTool::MAX_HITS`, so one pattern cannot flood a transcript.
pub(crate) const MAX_GLOB_ENTRIES: usize = 2_000;

#[derive(Debug)]
pub struct GlobMatcher {
    matcher: globset::GlobMatcher,
    path_mode: bool,
}

impl GlobMatcher {
    /// Compile a glob pattern. A pattern containing `/` is matched against the
    /// whole root-relative path with `literal_separator(true)` (`*` never crosses
    /// `/`, `**` is the only crosser); a pattern without `/` is matched against
    /// the basename of every file — the behaviour `find -name` gave us. Mirrors
    /// `GrepTool::include_matches`.
    pub fn new(pattern: &str) -> Result<Self, ToolError> {
        let raw = pattern
            .trim()
            .trim_start_matches("./")
            .trim_start_matches('/');
        let matcher = globset::GlobBuilder::new(raw)
            .literal_separator(true)
            .build()
            .map_err(|e| ToolError::ExecutionFailed {
                name: "glob".into(),
                reason: format!("invalid glob pattern: {e}"),
            })?
            .compile_matcher();
        Ok(Self {
            matcher,
            path_mode: raw.contains('/'),
        })
    }

    pub fn matches(&self, rel: &str) -> bool {
        if self.path_mode {
            return self.matcher.is_match(rel);
        }
        rel.rsplit('/')
            .next()
            .is_some_and(|base| self.matcher.is_match(base))
    }
}

/// Walk under `root` with `walkdir`, skipping [`GrepTool::SKIP_DIRS`] via
/// `filter_entry` (single source of truth). The deadline is re-read on every
/// iteration, so a tiny tree cannot silently ignore it, and the error carries
/// the configured timeout. Returns the capped paths plus the true hit count —
/// the caller needs the difference to print how many lines were dropped.
pub fn walk_with(
    root: &Path,
    matcher: &GlobMatcher,
    timeout: Duration,
) -> Result<(Vec<String>, usize), ToolError> {
    let deadline = Instant::now() + timeout;
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && GrepTool::SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        });
    let mut out = Vec::new();
    let mut matched = 0usize;
    for entry in walker {
        if Instant::now() >= deadline {
            return Err(ToolError::Timeout(timeout));
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if matcher.matches(&rel) {
            matched += 1;
            if out.len() < MAX_GLOB_ENTRIES {
                out.push(rel);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok((out, matched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn basename_pattern_matches_any_component() {
        let m = GlobMatcher::new("*.rs").unwrap();
        assert!(m.matches("src/main.rs"));
        assert!(m.matches("src/util/mod.rs"));
        assert!(!m.matches("src/main.txt"));
    }

    #[test]
    fn single_star_does_not_cross_separators() {
        assert!(GlobMatcher::new("src/*.rs").unwrap().matches("src/main.rs"));
        assert!(!GlobMatcher::new("src/*.rs").unwrap().matches("src/a/b.rs"));
        assert!(
            GlobMatcher::new("src/**/*.rs")
                .unwrap()
                .matches("src/a/b/c.rs")
        );
        assert!(
            !GlobMatcher::new("src/**/*.rs")
                .unwrap()
                .matches("tests/x.rs")
        );
    }

    #[test]
    fn invalid_pattern_is_a_tool_error() {
        match GlobMatcher::new("a[b") {
            Err(ToolError::ExecutionFailed { name, reason }) => {
                assert_eq!(name, "glob");
                assert!(reason.contains("invalid glob pattern"), "{reason}");
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn walk_skips_the_grep_skip_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut paths = vec!["keep.js".to_string()];
        for skip in GrepTool::SKIP_DIRS {
            paths.push(format!("{skip}/pkg/index.js"));
        }
        for p in &paths {
            let path = dir.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let m = GlobMatcher::new("*.js").unwrap();
        let (got, matched) = walk_with(dir.path(), &m, Duration::from_secs(5)).unwrap();
        // Every skip entry stays silent, so only the top-level file survives: dropping
        // any single entry would let that directory's `index.js` match and fail this.
        assert_eq!((got, matched), (vec!["keep.js".to_string()], 1));
    }

    #[test]
    fn zero_timeout_fires_on_a_tiny_tree() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        match walk_with(
            dir.path(),
            &GlobMatcher::new("*.txt").unwrap(),
            Duration::ZERO,
        ) {
            Err(ToolError::Timeout(d)) => assert_eq!(d, Duration::ZERO),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }
}
