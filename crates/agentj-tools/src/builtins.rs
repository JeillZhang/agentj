use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolCallContext};
use std::sync::Arc;

use crate::executor::{CommandSpec, ToolExecutor};

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
    const SKIP_DIRS: [&'static str; 9] = [
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
    ) -> Result<serde_json::Value, ToolError> {
        Self::run_with_rg("rg", pattern, include, cwd)
    }

    /// Search with the ripgrep binary when it is available, otherwise fall back to
    /// the in-process walker. The binary name is a parameter so the fallback can
    /// be exercised deterministically in tests.
    fn run_with_rg(
        rg_bin: &str,
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<serde_json::Value, ToolError> {
        match Self::run_rg(rg_bin, pattern, include, cwd) {
            Ok(stdout) => Ok(serde_json::Value::String(stdout)),
            // `rg` is not installed (fresh machine, minimal container, Windows):
            // fall back to the in-process walker instead of failing the tool.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::search_native(
                pattern,
                include,
                cwd.unwrap_or_else(|| std::path::Path::new(".")),
            )
            .map(serde_json::Value::String),
            Err(e) => Err(ToolError::ExecutionFailed {
                name: "grep".into(),
                reason: e.to_string(),
            }),
        }
    }

    /// Spawn error is preserved so the caller can tell "no ripgrep" from "rg ran
    /// and failed". `-n` keeps ripgrep's output in the same `path:line:match`
    /// shape the fallback emits.
    fn run_rg(
        rg_bin: &str,
        pattern: &str,
        include: &str,
        cwd: Option<&std::path::Path>,
    ) -> std::io::Result<String> {
        let mut cmd = std::process::Command::new(rg_bin);
        cmd.arg("--no-heading")
            .arg("--with-filename")
            .arg("--line-number")
            .arg(pattern)
            .arg("--glob")
            .arg(include);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Search without ripgrep: walk `root`, match `include` as a glob, run the
    /// regex per line, and print `path:line:match` like `rg --no-heading
    /// --with-filename --line-number`. Binary and oversized files are skipped,
    /// and the walk honours [`GrepTool::SKIP_DIRS`].
    fn search_native(
        pattern: &str,
        include: &str,
        root: &std::path::Path,
    ) -> Result<String, ToolError> {
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
        'walk: for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            if !Self::include_matches(include, entry.path(), root) {
                continue;
            }
            if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > Self::MAX_FILE_BYTES {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            // Non-UTF-8 (binary) files are skipped rather than lossily searched.
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let shown = entry.path().strip_prefix(root).unwrap_or(entry.path());
            for (idx, line) in text.lines().enumerate() {
                if !re.is_match(line) {
                    continue;
                }
                if hits == Self::MAX_HITS {
                    out.push_str(&format!("…[结果已截断，命中超过 {} 条]\n", Self::MAX_HITS));
                    break 'walk;
                }
                out.push_str(&format!("{}:{}:{}\n", shown.display(), idx + 1, line));
                hits += 1;
            }
        }
        Ok(out)
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
                "include": { "type": "string", "description": "File pattern (e.g. *.rs)" }
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        Self::run(pattern, include, None)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let (pattern, include) = Self::args(&input);
        Self::run(pattern, include, Some(&ctx.workspace_root))
    }
}

pub struct GlobTool;

impl GlobTool {
    fn run(pattern: &str, cwd: Option<&std::path::Path>) -> Result<serde_json::Value, ToolError> {
        let mut cmd = std::process::Command::new("find");
        cmd.arg(".").arg("-name").arg(pattern).arg("-type").arg("f");
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output().map_err(|e| ToolError::ExecutionFailed {
            name: "glob".into(),
            reason: e.to_string(),
        })?;
        Ok(serde_json::Value::String(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "pattern": { "type": "string" } },
            "required": ["pattern"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Self::run(input["pattern"].as_str().unwrap_or("*"), None)
    }
    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        Self::run(
            input["pattern"].as_str().unwrap_or("*"),
            Some(&ctx.workspace_root),
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
        )
        .unwrap();
        assert_eq!(
            out.as_str().unwrap(),
            "crates/demo/src/lib.rs:1:pub fn system_prompt() {}\ncrates/demo/src/lib.rs:2:// system_prompt again\n",
            "a missing rg binary must fall back to the in-process walker"
        );
    }

    /// `rg` matches the glob against the file name at any depth; the fallback
    /// must do the same (a bare `*.rs` cannot be matched against the full path).
    #[test]
    fn grep_fallback_matches_deep_files_and_honours_the_include_glob() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("a/b/c")).unwrap();
        std::fs::write(root.path().join("a/b/c/deep.rs"), "needle\n").unwrap();
        std::fs::write(root.path().join("a/b/c/deep.txt"), "needle\n").unwrap();

        let out = GrepTool::search_native("needle", "*.rs", root.path()).unwrap();
        assert_eq!(out, "a/b/c/deep.rs:1:needle\n");
    }

    #[test]
    fn grep_fallback_skips_binary_files_and_reports_bad_regex() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("bin.dat"), [0u8, 159, 146, 150, b'x']).unwrap();
        assert_eq!(GrepTool::search_native("x", "*", root.path()).unwrap(), "");

        let err = GrepTool::search_native("([", "*", root.path()).unwrap_err();
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
}
