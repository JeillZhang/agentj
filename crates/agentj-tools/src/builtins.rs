use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolCallContext};
use std::sync::Arc;

use crate::executor::{CommandSpec, RunFailure, ToolExecutor};

/// Lower bound for a model-supplied `timeout_ms`. Anything smaller is almost
/// always a units mistake (seconds read as milliseconds) and would kill the
/// command before it could do any work.
pub const MIN_TOOL_TIMEOUT_MS: i64 = 1_000;
/// Upper bound for a model-supplied `timeout_ms` (10 minutes), so a single tool
/// call cannot pin a turn forever.
pub const MAX_TOOL_TIMEOUT_MS: i64 = 600_000;
pub const BASH_DEFAULT_TIMEOUT_MS: i64 = 120_000;
pub const SEARCH_DEFAULT_TIMEOUT_MS: i64 = 60_000;

/// Clamp a caller-supplied `timeout_ms` into `[MIN, MAX]`; a missing or
/// non-integer value falls back to `default_ms` (clamping, not rejecting,
/// matches the `agent.max_tool_iterations` config style).
pub fn clamp_timeout_ms(raw: Option<i64>, default_ms: i64) -> std::time::Duration {
    let ms = raw
        .unwrap_or(default_ms)
        .clamp(MIN_TOOL_TIMEOUT_MS, MAX_TOOL_TIMEOUT_MS);
    std::time::Duration::from_millis(ms as u64)
}

pub struct ReadTool;
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read file contents from the filesystem"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File path to read" } },
            "required": ["path"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "read".into(),
                reason: "missing 'path'".into(),
            })?;
        let content = std::fs::read_to_string(path).map_err(|e| ToolError::ExecutionFailed {
            name: "read".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String(content))
    }
}

pub struct WriteTool;
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }
    fn description(&self) -> &str {
        "Write content to a file"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
            "required": ["path", "content"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: "missing 'path'".into(),
            })?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: "missing 'content'".into(),
            })?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::ExecutionFailed {
                name: "write".into(),
                reason: e.to_string(),
            })?;
        }
        std::fs::write(path, content).map_err(|e| ToolError::ExecutionFailed {
            name: "write".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String("ok".into()))
    }
}

pub struct BashTool;

impl BashTool {
    fn command_arg(input: &serde_json::Value) -> Result<&str, ToolError> {
        input["command"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                name: "bash".into(),
                reason: "missing 'command'".into(),
            })
    }

    fn run(
        command: &str,
        cwd: Option<&std::path::Path>,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, ToolError> {
        let spec = CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), command.to_string()],
            cwd: cwd.map(std::path::Path::to_path_buf),
        };
        let out = ToolExecutor::new(timeout, true).run_blocking(spec)?;
        Ok(serde_json::Value::String(out.stdout))
    }
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Execute a shell command"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Command to execute" },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (1000-600000; default 120000)"
                }
            },
            "required": ["command"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), BASH_DEFAULT_TIMEOUT_MS);
        Self::run(Self::command_arg(&input)?, None, timeout)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), BASH_DEFAULT_TIMEOUT_MS);
        Self::run(
            Self::command_arg(&input)?,
            Some(&ctx.workspace_root),
            timeout,
        )
    }
}

pub struct GrepTool;

impl GrepTool {
    /// Directories the in-process walker never descends into. The `rg` path
    /// honours `.gitignore`; the fallback has no ignore engine, so this list
    /// keeps it from crawling build output and dependency trees.
    pub(crate) const SKIP_DIRS: [&'static str; 9] = [
        ".git",
        "target",
        "node_modules",
        "dist",
        "build",
        ".venv",
        "venv",
        "__pycache__",
        ".next",
    ];
    /// Files larger than this are not searched by the fallback (source files are
    /// far below it; a multi-megabyte blob would only slow the walk down).
    const MAX_FILE_BYTES: u64 = 1 << 20;
    /// Upper bound on reported matches, so a runaway pattern cannot turn one tool
    /// result into a multi-megabyte transcript entry.
    const MAX_HITS: usize = 2_000;

