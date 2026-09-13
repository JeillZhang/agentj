use crate::artifacts::{TurnArtifact, todo_snapshot_from_args, write_change};
use crate::error::ModuleError;
use crate::git_sample;
use crate::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister, ToolExecutionOutcome, ToolSource,
};
use crate::permission::PermissionContext;
use crate::tool_paths::rebase_tool_args;
use crate::types::{ModuleId, ToolCall};
use std::path::{Path, PathBuf};

/// Pre-read state of a `write` target, captured before execution.
enum PreRead {
    /// File existed and was readable (overwrite).
    Content(String),
    /// File did not exist yet (new file).
    New,
    /// Pre-read failed; the artifact is skipped.
    Skip,
}

/// Rebased `write` target for `tool_call`, or `None` for other tools.
///
/// Mirrors the execution-time rebase (T7/T12): relative args resolve under the
/// session root, absolute args pass through.
fn write_target(tool_call: &ToolCall, root: &Path) -> Option<String> {
    if tool_call.name != "write" {
        return None;
    }
    let rebased = rebase_tool_args(&tool_call.name, tool_call.arguments.clone(), root);
    rebased
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Read the write target before execution: missing file is a new file, any
/// other read error degrades to skipping the artifact.
async fn pre_read(target: &str) -> PreRead {
    match tokio::fs::read_to_string(target).await {
        Ok(content) => PreRead::Content(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PreRead::New,
        Err(err) => {
            tracing::debug!(path = target, error = %err, "write artifact skipped: pre-read failed");
            PreRead::Skip
        }
    }
}

/// Read the write target after execution; any error skips the artifact.
async fn post_read(target: &str) -> Option<String> {
    match tokio::fs::read_to_string(target).await {
        Ok(content) => Some(content),
        Err(err) => {
            tracing::debug!(path = target, error = %err, "write artifact skipped: post-read failed");
            None
        }
    }
}

/// Path relative to the session root, falling back to the target as given.
fn relative_to_root(target: &str, root: &Path) -> String {
    Path::new(target)
        .strip_prefix(root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| target.to_string())
}

/// `file_change` artifact for a finished write; empty on any degradation.
async fn write_artifacts(
    target: Option<&str>,
    root: &Path,
    tool_call: &ToolCall,
    pre: PreRead,
) -> Vec<TurnArtifact> {
    let Some(target) = target else {
        return vec![];
    };
    let before = match pre {
        PreRead::Skip => return vec![],
        PreRead::New => None,
        PreRead::Content(content) => Some(content),
    };
    let Some(after) = post_read(target).await else {
        return vec![];
    };
    let path = relative_to_root(target, root);
    vec![TurnArtifact::FileChange(write_change(
        &path,
        before.as_deref(),
        &after,
        Some(&tool_call.id),
    ))]
}

/// `file_change` artifacts for a finished bash call; empty when the after-sample
/// degrades. The caller only passes a `before` snapshot that sampled cleanly.
async fn bash_artifacts(
    before: &git_sample::GitSnapshot,
    root: &Path,
    call_id: &str,
) -> Vec<TurnArtifact> {
    let Some(after) = git_sample::sample(root).await else {
        return vec![];
    };
    git_sample::changes_between(Some(before), &after, root, call_id)
        .await
        .into_iter()
        .map(TurnArtifact::FileChange)
        .collect()
}

/// Run one tool call, attaching `file_change` artifacts on success.
async fn execute_tool(
    ctx: &KernelContext,
    tool_call: &ToolCall,
    session_id: &str,
    workspace_root: PathBuf,
) -> Result<ToolExecutionOutcome, crate::error::ToolError> {
    let source = ctx
        .tools
        .source_of(&tool_call.name)
        .unwrap_or(ToolSource::Builtin);
    let perm_ctx = PermissionContext {
        session_id: session_id.to_string(),
        tool: tool_call.name.clone(),
        source,
        args: tool_call.arguments.clone(),
        workspace_root: workspace_root.clone(),
    };
    let target = write_target(tool_call, &workspace_root);
    let pre = match target.as_deref() {
        Some(target) => pre_read(target).await,
        None => PreRead::Skip,
    };
    // Git sampling is a bash-only side channel: a non-repo or failed `rev-parse`
    // yields `None`, which also gates the after-sample below.
    let git_before = if tool_call.name == "bash" {
        git_sample::sample(&workspace_root).await
    } else {
        None
    };
    let result = ctx
        .tools
        .execute_async(&tool_call.name, tool_call.arguments.clone(), &perm_ctx)
        .await?;
    let mut artifacts = write_artifacts(target.as_deref(), &workspace_root, tool_call, pre).await;
    if let Some(before) = git_before.as_ref() {
        artifacts.extend(bash_artifacts(before, &workspace_root, &tool_call.id).await);
    }
    if tool_call.name == "todo" {
        match todo_snapshot_from_args(&tool_call.arguments) {
            Ok(snapshot) => artifacts.push(TurnArtifact::Todo(snapshot)),
            Err(err) => {
                tracing::debug!(call = %tool_call.id, error = %err, "todo artifact skipped: unparsable args");
            }
        }
    }
    Ok(ToolExecutionOutcome { result, artifacts })
}

/// Executes tool calls routed by `KernelMessage::ToolCallRequest`.
///
/// The permission gate stays at `ToolRegistry::execute_async` (design #17)
/// — this module only moves the execution boundary from `AgentLoop`
/// into a kernel-routed module.
pub struct ToolExecutorModule {
    ctx: KernelContext,
    workspace_root: PathBuf,
}

impl ToolExecutorModule {
    pub fn new(ctx: KernelContext) -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            ctx,
            workspace_root,
        }
    }
}