    fn args(input: &serde_json::Value) -> (&str, &str) {
        (
            input["pattern"].as_str().unwrap_or(".*"),
            input["include"].as_str().unwrap_or("*"),
        )
    }

    fn run(
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, ToolError> {
        Self::run_with_rg("rg", pattern, include, cwd, timeout)
    }

    /// Search with the ripgrep binary when it is available, otherwise fall back to
    /// the in-process walker. The binary name is a parameter so the fallback can
    /// be exercised deterministically in tests.
    fn run_with_rg(
        rg_bin: &str,
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, ToolError> {
        match Self::run_rg(rg_bin, pattern, include, cwd, timeout) {
            Ok(stdout) => Ok(serde_json::Value::String(stdout)),
            // `rg` is not installed (fresh machine, minimal container, Windows):
            // fall back to the in-process walker instead of failing the tool.
            Err(RunFailure::ProgramMissing { .. }) => Self::search_native(
                pattern,
                include,
                cwd.unwrap_or_else(|| std::path::Path::new(".")),
                timeout,
            )
            .map(serde_json::Value::String),
            Err(other) => Err(other.into()),
        }
    }

    /// Run `rg` through the guarded executor so a hung search is killed together
    /// with its process group. A spawn `io::ErrorKind::NotFound` surfaces as
    /// [`RunFailure::ProgramMissing`], which is the signal that there is no
    /// ripgrep to fall back from — hence `RunFailure` rather than `ToolError`.
    /// The argument order is unchanged, which keeps the `path:line:match` output
    /// shape identical to the fallback.
    fn run_rg(
        rg_bin: &str,
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
        timeout: std::time::Duration,
    ) -> Result<String, RunFailure> {
        let spec = CommandSpec {
            program: rg_bin.to_string(),
            args: vec![
                "--no-heading".into(),
                "--with-filename".into(),
                "--line-number".into(),
                pattern.into(),
                "--glob".into(),
                include.into(),
            ],
            cwd: cwd.map(std::path::Path::to_path_buf),
        };
        ToolExecutor::new(timeout, true)
            .run_blocking(spec)
            .map(|out| out.stdout)
    }

    /// Search without ripgrep: walk `root`, match `include` as a glob, run the
    /// regex per line, and print `path:line:match` like `rg --no-heading
    /// --with-filename --line-number`. Binary and oversized files are skipped,
    /// and the walk honours [`GrepTool::SKIP_DIRS`]. `timeout` bounds the walk
    /// itself: the caller's already-clamped deadline is the only budget that
    /// exists once the `rg` subprocess is gone.
    fn search_native(
        pattern: &str,
        include: &str,
        root: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Result<String, ToolError> {
        let deadline = std::time::Instant::now() + timeout;
        let re = regex::Regex::new(pattern).map_err(|e| ToolError::ExecutionFailed {
            name: "grep".into(),
            reason: format!("invalid regex: {e}"),
        })?;
        let mut out = String::new();
        let mut hits = 0usize;
        let walker = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                !(e.file_type().is_dir()
                    && Self::SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
            });
        for entry in walker.filter_map(Result::ok) {
            if std::time::Instant::now() >= deadline {
                return Err(ToolError::Timeout(timeout));
            }
            if !Self::include_matches(include, entry.path(), root) {
                continue;
            }
            if Self::scan_entry(&entry, root, &re, deadline, timeout, &mut out, &mut hits)? {
                out.push_str(&format!("…[结果已截断，命中超过 {} 条]\n", Self::MAX_HITS));
                break;
            }
        }
        Ok(out)
    }

    /// Search one walked entry, returning `true` once [`GrepTool::MAX_HITS`] is
    /// reached. Non-matching entries (wrong type, oversized, binary) are skipped
    /// silently; only a timeout is an error.
    #[allow(clippy::too_many_arguments)]
    fn scan_entry(
        entry: &walkdir::DirEntry,
        root: &std::path::Path,
        re: &regex::Regex,
        deadline: std::time::Instant,
        timeout: std::time::Duration,
        out: &mut String,
        hits: &mut usize,
    ) -> Result<bool, ToolError> {
        if !entry.file_type().is_file() {
            return Ok(false);
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > Self::MAX_FILE_BYTES {
            return Ok(false);
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            return Ok(false);
        };
        // Non-UTF-8 (binary) files are skipped rather than lossily searched.
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(false);
        };
        let shown = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let rel = shown.display().to_string();
        Self::scan_text(&rel, out, hits, &text, re, deadline, timeout)?;
        Ok(*hits >= Self::MAX_HITS)
    }

    /// Scan one file's text, re-checking the deadline every 512 lines: the per-entry
    /// guard above cannot bound a single huge file, so the in-file check is the one
    /// that actually stops a pathological file. A timeout reports no matches at all.
    fn scan_text(
        rel: &str,
        out: &mut String,
        hits: &mut usize,
        text: &str,
        re: &regex::Regex,
        deadline: std::time::Instant,
        timeout: std::time::Duration,
    ) -> Result<(), ToolError> {
        for (idx, line) in text.lines().enumerate() {
            if idx & 511 == 0 && std::time::Instant::now() >= deadline {
                return Err(ToolError::Timeout(timeout));
            }
            if re.is_match(line) {
                *hits += 1;
                out.push_str(&format!("{rel}:{}:{line}\n", idx + 1));
                if *hits >= Self::MAX_HITS {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// `rg --glob` matches the pattern against the *file name* at any depth (so
    /// `*.rs` finds `a/b/c.rs`); a pattern containing a path separator is matched
    /// against the path relative to the search root.
    fn include_matches(include: &str, path: &std::path::Path, root: &std::path::Path) -> bool {
        let Ok(glob) = globset::Glob::new(include) else {
            return true;
        };
        let matcher = glob.compile_matcher();
        if let Some(name) = path.file_name()
            && matcher.is_match(name)
        {
            return true;
        }
        let rel = path.strip_prefix(root).unwrap_or(path);
        matcher.is_match(rel)
    }
}

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search file contents with regex"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "include": { "type": "string", "description": "File pattern (e.g. *.rs)" },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (1000-600000; default 60000)"
                }
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), SEARCH_DEFAULT_TIMEOUT_MS);
        Self::run(pattern, include, None, timeout)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), SEARCH_DEFAULT_TIMEOUT_MS);
        Self::run(pattern, include, Some(&ctx.workspace_root), timeout)
    }
}

pub struct GlobTool;

impl GlobTool {
    /// Render the in-process walk result into the frozen `./<rel path>` shape:
    /// hits sorted, one per line, capped at 2 000 with a `# ` truncation line,
    /// and a `no matches` line for an empty result.
    ///
    /// Tool errors are rendered as `Error: …` and the turn continues
    /// (crates/agentj-core/src/turn_runner.rs:838), so timeouts surface as failures.
    fn run_in_process(
        pattern: &str,
        root: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, ToolError> {
        match crate::glob::GlobMatcher::new(pattern)
            .and_then(|m| crate::glob::walk_with(root, &m, timeout))
        {
            Ok((hits, _)) if hits.is_empty() => Ok(serde_json::Value::String(format!(
                "no matches for {pattern}"
            ))),
            Ok((hits, matched)) => {
                let mut text = hits
                    .iter()
                    .map(|p| format!("./{p}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if matched > hits.len() {
                    text.push_str(&format!(
                        "\n# 已截断 {} 条命中（上限 {}），请缩小 pattern",
                        matched - hits.len(),
                        crate::glob::MAX_GLOB_ENTRIES
                    ));
                }
                Ok(serde_json::Value::String(text))
            }
            Err(e) => Err(e),
        }
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern in-process, skipping the same directories as grep"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (1000-600000; default 60000)"
                }
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), SEARCH_DEFAULT_TIMEOUT_MS);
        let cwd = std::env::current_dir().map_err(|e| ToolError::ExecutionFailed {
            name: "glob".into(),
            reason: e.to_string(),
        })?;
        Self::run_in_process(input["pattern"].as_str().unwrap_or("*"), &cwd, timeout)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let timeout = clamp_timeout_ms(input["timeout_ms"].as_i64(), SEARCH_DEFAULT_TIMEOUT_MS);
        Self::run_in_process(
            input["pattern"].as_str().unwrap_or("*"),
            &ctx.workspace_root,
            timeout,
        )
    }
}