impl Module for ToolExecutorModule {
    fn id(&self) -> ModuleId {
        "tool_executor".into()
    }

    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "tool_executor".into(),
            name: "ToolExecutor".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Bus-mediated tool execution".into(),
            author: "AgentJ".into(),
        }
    }

    fn init(
        &mut self,
        _ctx: &KernelContext,
        _register: &mut ModuleRegister,
    ) -> Result<(), ModuleError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ModuleError> {
        Ok(())
    }
}

impl KernelModule for ToolExecutorModule {
    fn handle(&mut self, msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        if let KernelMessage::ToolCallRequest {
            session_id,
            tool_call,
            reply,
            ..
        } = msg
        {
            let ctx = self.ctx.clone();
            let reply = reply.clone();
            let tool_call = tool_call.clone();
            let session_id = session_id.clone();
            // Session-level path base (design #3 ③): permission context carries the
            // resolved root; T7's execute_async rebases args with it. Missing session
            // or unset root falls back to the process CWD captured at construction.
            let workspace_root = ctx
                .session
                .project_root(&session_id)
                .unwrap_or_else(|| self.workspace_root.clone());
            tokio::spawn(async move {
                let outcome = execute_tool(&ctx, &tool_call, &session_id, workspace_root).await;
                if let Some(tx) = reply.lock().unwrap().take() {
                    let _ = tx.send(outcome);
                }
            });
        }
        Ok(KernelResponse::Handled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ToolError;
    use crate::kernel::Kernel;
    use crate::module::{Module, ProviderRegistry, Tool, ToolExecutionOutcome, ToolRegistry};
    use crate::permission::{
        PermissionChecker, PermissionContext as PcContext, PermissionDecision, PermissionRequestId,
    };
    use crate::types::ToolCall;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"done": true}))
        }
    }

    struct FsWriteTool;
    impl Tool for FsWriteTool {
        fn name(&self) -> &str {
            "write"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            let path = input["path"]
                .as_str()
                .ok_or_else(|| ToolError::ExecutionFailed {
                    name: "write".into(),
                    reason: "missing 'path'".into(),
                })?;
            let content = input["content"].as_str().unwrap_or_default();
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
            Ok(serde_json::json!({"ok": true}))
        }
    }

    struct PhantomWriteTool;
    impl Tool for PhantomWriteTool {
        fn name(&self) -> &str {
            "write"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
    }

    struct AllowAll;
    #[async_trait]
    impl PermissionChecker for AllowAll {
        fn check(&self, _ctx: &PcContext) -> PermissionDecision {
            PermissionDecision::Allow
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct DenyAll;
    #[async_trait]
    impl PermissionChecker for DenyAll {
        fn check(&self, _ctx: &PcContext) -> PermissionDecision {
            PermissionDecision::Deny {
                reason: "denied".into(),
            }
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Deny {
                reason: "denied".into(),
            }
        }
    }

    struct RecordingChecker {
        roots: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    }
    #[async_trait]
    impl PermissionChecker for RecordingChecker {
        fn check(&self, ctx: &PcContext) -> PermissionDecision {
            self.roots.lock().unwrap().push(ctx.workspace_root.clone());
            PermissionDecision::Allow
        }
        async fn await_approval(&self, _rid: &PermissionRequestId) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct MockSession {
        root: Option<PathBuf>,
    }
    impl crate::module::SessionStore for MockSession {
        fn create(&self) -> Result<crate::types::SessionId, crate::error::ModuleError> {
            Ok("s1".into())
        }
        fn project_root(&self, _s: &crate::types::SessionId) -> Option<PathBuf> {
            self.root.clone()
        }
        fn append(
            &self,
            _s: &crate::types::SessionId,
            _m: crate::types::ChatMessage,
        ) -> Result<(), crate::error::ModuleError> {
            Ok(())
        }
        fn get(
            &self,
            _s: &crate::types::SessionId,
        ) -> Result<Vec<crate::types::ChatMessage>, crate::error::ModuleError> {
            Ok(vec![])
        }
        fn estimate_tokens(
            &self,
            _s: &crate::types::SessionId,
        ) -> Result<usize, crate::error::ModuleError> {
            Ok(0)
        }
    }

    struct MockConfig;
    impl crate::module::ConfigProvider for MockConfig {
        fn get_str(&self, _k: &str) -> Option<String> {
            None
        }
        fn get_int(&self, _k: &str) -> Option<i64> {
            None
        }
        fn providers(&self) -> Vec<(String, String)> {
            vec![]
        }
    }

    fn executor_ctx(checker: Arc<dyn PermissionChecker>) -> KernelContext {
        executor_ctx_with_store(checker, Arc::new(MockSession { root: None }))
    }

    fn executor_ctx_with_store(
        checker: Arc<dyn PermissionChecker>,
        session: Arc<dyn crate::module::SessionStore>,
    ) -> KernelContext {
        let tools = ToolRegistry::new();
        tools.register(Arc::new(DummyTool));
        tools.set_permission_checker(checker);
        KernelContext {
            session,
            tools: Arc::new(tools),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig),
        }
    }

    #[tokio::test]
    async fn tool_call_request_is_executed_and_replied() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn tool_executor_replies_with_outcome() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        let outcome = reply.expect("successful tool call must reply Ok");
        assert_eq!(outcome.result, serde_json::json!({"done": true}));
        assert!(
            outcome.artifacts.is_empty(),
            "artifacts stay empty until T13/T14/T16"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn denied_tool_call_replies_permission_denied() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(DenyAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert!(matches!(reply, Err(ToolError::PermissionDenied { .. })));
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn executor_uses_session_project_root() {
        let mut kernel = Kernel::new();
        let roots = Arc::new(std::sync::Mutex::new(Vec::new()));
        let checker = Arc::new(RecordingChecker {
            roots: Arc::clone(&roots),
        });
        let session = Arc::new(MockSession {
            root: Some(PathBuf::from("/tmp/proj")),
        });
        let ctx = executor_ctx_with_store(checker, session);
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({"path": "src/main.rs"}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        let seen = roots.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![PathBuf::from("/tmp/proj")],
            "permission context must use the session project_root"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn executor_without_project_root_uses_process_cwd() {
        let mut kernel = Kernel::new();
        let roots = Arc::new(std::sync::Mutex::new(Vec::new()));
        let checker = Arc::new(RecordingChecker {
            roots: Arc::clone(&roots),
        });
        let session = Arc::new(MockSession { root: None });
        let ctx = executor_ctx_with_store(checker, session);
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        assert_eq!(reply.unwrap().result["done"], true);
        let cwd = std::env::current_dir().expect("process cwd is available");
        assert_eq!(
            roots.lock().unwrap().clone(),
            vec![cwd],
            "unset project_root must preserve the process CWD fallback"
        );
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn taken_reply_is_dropped_without_panic() {
        let mut kernel = Kernel::new();
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        // Take the sender immediately so executor finds None.
        let reply_slot = Arc::new(std::sync::Mutex::new(Some(tx)));
        // Simulate already-taken by taking before send.
        reply_slot.lock().unwrap().take();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "dummy".into(),
                    arguments: serde_json::json!({}),
                },
                target: "tool_executor".into(),
                reply: reply_slot,
            })
            .unwrap();
        // Give executor time to run; must not panic.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn module_id_is_tool_executor() {
        let ctx = executor_ctx(Arc::new(AllowAll));
        let m = ToolExecutorModule::new(ctx);
        assert_eq!(m.id(), "tool_executor");
    }

    #[test]
    fn non_tool_message_is_ignored() {
        let ctx = executor_ctx(Arc::new(AllowAll));
        let mut m = ToolExecutorModule::new(ctx);
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let msg = KernelMessage::UserRequest {
            session_id: "s1".into(),
            content: "hi".into(),
            attachments: vec![],
            reply: tx,
        };
        let res = m.handle(&msg).unwrap();
        assert!(matches!(res, KernelResponse::Handled));
    }

    /// Unique temp directory cleaned up by the caller (best effort).
    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("agentj-t13-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root is creatable");
        root
    }

    fn write_executor_ctx(
        checker: Arc<dyn PermissionChecker>,
        session: Arc<dyn crate::module::SessionStore>,
        tool: Arc<dyn Tool>,
    ) -> KernelContext {
        let tools = ToolRegistry::new();
        tools.register(tool);
        tools.set_permission_checker(checker);
        KernelContext {
            session,
            tools: Arc::new(tools),
            providers: Arc::new(ProviderRegistry::new()),
            config: Arc::new(MockConfig),
        }
    }

    /// Run one tool call through a fresh kernel and return the executor reply.
    async fn run_tool_call(
        ctx: &KernelContext,
        tool_call: ToolCall,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        let mut kernel = Kernel::new();
        let mut register =
            crate::module::ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                ctx,
                &mut register,
            )
            .unwrap();
        let handle = kernel.start();
        let (tx, rx) = tokio::sync::oneshot::channel();
        kernel
            .sender()
            .send(KernelMessage::ToolCallRequest {
                session_id: "s1".into(),
                tool_call,
                target: "tool_executor".into(),
                reply: Arc::new(std::sync::Mutex::new(Some(tx))),
            })
            .unwrap();
        let reply: Result<ToolExecutionOutcome, ToolError> =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx)
                .await
                .expect("reply must arrive")
                .expect("sender dropped");
        kernel.sender().send(KernelMessage::Shutdown).unwrap();
        handle.await.unwrap().unwrap();
        reply
    }

    struct BashMock {
        fail: bool,
    }

    impl Tool for BashMock {
        fn name(&self) -> &str {
            "bash"
        }

        fn description(&self) -> &str {
            ""
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }

        fn execute_with_context(
            &self,
            input: serde_json::Value,
            ctx: &crate::module::ToolCallContext,
        ) -> Result<serde_json::Value, ToolError> {
            let command = input["command"].as_str().unwrap_or_default();
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.workspace_root)
                .output()
                .map_err(|err| ToolError::ExecutionFailed {
                    name: "bash".into(),
                    reason: err.to_string(),
                })?;
            if self.fail {
                return Err(ToolError::ExecutionFailed {
                    name: "bash".into(),
                    reason: format!("exit code {}", output.status.code().unwrap_or(-1)),
                });
            }
            Ok(serde_json::json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }))
        }
    }

    fn bash_call(id: &str, command: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": command}),
        }
    }

    fn file_changes(outcome: &ToolExecutionOutcome) -> Vec<crate::artifacts::FileChangeArtifact> {
        outcome
            .artifacts
            .iter()
            .filter_map(|artifact| match artifact {
                TurnArtifact::FileChange(change) => Some(change.clone()),
                TurnArtifact::Todo(_) => None,
            })
            .collect()
    }

    fn init_git_repo(root: &Path) {
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
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
        };
        git(&["init"]);
        std::fs::write(root.join("A.txt"), "1\n2\n3\n").expect("fixture file");
        git(&["add", "A.txt"]);
        git(&[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-m",
            "init",
        ]);
    }

    #[tokio::test]
    async fn bash_tool_emits_git_file_change_artifacts() {
        use crate::artifacts::FileChangeOp;
        let root = temp_root("bash-git");
        init_git_repo(&root);
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(BashMock { fail: false }),
        );

        let modified = run_tool_call(&ctx, bash_call("c1", "echo x >> A.txt"))
            .await
            .expect("bash modify succeeds");
        let changes = file_changes(&modified);
        assert_eq!(changes.len(), 1, "one modify artifact: {changes:?}");
        assert_eq!(changes[0].path, "A.txt");
        assert_eq!(changes[0].op, FileChangeOp::Modify);
        assert_eq!(changes[0].additions, 1);
        assert_eq!(changes[0].deletions, 0);
        assert_eq!(changes[0].tool_call_id.as_deref(), Some("c1"));
        assert!(
            changes[0]
                .diff
                .as_deref()
                .unwrap_or_default()
                .contains("+x"),
            "diff carries the git delta: {:?}",
            changes[0].diff
        );

        let created = run_tool_call(&ctx, bash_call("c2", "printf 'n1\\nn2\\n' > B.txt"))
            .await
            .expect("bash create succeeds");
        let changes = file_changes(&created);
        assert_eq!(changes.len(), 1, "one add artifact: {changes:?}");
        assert_eq!(changes[0].path, "B.txt");
        assert_eq!(changes[0].op, FileChangeOp::Add);
        assert_eq!(changes[0].additions, 2);
        assert!(changes[0].diff.is_none(), "untracked add keeps diff=None");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_tool_error_emits_no_artifacts() {
        let root = temp_root("bash-fail");
        init_git_repo(&root);
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(BashMock { fail: true }),
        );
        let reply = run_tool_call(&ctx, bash_call("c1", "echo x >> A.txt")).await;
        assert!(
            matches!(reply, Err(ToolError::ExecutionFailed { .. })),
            "failing bash keeps its error and carries no artifacts: {reply:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn bash_tool_outside_git_emits_no_artifacts() {
        let root = temp_root("bash-non-git");
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(BashMock { fail: false }),
        );
        let outcome = run_tool_call(&ctx, bash_call("c1", "printf 'hi\\n' > fresh.txt"))
            .await
            .expect("bash succeeds outside git");
        assert!(outcome.result.get("stdout").is_some(), "result unaffected");
        assert!(outcome.artifacts.is_empty(), "non-git emits no artifacts");
        assert_eq!(
            std::fs::read_to_string(root.join("fresh.txt")).unwrap(),
            "hi\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_produces_add_then_modify_artifacts() {
        use crate::artifacts::{FileChangeOp, TurnArtifact};
        let root = temp_root("add-modify");
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(FsWriteTool),
        );
        let first = run_tool_call(
            &ctx,
            ToolCall {
                id: "c1".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "notes.txt", "content": "a\nb\n"}),
            },
        )
        .await
        .expect("first write succeeds");
        assert_eq!(first.artifacts.len(), 1, "one artifact for the new file");
        match &first.artifacts[0] {
            TurnArtifact::FileChange(change) => {
                assert_eq!(change.op, FileChangeOp::Add);
                assert_eq!(change.path, "notes.txt");
                assert_eq!(change.tool_call_id.as_deref(), Some("c1"));
                assert_eq!(change.additions, 2);
                assert_eq!(change.deletions, 0);
            }
            other => panic!("expected FileChange, got {other:?}"),
        }
        let second = run_tool_call(
            &ctx,
            ToolCall {
                id: "c2".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "notes.txt", "content": "a\nc\n"}),
            },
        )
        .await
        .expect("overwrite succeeds");
        assert_eq!(second.artifacts.len(), 1, "one artifact for the overwrite");
        match &second.artifacts[0] {
            TurnArtifact::FileChange(change) => {
                assert_eq!(change.op, FileChangeOp::Modify);
                assert_eq!(change.path, "notes.txt");
                assert_eq!(change.tool_call_id.as_deref(), Some("c2"));
                assert_eq!(change.additions, 1);
                assert_eq!(change.deletions, 1);
                let diff = change.diff.as_deref().expect("small diff is kept");
                assert!(diff.contains("-b"), "diff: {diff}");
                assert!(diff.contains("+c"), "diff: {diff}");
            }
            other => panic!("expected FileChange, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_artifact_skipped_when_pre_read_errors() {
        let root = temp_root("pre-error");
        let target = root.join("bad.txt");
        std::fs::write(&target, [0xffu8, 0xfe]).expect("invalid utf-8 fixture");
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(FsWriteTool),
        );
        let outcome = run_tool_call(
            &ctx,
            ToolCall {
                id: "c1".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "bad.txt", "content": "fixed\n"}),
            },
        )
        .await
        .expect("write still succeeds");
        assert!(
            outcome.result.is_object(),
            "tool result is returned normally"
        );
        assert!(
            outcome.artifacts.is_empty(),
            "pre-read error skips the artifact"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "fixed\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn write_artifact_skipped_when_post_read_fails() {
        let root = temp_root("post-error");
        let ctx = write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession {
                root: Some(root.clone()),
            }),
            Arc::new(PhantomWriteTool),
        );
        let outcome = run_tool_call(
            &ctx,
            ToolCall {
                id: "c1".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path": "ghost.txt", "content": "x\n"}),
            },
        )
        .await
        .expect("phantom write still succeeds");
        assert_eq!(outcome.result["ok"], true);
        assert!(
            outcome.artifacts.is_empty(),
            "post-read failure skips the artifact"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    struct TodoMock {
        reject: bool,
    }

    impl Tool for TodoMock {
        fn name(&self) -> &str {
            "todo"
        }

        fn description(&self) -> &str {
            ""
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            let items = input["items"].as_array().cloned().unwrap_or_default();
            if self.reject {
                return Err(ToolError::ExecutionFailed {
                    name: "todo".into(),
                    reason: "items[0].content must be a non-empty string".into(),
                });
            }
            Ok(serde_json::json!({"ok": true, "count": items.len()}))
        }
    }

    fn todo_call(id: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "todo".into(),
            arguments: args,
        }
    }

    fn todo_ctx(reject: bool) -> KernelContext {
        write_executor_ctx(
            Arc::new(AllowAll),
            Arc::new(MockSession { root: None }),
            Arc::new(TodoMock { reject }),
        )
    }

    #[tokio::test]
    async fn todo_tool_call_yields_snapshot_artifact() {
        use crate::artifacts::TodoStatusArtifact;
        let ctx = todo_ctx(false);
        let outcome = run_tool_call(
            &ctx,
            todo_call(
                "c1",
                serde_json::json!({
                    "items": [
                        {"content": "first", "status": "in_progress"},
                        {"content": "second", "status": "completed"}
                    ]
                }),
            ),
        )
        .await
        .expect("todo tool succeeds");
        assert_eq!(outcome.result["count"], 2);
        assert_eq!(outcome.artifacts.len(), 1, "one todo snapshot artifact");
        match &outcome.artifacts[0] {
            TurnArtifact::Todo(snapshot) => {
                assert_eq!(snapshot.items.len(), 2, "two ids for two items");
                assert_eq!(snapshot.items[0].content, "first");
                assert_eq!(snapshot.items[0].status, TodoStatusArtifact::InProgress);
                assert_eq!(snapshot.items[1].content, "second");
                assert_eq!(snapshot.items[1].status, TodoStatusArtifact::Completed);
                assert!(
                    snapshot.items[0].id.starts_with('t'),
                    "id is content-derived: {}",
                    snapshot.items[0].id
                );
                assert_ne!(snapshot.items[0].id, snapshot.items[1].id);
            }
            other => panic!("expected Todo artifact, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn todo_tool_rejection_yields_no_artifacts() {
        let ctx = todo_ctx(true);
        let reply = run_tool_call(
            &ctx,
            todo_call("c1", serde_json::json!({"items": [{"content": ""}]})),
        )
        .await;
        assert!(
            matches!(reply, Err(ToolError::ExecutionFailed { .. })),
            "rejected todo keeps its error and carries no artifacts: {reply:?}"
        );
    }

    #[tokio::test]
    async fn invalid_todo_args_on_ok_result_skip_artifact() {
        let ctx = todo_ctx(false);
        let outcome = run_tool_call(
            &ctx,
            todo_call(
                "c1",
                serde_json::json!({"items": [{"content": "ok", "status": "doing"}]}),
            ),
        )
        .await
        .expect("tool Ok is preserved even when the snapshot cannot be derived");
        assert_eq!(outcome.result["ok"], true, "tool result is untouched");
        assert!(
            outcome.artifacts.is_empty(),
            "unparsable todo args skip the artifact"
        );
    }

    #[tokio::test]
    async fn todo_clear_emits_empty_snapshot_artifact() {
        let ctx = todo_ctx(false);
        let outcome = run_tool_call(&ctx, todo_call("c1", serde_json::json!({"items": []})))
            .await
            .expect("empty todo clear succeeds");
        match &outcome.artifacts[0] {
            TurnArtifact::Todo(snapshot) => {
                assert!(snapshot.items.is_empty(), "[] is a legal clear snapshot");
            }
            other => panic!("expected Todo artifact, got {other:?}"),
        }
    }
}