pub struct TodoTool;

impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }
    fn description(&self) -> &str {
        "Submit the full TODO snapshot for the current task list (empty array clears it)"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "description": "Full TODO snapshot; [] clears the list",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "Task text (non-empty)" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["items"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let bad = |reason: String| ToolError::ExecutionFailed {
            name: "todo".into(),
            reason,
        };
        let items = input["items"]
            .as_array()
            .ok_or_else(|| bad("'items' must be a JSON array".into()))?;
        for (i, item) in items.iter().enumerate() {
            if item["content"].as_str().is_none_or(|c| c.trim().is_empty()) {
                return Err(bad(format!(
                    "items[{i}].content must be a non-empty string"
                )));
            }
            let status = item["status"].as_str().unwrap_or_default();
            if !matches!(
                status,
                "pending" | "in_progress" | "completed" | "cancelled"
            ) {
                return Err(bad(format!(
                    "items[{i}].status must be one of pending|in_progress|completed|cancelled"
                )));
            }
        }
        Ok(serde_json::json!({ "ok": true, "count": items.len() }))
    }
}

pub fn register_builtins(registry: &agentj_core::module::ToolRegistry) {
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(TodoTool));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_core::error::ToolError;
    use agentj_core::module::{Tool, ToolCallContext};
    use std::time::Duration;

    #[test]
    fn read_tool_reads_file() -> Result<(), ToolError> {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = ReadTool;
        let result = tool.execute(serde_json::json!({"path": file_path.to_str().unwrap()}))?;
        assert_eq!(result.as_str().unwrap(), "hello world");
        Ok(())
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_for_exit(pid: u32) {
        for _ in 0..50 {
            if !process_alive(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn clamp_timeout_ms_clamps_into_the_supported_range() {
        assert_eq!(
            clamp_timeout_ms(None, 120_000),
            std::time::Duration::from_secs(120)
        );
        assert_eq!(
            clamp_timeout_ms(Some(500), 120_000),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            clamp_timeout_ms(Some(-5_000), 120_000),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            clamp_timeout_ms(Some(2_000), 120_000),
            std::time::Duration::from_secs(2)
        );
        assert_eq!(
            clamp_timeout_ms(Some(10_000_000), 60_000),
            std::time::Duration::from_secs(600)
        );
    }

    #[test]
    fn clamp_timeout_ms_falls_back_for_non_integer_values() {
        // A model may send 1200.5 or "1200"; neither is an i64, so the default applies
        // instead of failing the whole tool call.
        let float = serde_json::json!({ "timeout_ms": 1200.5 });
        let text = serde_json::json!({ "timeout_ms": "1200" });
        assert_eq!(
            clamp_timeout_ms(float["timeout_ms"].as_i64(), 60_000),
            std::time::Duration::from_secs(60)
        );
        assert_eq!(
            clamp_timeout_ms(text["timeout_ms"].as_i64(), 60_000),
            std::time::Duration::from_secs(60)
        );
    }

    #[cfg(unix)]
    #[test]
    fn bash_tool_times_out_and_reclaims_its_process_group() {
        let dir = tempfile::TempDir::new().unwrap();
        let pidfile = dir.path().join("pid");
        // $$ is the `sh -c` process itself, which is the process-group leader
        // (spawn_guarded sets process_group(0)), so killpg(-pid) must take it down.
        let command = format!("echo $$ > {}; sleep 30", pidfile.display());
        let started = std::time::Instant::now();
        let err = BashTool
            .execute(serde_json::json!({ "command": command, "timeout_ms": 1500 }))
            .unwrap_err();
        // 1500 ms proves the per-call value was honoured: the 120 s default could not
        // have returned inside the 5 s budget asserted below.
        assert!(matches!(&err, ToolError::Timeout(_)), "unexpected: {err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let pid: u32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        wait_for_exit(pid);
        assert!(!process_alive(pid), "child pid {pid} must be reclaimed");
    }

    #[cfg(unix)]
    #[test]
    fn bash_tool_kills_grandchildren_on_timeout() {
        let dir = tempfile::TempDir::new().unwrap();
        let childfile = dir.path().join("child");
        let command = format!("sleep 30 & echo $! > {}; sleep 30", childfile.display());
        let err = BashTool
            .execute(serde_json::json!({ "command": command, "timeout_ms": 1500 }))
            .unwrap_err();
        assert!(matches!(&err, ToolError::Timeout(_)), "unexpected: {err}");
        let child: u32 = std::fs::read_to_string(&childfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        wait_for_exit(child);
        assert!(
            !process_alive(child),
            "background child {child} must be reclaimed with the group"
        );
    }

    #[test]
    fn bash_timeout_error_renders_the_model_visible_message() {
        let err = ToolError::Timeout(std::time::Duration::from_secs(120));
        assert_eq!(err.to_string(), "tool timeout after 120s");
    }

    #[cfg(unix)]
    #[test]
    fn bash_tool_runs_from_the_blocking_pool_inside_a_runtime() {
        // The production path is spawn_blocking inside a runtime (agentj-core
        // module.rs:451). Runtime::block_on is only safe on such a thread because
        // blocking-pool workers never set EnterRuntime::Entered (design D1).
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let out = runtime.block_on(async {
            tokio::task::spawn_blocking(|| {
                BashTool
                    .execute(serde_json::json!({ "command": "echo hi" }))
                    .unwrap()
            })
            .await
            .unwrap()
        });
        assert_eq!(out.as_str().unwrap().trim(), "hi");
    }

    #[cfg(unix)]
    #[test]
    fn bash_tool_drains_large_stdout_without_deadlock() {
        // 1 MiB is far past the 64 KiB pipe buffer: if the implementation waited for
        // exit before reading, this would hang instead of returning the output.
        let out = BashTool
            .execute(serde_json::json!({
                "command": "head -c 1048576 /dev/zero | tr '\\0' 'x'",
                "timeout_ms": 60000
            }))
            .unwrap();
        assert_eq!(out.as_str().unwrap().len(), 1_048_576);
    }

    #[test]
    fn bash_schema_declares_an_optional_timeout_ms() {
        // The model only learns about timeout_ms from the schema, so the declaration
        // itself is part of the contract (nothing else in the repo asserts tool schemas).
        let schema = BashTool.input_schema();
        assert_eq!(schema["properties"]["timeout_ms"]["type"], "integer");
        assert_eq!(schema["required"], serde_json::json!(["command"]));
    }

    #[test]
    fn todo_tool_accepts_valid_items_and_counts() {
        let out = TodoTool
            .execute(serde_json::json!({"items": [
                {"content": "a", "status": "pending"},
                {"content": "b", "status": "in_progress"},
                {"content": "c", "status": "completed"},
                {"content": "d", "status": "cancelled"}
            ]}))
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "count": 4}));
    }

    #[test]
    fn todo_tool_accepts_empty_items_as_snapshot_clear() {
        let out = TodoTool.execute(serde_json::json!({"items": []})).unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "count": 0}));
    }

    #[test]
    fn todo_tool_rejects_empty_content() {
        let err = TodoTool
            .execute(serde_json::json!({"items": [{"content": "", "status": "pending"}]}))
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("content")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn todo_tool_rejects_missing_items() {
        let err = TodoTool.execute(serde_json::json!({})).unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("items")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn todo_tool_rejects_bad_status() {
        let err = TodoTool
            .execute(serde_json::json!({"items": [{"content": "x", "status": "doing"}]}))
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "todo" && reason.contains("status")),
            "unexpected error: {err}"
        );
    }

    /// The model's failing call was `grep {"include":"*.rs","pattern":"system_prompt"}`
    /// on a machine without ripgrep, where the tool returned
    /// `execution failed: No such file or directory (os error 2)`. The fallback
    /// walker must find the same matches with no `rg` binary at all.
    #[test]
    fn grep_falls_back_to_an_in_process_search_without_ripgrep() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("crates/demo/src")).unwrap();
        std::fs::write(
            root.path().join("crates/demo/src/lib.rs"),
            "pub fn system_prompt() {}\n// system_prompt again\n",
        )
        .unwrap();
        std::fs::write(root.path().join("crates/demo/notes.md"), "system_prompt\n").unwrap();
        std::fs::create_dir_all(root.path().join("target/debug")).unwrap();
        std::fs::write(root.path().join("target/debug/big.rs"), "system_prompt\n").unwrap();

        let out = GrepTool::run_with_rg(
            "agentj-test-missing-rg-binary",
            "system_prompt",
            "*.rs",
            Some(root.path()),
            std::time::Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(
            out.as_str().unwrap(),
            "crates/demo/src/lib.rs:1:pub fn system_prompt() {}\ncrates/demo/src/lib.rs:2:// system_prompt again\n",
            "a missing rg binary must fall back to the in-process walker"
        );
    }

    #[cfg(unix)]
    fn slow_executable(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn grep_tool_times_out_and_reclaims_its_process_group() {
        let dir = tempfile::TempDir::new().unwrap();
        let fake_rg = slow_executable(dir.path(), "slow-rg");
        let started = std::time::Instant::now();
        let err = GrepTool::run_with_rg(
            fake_rg.to_str().unwrap(),
            "needle",
            "*",
            Some(dir.path()),
            std::time::Duration::from_millis(1_000),
        )
        .unwrap_err();
        // Err(Timeout) — NOT Ok(""): an empty result would read to the model as
        // "no matches", turning a timeout into a false conclusion.
        assert!(matches!(&err, ToolError::Timeout(_)), "unexpected: {err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn grep_schema_declares_an_optional_timeout_ms() {
        let schema = GrepTool.input_schema();
        assert_eq!(schema["properties"]["timeout_ms"]["type"], "integer");
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v != "timeout_ms"),
            "timeout_ms must stay optional"
        );
    }

    #[test]
    fn glob_schema_declares_an_optional_timeout_ms() {
        let schema = GlobTool.input_schema();
        assert_eq!(schema["properties"]["timeout_ms"]["type"], "integer");
        assert_eq!(schema["required"], serde_json::json!(["pattern"]));
    }

    /// `rg` matches the glob against the file name at any depth; the fallback
    /// must do the same (a bare `*.rs` cannot be matched against the full path).
    #[test]
    fn grep_fallback_matches_deep_files_and_honours_the_include_glob() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("a/b/c")).unwrap();
        std::fs::write(root.path().join("a/b/c/deep.rs"), "needle\n").unwrap();
        std::fs::write(root.path().join("a/b/c/deep.txt"), "needle\n").unwrap();

        let out = GrepTool::search_native(
            "needle",
            "*.rs",
            root.path(),
            Duration::from_millis(SEARCH_DEFAULT_TIMEOUT_MS as u64),
        )
        .unwrap();
        assert_eq!(out, "a/b/c/deep.rs:1:needle\n");
    }

    #[test]
    fn grep_fallback_skips_binary_files_and_reports_bad_regex() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("bin.dat"), [0u8, 159, 146, 150, b'x']).unwrap();
        assert_eq!(
            GrepTool::search_native(
                "x",
                "*",
                root.path(),
                Duration::from_millis(SEARCH_DEFAULT_TIMEOUT_MS as u64),
            )
            .unwrap(),
            ""
        );

        let err = GrepTool::search_native(
            "([",
            "*",
            root.path(),
            Duration::from_millis(SEARCH_DEFAULT_TIMEOUT_MS as u64),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "grep" && reason.contains("invalid regex")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn register_builtins_registers_six_tools_including_todo() {
        let registry = agentj_core::module::ToolRegistry::new();
        register_builtins(&registry);
        assert_eq!(registry.definitions().len(), 6);
        assert!(registry.get("todo").is_some());
    }

    fn ctx_for(root: &std::path::Path) -> ToolCallContext {
        ToolCallContext {
            session_id: "s1".into(),
            workspace_root: root.to_path_buf(),
        }
    }

    #[test]
    fn glob_tool_scopes_to_context_workspace_root() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(outside.path().join("outside.rs"), "").unwrap();

        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "*.rs"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        let stdout = out.as_str().unwrap();
        assert!(
            stdout.contains("./src/main.rs"),
            "missing root file: {stdout}"
        );
        assert!(
            !stdout.contains("outside.rs"),
            "leaked outside root: {stdout}"
        );
    }

    #[test]
    fn glob_tool_survives_registry_rebase_of_basename_pattern() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();

        let args = agentj_core::tool_paths::rebase_tool_args(
            "glob",
            serde_json::json!({"pattern": "*.rs"}),
            root.path(),
        );
        assert_eq!(args, serde_json::json!({"pattern": "*.rs"}));
        let out = GlobTool
            .execute_with_context(args, &ctx_for(root.path()))
            .unwrap();
        assert!(out.as_str().unwrap().contains("./src/main.rs"));
    }

    #[test]
    fn glob_basename_pattern_still_matches_at_any_depth() {
        let root = tempfile::TempDir::new().unwrap();
        for p in ["a/x.rs", "b/c/x.rs", "a/y.txt"] {
            let path = root.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "x.rs"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        assert_eq!(out.as_str().unwrap(), "./a/x.rs\n./b/c/x.rs");
    }

    #[test]
    fn glob_reports_no_matches_and_invalid_patterns() {
        let root = tempfile::TempDir::new().unwrap();
        let empty = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "*.nope"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        assert_eq!(empty.as_str().unwrap(), "no matches for *.nope");

        let bad = GlobTool
            .execute_with_context(serde_json::json!({"pattern": "a[b"}), &ctx_for(root.path()));
        assert!(
            matches!(bad, Err(ToolError::ExecutionFailed { .. })),
            "got {bad:?}"
        );
    }

    #[test]
    fn glob_output_is_sorted_and_prefixed_with_dot_slash() {
        let root = tempfile::TempDir::new().unwrap();
        // Created in a deliberately scrambled order: the walk is not guaranteed to visit
        // these in sorted order, so the exact-output assertion only holds because the
        // tool sorts before rendering.
        for p in ["z.txt", "m/b.txt", "a.txt", "m/a.txt", "a/b.txt"] {
            let path = root.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "*.txt"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        let text = out.as_str().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines.iter().all(|l| l.starts_with("./")),
            "every line must be ./ prefixed: {text}"
        );
        // Pin the exact sorted order rather than only "lines == lines.sort()": dropping
        // the sort would stay green only if the walk happened to visit in sorted order.
        assert_eq!(text, "./a.txt\n./a/b.txt\n./m/a.txt\n./m/b.txt\n./z.txt");
    }

    #[test]
    fn glob_matches_path_shaped_patterns() {
        let root = tempfile::TempDir::new().unwrap();
        for p in ["dir/a.rs", "dir/sub/b.rs", "other/c.rs"] {
            let path = root.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "dir/*.rs"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        assert_eq!(out.as_str().unwrap(), "./dir/a.rs");
    }

    #[test]
    fn glob_caps_entries_and_announces_truncation() {
        let root = tempfile::TempDir::new().unwrap();
        for i in 0..2_001 {
            std::fs::write(root.path().join(format!("f{i:04}.txt")), "").unwrap();
        }
        let out = GlobTool
            .execute_with_context(
                serde_json::json!({"pattern": "*.txt"}),
                &ctx_for(root.path()),
            )
            .unwrap();
        let text = out.as_str().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("./")).count(), 2_000);
        let last = lines.last().copied().unwrap();
        assert_eq!(last, "# 已截断 1 条命中（上限 2000），请缩小 pattern");
        assert!(!last.starts_with("./"));
    }

    #[test]
    fn glob_timeout_is_not_downgraded_to_a_string() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.rs"), "").unwrap();
        let out = GlobTool::run_in_process("*.rs", root.path(), Duration::ZERO);
        match out {
            Err(ToolError::Timeout(d)) => assert_eq!(d, Duration::ZERO),
            other => panic!("expected ToolError::Timeout, got {other:?}"),
        }
    }

    #[test]
    fn glob_timeout_message_is_the_shared_tool_error() {
        let rendered = format!("Error: {}", ToolError::Timeout(Duration::from_secs(60)));
        assert_eq!(rendered, "Error: tool timeout after 60s");
        assert!(!rendered.contains("glob timed out"));
    }

    #[test]
    fn bash_tool_uses_context_workspace_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolCallContext {
            session_id: "s1".into(),
            workspace_root: dir.path().to_path_buf(),
        };
        let out = BashTool
            .execute_with_context(serde_json::json!({"command": "pwd"}), &ctx)
            .unwrap();
        let stdout = out.as_str().unwrap().trim();
        assert_eq!(
            std::fs::canonicalize(stdout).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn bash_tool_execute_without_context_keeps_process_cwd() {
        let out = BashTool
            .execute(serde_json::json!({"command": "pwd"}))
            .unwrap();
        let stdout = out.as_str().unwrap().trim();
        let expected = std::env::current_dir().unwrap();
        assert_eq!(
            std::fs::canonicalize(stdout).unwrap(),
            std::fs::canonicalize(expected).unwrap()
        );
    }

    #[test]
    fn native_search_times_out_when_the_budget_is_zero() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        let err = GrepTool::search_native("needle", "*", root.path(), Duration::ZERO).unwrap_err();
        assert!(
            matches!(err, ToolError::Timeout(_)),
            "zero budget must time out structurally: {err}"
        );
    }

    #[test]
    fn native_search_finds_hits_within_the_budget() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        let out =
            GrepTool::search_native("needle", "*", root.path(), Duration::from_secs(5)).unwrap();
        assert_eq!(out, "a.txt:1:needle\n");
    }

    #[test]
    fn grep_fallback_honours_the_clamped_timeout() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "needle\n").unwrap();
        // 钳制下限是 1s：timeout_ms = 1 与缺省值都要落到同一个 Duration。
        assert_eq!(
            clamp_timeout_ms(Some(1), SEARCH_DEFAULT_TIMEOUT_MS),
            Duration::from_millis(MIN_TOOL_TIMEOUT_MS as u64)
        );
        let err = GrepTool::run_with_rg(
            "agentj-test-missing-rg-binary",
            "needle",
            "*",
            Some(root.path()),
            Duration::ZERO,
        )
        .unwrap_err();
        assert!(
            matches!(err, ToolError::Timeout(_)),
            "the fallback must honour the deadline: {err}"
        );
    }

    #[test]
    fn scan_text_checks_the_deadline_inside_a_file() {
        let re = regex::Regex::new("needle").unwrap();
        let body: String = (0..1_000)
            .map(|i| format!("line {i} without a match\n"))
            .collect();
        let (mut out, mut hits) = (String::new(), 0usize);
        // 已经过去的 deadline：idx 0 的复查立即命中，与挂钟速度无关。
        let deadline = std::time::Instant::now();
        let err = GrepTool::scan_text(
            "big.txt",
            &mut out,
            &mut hits,
            &body,
            &re,
            deadline,
            Duration::ZERO,
        )
        .unwrap_err();
        assert!(
            matches!(err, ToolError::Timeout(_)),
            "the in-file guard must fire: {err}"
        );
        assert_eq!(hits, 0, "a timed-out scan reports nothing");
        assert!(out.is_empty(), "no partial output escapes a timeout");
    }

    #[test]
    fn native_search_skips_files_over_the_size_cap() {
        let root = tempfile::TempDir::new().unwrap();
        let body = "needle\n".repeat(200_000);
        assert!(
            body.len() as u64 > GrepTool::MAX_FILE_BYTES,
            "the fixture must exceed the 1 MiB cap or the cap is never exercised"
        );
        std::fs::write(root.path().join("big.txt"), &body).unwrap();
        let out =
            GrepTool::search_native("needle", "*", root.path(), Duration::from_secs(30)).unwrap();
        assert_eq!(out, "", "files above the size cap are skipped");
    }
}
