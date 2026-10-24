//! Crate-internal `agentj-ipc` tests, split out of `lib.rs`.

mod lifecycle;

use super::*;
use crate::handlers::run::build_run_reply;
use crate::test_support::{
    MockConfig, stub_bus_server, stub_bus_server_with_usage, stub_server, stub_session,
};
use agentj_context::ContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::*;
use agentj_core::permission::{PermissionChecker, PermissionContext, PermissionDecision};
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::*;
use agentj_model::mock::MockModelProvider;
use agentj_protocol::session::SessionRunResult;
use agentj_session::InMemoryStore;
use std::sync::Arc;

/// Crate-internal fixtures shared by lib and tcp test modules.
pub(crate) mod test_support {
    use agentj_context::ContextPipeline;
    use agentj_core::agent_loop_module::AgentLoopModule;
    use agentj_core::module::*;
    use agentj_core::tool_executor::ToolExecutorModule;
    use agentj_core::turn_runner::TurnRunner;
    use agentj_core::types::*;
    use agentj_model::mock::MockModelProvider;
    use agentj_session::InMemoryStore;
    use std::sync::Arc;

    pub(crate) struct MockConfig;
    impl ConfigProvider for MockConfig {
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

    pub(crate) fn stub_session() -> Arc<InMemoryStore> {
        Arc::new(InMemoryStore::new())
    }

    #[allow(dead_code)]
    pub(crate) fn stub_server() -> crate::IpcServer {
        let session = stub_session();
        crate::IpcServer::new(agentj_core::kernel::Kernel::new(), session)
    }

    /// Build an IpcServer whose kernel has AgentLoopModule + ToolExecutorModule
    /// wired to a MockModelProvider emitting `hello`.
    pub(crate) fn stub_bus_server() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_bus_server_with_usage() -> crate::IpcServer {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Usage(TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                })),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn.clone(),
            tools: tools.clone(),
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let mut kernel = agentj_core::kernel::Kernel::new();
        let mut register =
            ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
        kernel
            .register_module(
                Box::new(ToolExecutorModule::new(ctx.clone())),
                &ctx,
                &mut register,
            )
            .unwrap();
        let runner = TurnRunner::new(
            ctx.clone(),
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        );
        kernel
            .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
            .unwrap();
        crate::IpcServer::new(kernel, session_dyn)
    }

    #[allow(dead_code)]
    pub(crate) fn stub_agent_loop() -> TurnRunner {
        let session: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
        let session_dyn: Arc<dyn SessionStore> = session.clone();
        let tools = Arc::new(ToolRegistry::new());
        let providers = Arc::new(ProviderRegistry::new());
        providers.register(
            "mock".into(),
            Arc::new(MockModelProvider::new(vec![
                Ok(ModelEvent::Delta("hello".into())),
                Ok(ModelEvent::Done),
            ])),
        );
        let ctx = KernelContext {
            session: session_dyn,
            tools,
            providers: providers.clone(),
            config: Arc::new(MockConfig),
        };
        let kernel = agentj_core::kernel::Kernel::new();
        TurnRunner::new(
            ctx,
            Arc::new(ContextPipeline::new()),
            "You are helpful.".into(),
            kernel.sender(),
            "tool_executor".into(),
        )
    }
}

fn permission_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    permission_server_with(std::time::Duration::from_millis(50))
}

/// `permission_server` with an explicit approval timeout, so a test can hold
/// an approval pending long enough to observe *how* it gets settled.
fn permission_server_with(
    timeout: std::time::Duration,
) -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    let module = Arc::new(agentj_permission::PermissionModule::new(policies, timeout));
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    server.permission_module = Some(module.clone());
    (server, module)
}

/// Test ProviderService double: records every call and returns minimal results.
#[derive(Default)]
struct Recording {
    calls: std::sync::Mutex<Vec<String>>,
    set_api_key_params: std::sync::Mutex<Vec<SetApiKeyCall>>,
    clear_credential_params: std::sync::Mutex<Vec<Option<String>>>,
}

/// One `set_api_key` invocation as the double observed it: `(provider, api_key, label)`.
type SetApiKeyCall = (Option<String>, String, Option<String>);

#[async_trait]
impl ProviderService for Recording {
    async fn model_list(&self, _p: ModelListParams) -> Result<ModelListResult, DispatchError> {
        lock_or_recover(&self.calls).push("model_list".into());
        Ok(ModelListResult {
            provider: "opencode-go".into(),
            default_model: "m".into(),
            source: agentj_protocol::provider::ModelSource::Overlay,
            fetched_at: 1,
            models: vec![],
        })
    }
    async fn auth_status(&self, p: AuthStatusParams) -> Result<AuthStatusResult, DispatchError> {
        lock_or_recover(&self.calls).push("auth_status".into());
        Ok(AuthStatusResult {
            provider: p.provider.unwrap_or_else(|| "opencode-go".into()),
            authenticated: true,
            credential: None,
            models_count: 0,
        })
    }
    async fn usage(&self, _p: ProviderUsageParams) -> Result<ProviderUsageResult, DispatchError> {
        lock_or_recover(&self.calls).push("usage".into());
        Ok(ProviderUsageResult {
            provider: "opencode-go".into(),
            fetched_at: 1,
            stale: false,
            warn_percent: 80,
            windows: vec![],
            error: None,
        })
    }
    async fn device_start(
        &self,
        _p: AuthDeviceStartParams,
    ) -> Result<AuthDeviceStartResult, DispatchError> {
        lock_or_recover(&self.calls).push("device_start".into());
        Ok(AuthDeviceStartResult {
            provider: "opencode-go".into(),
            device_code: "dc".into(),
            user_code: "UC".into(),
            verification_uri: "u".into(),
            expires_in: 600,
            interval: 5,
        })
    }
    async fn device_poll(
        &self,
        _p: AuthDevicePollParams,
    ) -> Result<AuthDevicePollResult, DispatchError> {
        lock_or_recover(&self.calls).push("device_poll".into());
        Ok(AuthDevicePollResult {
            state: agentj_protocol::provider::DeviceAuthState::Pending,
            interval: None,
            message: None,
            credential: None,
        })
    }
    async fn set_api_key(
        &self,
        p: AuthSetApiKeyParams,
    ) -> Result<AuthSetApiKeyResult, DispatchError> {
        lock_or_recover(&self.calls).push("set_api_key".into());
        lock_or_recover(&self.set_api_key_params).push((
            p.provider.clone(),
            p.api_key.clone(),
            p.label.clone(),
        ));
        Ok(AuthSetApiKeyResult {
            provider: p.provider.unwrap_or_else(|| "opencode-go".into()),
            saved: true,
            outcome: agentj_protocol::provider::ApiKeyOutcome::Verified,
            error: None,
            credential: None,
            source: None,
            effective_source: agentj_protocol::provider::CredentialSource::File,
            fetched_at: 1,
            models: vec![],
        })
    }
    async fn clear_credential(
        &self,
        p: AuthClearCredentialParams,
    ) -> Result<AuthClearCredentialResult, DispatchError> {
        lock_or_recover(&self.calls).push("clear_credential".into());
        lock_or_recover(&self.clear_credential_params).push(p.provider.clone());
        Ok(AuthClearCredentialResult {
            provider: p.provider.unwrap_or_else(|| "opencode-go".into()),
            cleared: true,
        })
    }
}

#[tokio::test]
async fn handle_is_optional_and_the_trait_is_object_safe() {
    assert!(stub_server().provider_service.is_none());

    let rec = Arc::new(Recording::default());
    let svc: Arc<dyn ProviderService> = rec.clone();
    assert_eq!(
        svc.model_list(ModelListParams {
            provider: None,
            refresh: None
        })
        .await
        .unwrap()
        .provider,
        "opencode-go"
    );
    assert_eq!(lock_or_recover(&rec.calls).as_slice(), ["model_list"]);
}

#[test]
fn dispatch_error_is_constructible_from_outside_the_crate() {
    let e = DispatchError::invalid_params_with_data(
        "InvalidParams",
        serde_json::json!({"field": "provider"}),
    );
    assert_eq!(e.code, error_codes::INVALID_PARAMS);
    assert_eq!(e.data.unwrap()["field"], "provider");
    assert!(
        crate::tcp::TcpBindOptions::default()
            .provider_service
            .is_none()
    );
}

async fn call(server: &IpcServer, method: &str, params: serde_json::Value) -> serde_json::Value {
    let raw =
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":method,"params":params}).to_string();
    serde_json::from_str(&server.handle_request_from(&raw, "c1").await.unwrap()).unwrap()
}

#[tokio::test]
async fn subscription_methods_forward_to_the_service() {
    let mut server = stub_server();
    let rec = Arc::new(Recording::default());
    let svc: Arc<dyn ProviderService> = rec.clone();
    server.provider_service = Some(svc);

    let v = call(
        &server,
        "auth/status",
        serde_json::json!({"provider":"opencode-go"}),
    )
    .await;
    assert_eq!(v["result"]["provider"], "opencode-go");
    assert!(v.get("error").is_none());
    assert_eq!(lock_or_recover(&rec.calls).len(), 1);

    for (method, params, name) in [
        ("model/list", serde_json::json!({}), "model_list"),
        ("provider/usage", serde_json::json!({}), "usage"),
        ("auth/deviceStart", serde_json::json!({}), "device_start"),
        (
            "auth/devicePoll",
            serde_json::json!({"device_code":"dc"}),
            "device_poll",
        ),
        (
            "auth/setApiKey",
            serde_json::json!({"provider":"opencode-go","api_key":"sk-x"}),
            "set_api_key",
        ),
        (
            "auth/clearCredential",
            serde_json::json!({"provider":"opencode-go"}),
            "clear_credential",
        ),
    ] {
        let v = call(&server, method, params).await;
        assert!(v.get("error").is_none(), "{method} must succeed: {v}");
        assert!(v["result"].is_object(), "{method} result must be an object");
        assert!(lock_or_recover(&rec.calls).iter().any(|c| c == name));
    }
}

#[tokio::test]
async fn set_api_key_and_clear_credential_forward_to_the_service() {
    let mut server = stub_server();
    let rec = Arc::new(Recording::default());
    let svc: Arc<dyn ProviderService> = rec.clone();
    server.provider_service = Some(svc);

    let v = call(
        &server,
        "auth/setApiKey",
        serde_json::json!({"provider":"opencode-go","api_key":"sk-x"}),
    )
    .await;
    assert!(v.get("error").is_none(), "auth/setApiKey must succeed: {v}");
    assert_eq!(v["result"]["provider"], "opencode-go");
    assert_eq!(v["result"]["outcome"], "verified");
    assert!(v["result"].get("api_key").is_none());
    assert_eq!(
        lock_or_recover(&rec.set_api_key_params).as_slice(),
        [(Some("opencode-go".into()), "sk-x".to_string(), None)]
    );

    let v = call(
        &server,
        "auth/clearCredential",
        serde_json::json!({"provider":"opencode-go"}),
    )
    .await;
    assert!(
        v.get("error").is_none(),
        "auth/clearCredential must succeed: {v}"
    );
    assert_eq!(v["result"]["cleared"], true);
    assert_eq!(
        lock_or_recover(&rec.clear_credential_params).as_slice(),
        [Some("opencode-go".to_string())]
    );
    assert_eq!(
        lock_or_recover(&rec.calls).as_slice(),
        ["set_api_key", "clear_credential"]
    );
}

#[tokio::test]
async fn missing_service_is_internal_error_and_bad_params_are_invalid_params() {
    let server = stub_server();
    for method in [
        "model/list",
        "auth/status",
        "provider/usage",
        "auth/deviceStart",
        "auth/devicePoll",
        "auth/setApiKey",
        "auth/clearCredential",
    ] {
        let v = call(&server, method, serde_json::json!({})).await;
        assert_eq!(
            v["error"]["code"], -32603,
            "{method} must report the missing handle"
        );
        assert_eq!(v["error"]["data"]["reason"], "provider_service_unavailable");
    }

    let mut server = stub_server();
    let rec = Arc::new(Recording::default());
    let svc: Arc<dyn ProviderService> = rec.clone();
    server.provider_service = Some(svc);
    let v = call(
        &server,
        "auth/devicePoll",
        serde_json::json!({"device_code": 42}),
    )
    .await;
    assert_eq!(v["error"]["code"], -32602);
    assert!(
        lock_or_recover(&rec.calls).is_empty(),
        "service must not be called on bad params"
    );

    let v = call(
        &server,
        "auth/setApiKey",
        serde_json::json!({"api_key": 42}),
    )
    .await;
    assert_eq!(v["error"]["code"], -32602);
    assert_eq!(v["error"]["data"]["reason"], "invalid_params");
    assert!(
        lock_or_recover(&rec.calls).is_empty(),
        "service must not be called on bad params"
    );

    let v = call(&server, "kernel/status", serde_json::json!({})).await;
    assert!(v.get("error").is_none(), "existing dispatch unaffected");

    let v = call(&server, "no/such/method", serde_json::json!({})).await;
    assert_eq!(v["error"]["code"], -32601);
}

/// Fixture for the cancel regression test: the AgentLoop's ToolRegistry is
/// wired to the SAME PermissionModule the server dispatches into, and the
/// mock script emits a `bash` ToolCall — so a session/run genuinely blocks
/// on a permission Ask while in-flight (the real cancel scenario).
fn blocking_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    blocking_run_server_with(1)
}

/// As [`blocking_run_server`], but the mock script emits `tool_calls` `bash` calls
/// in ONE model round: the run then asks for that many approvals in sequence (a
/// later one arrives only after the previous is settled, i.e. during the drain).
fn blocking_run_server_with(
    tool_calls: usize,
) -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    let module = Arc::new(agentj_permission::PermissionModule::new(
        policies,
        std::time::Duration::from_secs(10),
    ));
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    tools.set_permission_checker(module.clone());
    let providers = Arc::new(ProviderRegistry::new());
    let script = (0..tool_calls)
        .map(|i| {
            Ok(ModelEvent::ToolCall(ToolCall {
                id: format!("call_{i}"),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "curl x"}),
            }))
        })
        .collect();
    providers.register("mock".into(), Arc::new(MockModelProvider::new(script)));
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut register =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut register,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    let mut server = IpcServer::new(kernel, session_dyn);
    server.permission_module = Some(module.clone());
    (server, module)
}

#[tokio::test]
async fn permission_respond_owner_gate_is_per_session() {
    let (server, module) = permission_server();
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "conn-a");
    write_or_recover(&server.registry).set_session_owner(&"s2".to_string(), "conn-b");
    let ctx1 = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d1 = module.check(&ctx1);
    let PermissionDecision::Ask { request_id: rid1 } = d1 else {
        panic!("expected Ask")
    };
    let raw1 = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid1.0
    );
    let resp = server.handle_request_from(&raw1, "conn-b").await.unwrap();
    assert!(
        resp.contains("-32806"),
        "non-owner must get NotAuthorized: {resp}"
    );
    let resp = server.handle_request_from(&raw1, "conn-a").await.unwrap();
    assert!(resp.contains("\"result\""), "owner must resolve: {resp}");
    let ctx2 = PermissionContext {
        session_id: "s2".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl y"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d2 = module.check(&ctx2);
    let PermissionDecision::Ask { request_id: rid2 } = d2 else {
        panic!("expected Ask")
    };
    let raw2 = format!(
        r#"{{"jsonrpc":"2.0","id":8,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid2.0
    );
    let resp = server.handle_request_from(&raw2, "conn-b").await.unwrap();
    assert!(resp.contains("\"result\""), "s2 owner must resolve: {resp}");
}

#[tokio::test]
async fn permission_respond_resolves_pending_approval() {
    let (server, module) = permission_server();
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let d = module.check(&ctx);
    if let PermissionDecision::Ask { request_id } = d {
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw = format!(
            concat!(
                r#"{{"jsonrpc":"2.0","id":7,"method":"permission/respond","#,
                r#""params":{{"request_id":"{}","decision":"allow"}}}}"#,
            ),
            request_id.0
        );
        let resp = server.handle_request(&raw).await.unwrap();
        assert!(resp.contains("\"result\""));
        assert!(matches!(handle.await.unwrap(), PermissionDecision::Allow));
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn permission_respond_unknown_request_id_errors() {
    let (server, _module) = permission_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":8,"method":"permission/respond","#,
        r#""params":{"request_id":"nope","decision":"allow"}}"#,
    );
    let resp = server.handle_request(raw).await.unwrap();
    assert!(resp.contains("\"error\""));
    assert!(resp.contains("-32600") || resp.contains("InvalidRequest") || resp.contains("unknown"));
}

#[tokio::test]
async fn cancel_request_resolves_pending_to_deny() {
    let (server, _module) = permission_server();
    let module = server.permission_module.clone().unwrap();
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw =
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
        let _ = server.handle_request(raw).await.unwrap();
        assert_eq!(
            handle.await.unwrap(),
            PermissionDecision::Deny {
                reason: "cancelled".into()
            }
        );
    } else {
        panic!("expected Ask");
    }
}

/// BUG-004: only the session owner may answer an approval (design 决策 #5),
/// so once that connection is gone nobody ever can — the approval must be
/// settled at disconnect (`PermissionModule::cancel_session` documents
/// "session termination / $/cancelRequest / client disconnect"). Left
/// pending, the orphan run blocks for the whole approval timeout (60s
/// default) and the session stays busy for a reconnecting client.
#[tokio::test]
async fn owner_disconnect_denies_pending_approval() {
    let (server, _module) = permission_server_with(std::time::Duration::from_secs(30));
    let module = server.permission_module.clone().unwrap();
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "conn-a");
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask");
    };
    let module2 = module.clone();
    let handle = tokio::spawn(async move { module2.await_approval(&request_id).await });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    server.cleanup_connection("conn-a");
    let decision = tokio::time::timeout(std::time::Duration::from_millis(500), handle)
        .await
        .expect("owner disconnect must settle the pending approval")
        .unwrap();
    assert_eq!(
        decision,
        PermissionDecision::Deny {
            reason: "disconnected".into()
        }
    );
}

/// Fixture: an IpcServer wired to a fresh QuestionModule (notifier not yet attached).
fn question_server() -> (IpcServer, Arc<agentj_question::QuestionModule>) {
    let module = Arc::new(agentj_question::QuestionModule::new());
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    server.question_module = Some(module.clone());
    (server, module)
}

fn ask_params(sid: &str) -> agentj_protocol::question::AskQuestionParams {
    use agentj_protocol::question::AskOption;
    agentj_protocol::question::AskQuestionParams {
        question_id: String::new(), // ask_blocking generates the real id
        session_id: sid.to_string(),
        question: "Pick a color".into(),
        header: None,
        options: vec![
            AskOption {
                id: "red".into(),
                label: "Red".into(),
                hint: None,
            },
            AskOption {
                id: "blue".into(),
                label: "Blue".into(),
                hint: None,
            },
        ],
        allow_multiple: false,
        allow_other: true,
        timeout_s: None,
    }
}

fn question_id_of(msg: &str) -> String {
    serde_json::from_str::<serde_json::Value>(msg).unwrap()["params"]["question_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn respond_request(qid: &str, answers: &str) -> String {
    format!(
        concat!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"question/respond","#,
            r#""params":{{"question_id":"{}","answers":{}}}}}"#
        ),
        qid, answers
    )
}

/// Wait for the owner's `$/askQuestion` notification; return the generated id.
async fn wait_for_ask_question(server: &Arc<IpcServer>, owner: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(msg) = drain_sent(server, owner)
                .into_iter()
                .find(|m| m.contains("$/askQuestion"))
            {
                return question_id_of(&msg);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed out waiting for $/askQuestion")
}

#[tokio::test]
async fn question_respond_non_owner_rejected() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let respond = respond_request(&qid, r#"["red"]"#);
    let resp = server.handle_request_from(&respond, &b).await.unwrap();
    assert!(
        resp.contains("-32806") && resp.contains("NotAuthorized"),
        "non-owner must be rejected with -32806: {resp}"
    );
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(
        resp.contains("\"accepted\":true"),
        "owner must resolve: {resp}"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(
        outcome,
        agentj_question::QuestionOutcome::Answered {
            answers: vec!["red".into()]
        }
    );
}

#[tokio::test]
async fn question_respond_unknown_id_is_idempotent_false() {
    let (server, module) = question_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":4,"method":"question/respond","#,
        r#""params":{"question_id":"nope","answers":[]}}"#,
    );
    let resp = server.handle_request_from(raw, "conn-x").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["accepted"], false, "{resp}");
    assert!(
        v.get("error").is_none(),
        "unknown id must not error: {resp}"
    );
    assert_eq!(module.pending_count(), 0);
}

#[tokio::test]
async fn cancel_request_settles_pending_question() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    assert_eq!(module.pending_count(), 1);
    let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#;
    let _ = server.handle_request_from(raw, &a).await.unwrap();
    assert_eq!(
        module.pending_count(),
        0,
        "cancel must settle the pending question"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
}

#[tokio::test]
async fn notifier_targets_owner_connection() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let ask = drain_sent(&server, &a)
        .into_iter()
        .find(|m| m.contains("$/askQuestion"))
        .expect("owner must receive $/askQuestion");
    let v: serde_json::Value = serde_json::from_str(&ask).unwrap();
    assert_eq!(v["method"], "$/askQuestion");
    assert_eq!(v["params"]["question_id"], qid);
    assert_eq!(v["params"]["session_id"], "s1");
    assert_eq!(v["params"]["question"], "Pick a color");
    assert_eq!(v["params"]["options"].as_array().unwrap().len(), 2);
    let b_sent = drain_sent(&server, &b);
    assert!(
        !b_sent.iter().any(|m| m.contains("$/askQuestion")),
        "non-owner must NOT receive $/askQuestion: {b_sent:?}"
    );
    let _ = module.cancel_session("s1");
    assert_eq!(
        handle.join().unwrap().1,
        agentj_question::QuestionOutcome::Skipped
    );
}

#[tokio::test]
async fn question_respond_maps_validation_errors_to_error_data() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let unknown = respond_request(&qid, r#"["purple"]"#);
    let resp = server.handle_request_from(&unknown, &a).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "{resp}");
    assert_eq!(v["error"]["data"]["unknown_options"][0], "purple", "{resp}");
    let too_many = respond_request(&qid, r#"["red","blue"]"#);
    let resp = server.handle_request_from(&too_many, &a).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "{resp}");
    assert_eq!(v["error"]["data"]["reason"], "too_many_answers", "{resp}");
    assert_eq!(
        module.pending_count(),
        1,
        "rejected validation must leave the question pending"
    );
    let _ = module.cancel_session("s1");
    handle.join().unwrap();
}

#[tokio::test]
async fn owner_disconnect_settles_pending_question_skipped() {
    let (server, module) = question_server();
    let server = Arc::new(server);
    server.attach_question_notifier();
    let (conn, log, tx) = scripted_conn("tcp-owner");
    let s = Arc::clone(&server);
    let task = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });
    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |m| m.contains("\"session_id\"")).await;
    let sid = extract_sid(&new_resp);
    let m = module.clone();
    let params = ask_params(&sid);
    let handle = std::thread::spawn(move || m.ask_blocking(&sid, &params));
    let ask_line = wait_for_line(&log, |m| m.contains("$/askQuestion")).await;
    let qid = question_id_of(&ask_line);
    assert_eq!(module.pending_count(), 1);
    drop(tx); // EOF -> serve_connection unregisters and settles owned sessions
    task.await.unwrap().unwrap();
    assert_eq!(
        module.pending_count(),
        0,
        "disconnect must settle pending questions"
    );
    let (settled_id, outcome) = handle.join().unwrap();
    assert_eq!(settled_id, qid);
    assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
}

#[tokio::test]
async fn question_settle_records_ask_and_answer_trajectory_once() {
    let dir = temp_dir_traj("question");
    let (mut server, module) = question_server();
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    server.attach_question_notifier();
    let a = register_fake_conn(&server);
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), &a);
    let m = module.clone();
    let params = ask_params("s1");
    let handle = std::thread::spawn(move || m.ask_blocking("s1", &params));
    let qid = wait_for_ask_question(&server, &a).await;
    let respond = respond_request(&qid, r#"["red"]"#);
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(resp.contains("\"accepted\":true"), "{resp}");
    assert_eq!(
        handle.join().unwrap().1,
        agentj_question::QuestionOutcome::Answered {
            answers: vec!["red".into()]
        }
    );
    // Duplicate respond is idempotent and must NOT record a second answer.
    let resp = server.handle_request_from(&respond, &a).await.unwrap();
    assert!(resp.contains("\"accepted\":false"), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["ask_question", "question_answer"]);
    std::fs::remove_dir_all(&dir).ok();
}

/// M1 regression: JSON-RPC 2.0 §2.1 — a notification (request without an
/// id, e.g. $/cancelRequest) must produce NO response line, while a
/// request WITH an id still gets its response.
#[tokio::test]
async fn id_less_notification_gets_no_response() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","method":"session/new"}"#)
        .await
        .unwrap();
    assert!(resp.is_empty(), "notification must not respond: {resp}");
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    assert!(!resp.is_empty(), "id'd request must respond");
    assert!(resp.contains("\"result\""));
}

async fn wait_for_pending_run(
    server: &Arc<IpcServer>,
    sid: &str,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if read_or_recover(&server.registry).pending_runs.contains(sid) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
}

/// Register a pending Ask approval for `sid` and await it in a task.
fn spawn_pending_approval(
    module: &Arc<agentj_permission::PermissionModule>,
    sid: &str,
) -> tokio::task::JoinHandle<PermissionDecision> {
    let ctx = PermissionContext {
        session_id: sid.to_string(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
        let module2 = module.clone();
        let rid = request_id.clone();
        tokio::spawn(async move { module2.await_approval(&rid).await })
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn session_run_records_inflight_session_for_cancel() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&new_resp).unwrap();
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let server = Arc::clone(&server);
        async move { server.handle_request(&raw).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("session/run must record the in-flight session id");
    let pending = spawn_pending_approval(&module, &sid);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let cancel = format!(
        r#"{{"jsonrpc":"2.0","method":"$/cancelRequest","params":{{"id":5,"session_id":"{sid}"}}}}"#
    );
    let _ = server.handle_request(&cancel).await.unwrap();
    assert_eq!(
        pending.await.unwrap(),
        PermissionDecision::Deny {
            reason: "cancelled".into()
        }
    );
    let run_resp = run_task.await.unwrap();
    assert!(
        run_resp.contains("\"result\"") || run_resp.contains("-32800"),
        "cancel should either complete or return RequestCancelled: {run_resp}"
    );
    assert!(
        !read_or_recover(&server.registry)
            .pending_runs
            .contains(&sid),
        "pending run cleared when the run finished"
    );
}

struct BlockerTool;
impl agentj_core::module::Tool for BlockerTool {
    fn name(&self) -> &str {
        "blocker"
    }
    fn description(&self) -> &str {
        "blocks"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn execute(
        &self,
        _i: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        std::thread::sleep(std::time::Duration::from_secs(1));
        Ok(serde_json::json!({"ok": 1}))
    }
}

struct RepeatingMock {
    c1: std::sync::atomic::AtomicUsize,
    c2: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for RepeatingMock {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        let is_s2 = messages.iter().any(|m| m.content.contains("run2"));
        let n = if is_s2 {
            self.c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        } else {
            self.c1.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        };
        if n == 0 {
            let (tn, args) = if is_s2 {
                ("blocker", serde_json::json!({}))
            } else {
                ("bash", serde_json::json!({"command": "echo hi"}))
            };
            let ev = vec![Ok(ModelEvent::ToolCall(ToolCall {
                id: "call_1".into(),
                name: tn.into(),
                arguments: args,
            }))];
            Ok(Box::pin(futures::stream::iter(ev)))
        } else {
            let ev = vec![Ok(ModelEvent::Delta("done".into())), Ok(ModelEvent::Done)];
            Ok(Box::pin(futures::stream::iter(ev)))
        }
    }
    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<ChatResponse, agentj_core::error::ModelError> {
        Ok(ChatResponse {
            content: "".into(),
            tool_calls: vec![],
        })
    }
}

fn repeating_run_server() -> (IpcServer, Arc<agentj_permission::PermissionModule>) {
    let mut policies = std::collections::HashMap::new();
    policies.insert(
        "bash".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Ask,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    policies.insert(
        "blocker".to_string(),
        agentj_permission::ToolPolicy {
            mode: agentj_permission::PermissionMode::Allow,
            path_rules: vec![],
            command_rules: vec![],
        },
    );
    let module = Arc::new(agentj_permission::PermissionModule::new(
        policies,
        std::time::Duration::from_secs(10),
    ));
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(BlockerTool));
    tools.set_permission_checker(module.clone());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register(
        "mock".into(),
        Arc::new(RepeatingMock {
            c1: std::sync::atomic::AtomicUsize::new(0),
            c2: std::sync::atomic::AtomicUsize::new(0),
        }),
    );
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut reg =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut reg,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut reg)
        .unwrap();
    let mut server = IpcServer::new(kernel, session_dyn);
    server.permission_module = Some(module.clone());
    (server, module)
}

/// Provider double recording every `ModelOptions` it is handed, so the
/// `session/run` dispatch tests can assert which model actually reached the
/// provider (T9: per-run `provider`/`model` overrides).
struct CapturingMock(Arc<std::sync::Mutex<Vec<ModelOptions>>>);

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for CapturingMock {
    async fn chat_stream(
        &self,
        _m: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        options: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        lock_or_recover(&self.0).push(options);
        let ev = vec![Ok(ModelEvent::Delta("ok".into())), Ok(ModelEvent::Done)];
        Ok(Box::pin(futures::stream::iter(ev)))
    }
    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<ChatResponse, agentj_core::error::ModelError> {
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![],
        })
    }
}

/// Per-provider options logs of one capturing server. `mock` is registered
/// first and is therefore the *default* provider, `other` the alternative:
/// two separate sinks are what makes a `provider` override observable (a
/// turn served by `other` must leave `mock` empty, and vice versa).
struct CapturedRuns {
    mock: Arc<std::sync::Mutex<Vec<ModelOptions>>>,
    other: Arc<std::sync::Mutex<Vec<ModelOptions>>>,
}

/// Same wiring as `repeating_run_server` (session store + ToolRegistry +
/// ProviderRegistry + kernel modules), with TWO capturing providers — `mock`
/// first (hence the default), `other` second — each behind its own options
/// log, and no permission policies (the mocks never call a tool). Returns
/// the server and both logs.
fn capturing_run_server() -> (IpcServer, CapturedRuns) {
    let mock = Arc::new(std::sync::Mutex::new(Vec::new()));
    let other = Arc::new(std::sync::Mutex::new(Vec::new()));
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register("mock".into(), Arc::new(CapturingMock(Arc::clone(&mock))));
    providers.register("other".into(), Arc::new(CapturingMock(Arc::clone(&other))));
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut reg =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut reg,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut reg)
        .unwrap();
    (
        IpcServer::new(kernel, session_dyn),
        CapturedRuns { mock, other },
    )
}

/// Dispatch one `session/run` request against a fresh capturing server and
/// return the parsed response plus both providers' recorded `ModelOptions`.
async fn call_session_run(params: Value) -> (Value, CapturedRuns) {
    let (server, caps) = capturing_run_server();
    let raw = serde_json::json!({
        "jsonrpc": "2.0", "id": 9, "method": "session/run", "params": params,
    })
    .to_string();
    let line = server
        .handle_request_from(&raw, "conn-capture")
        .await
        .expect("dispatch returns a JSON-RPC line");
    let resp: Value = serde_json::from_str(&line).expect("response is JSON");
    (resp, caps)
}

#[tokio::test]
async fn run_turn_stream_with_routes_to_explicit_provider() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream_with(
        &sid,
        "hi",
        RunOverrides {
            provider: Some("other".into()),
            model: Some("m1".into()),
        },
    );
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(lock_or_recover(&caps.other).last().unwrap().model, "m1");
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "the default provider must not serve a turn named for another provider"
    );
}

#[tokio::test]
// Regression pin (also green before this task): proves the default path is unchanged.
async fn run_turn_stream_default_matches_overrides_default() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream(&sid, "hi");
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
    assert!(lock_or_recover(&caps.other).is_empty());
}

#[tokio::test]
async fn run_turn_stream_with_blank_overrides_are_absent() {
    let (server, caps) = capturing_run_server();
    let sid = server.create_session().unwrap();
    let mut rx = server.run_turn_stream_with(
        &sid,
        "hi",
        RunOverrides {
            provider: Some("   ".into()),
            model: Some(String::new()),
        },
    );
    while let Some(ev) = rx.recv().await {
        if matches!(ev, AgentEvent::Done { .. } | AgentEvent::Error(_)) {
            break;
        }
    }
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
    assert!(lock_or_recover(&caps.other).is_empty());
}

#[tokio::test]
async fn session_run_passes_model_override_to_provider() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "mock", "model": "custom-model"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "custom-model"
    );
}

/// The default provider is `mock` (registered first), so naming `other`
/// proves the wire `provider` string really reaches provider selection.
#[tokio::test]
async fn session_run_provider_override_selects_that_provider() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "other", "model": "other-model"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.other).last().unwrap().model,
        "other-model",
        "the named provider must serve the turn"
    );
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "the default provider must not serve a turn named for another provider"
    );
}

/// A well-formed but unregistered provider name must fail the run on the
/// wire with the actionable message the turn runner builds. Turn errors
/// reach the client as `AgentEvent::Error` -> `DispatchError::internal`
/// (code `-32603`); the registered-name list order is a `HashMap`
/// iteration, so only membership is asserted.
#[tokio::test]
async fn session_run_unknown_provider_fails_on_the_wire() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "nope"
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32603, "{resp}");
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("unknown provider: nope"), "{msg}");
    assert!(msg.contains("mock"), "{msg}");
    assert!(msg.contains("other"), "{msg}");
    assert!(
        lock_or_recover(&caps.mock).is_empty(),
        "no provider may serve a turn with an unknown provider name"
    );
    assert!(
        lock_or_recover(&caps.other).is_empty(),
        "no provider may serve a turn with an unknown provider name"
    );
}

#[tokio::test]
async fn session_run_without_overrides_uses_default_model() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi"
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
}

#[tokio::test]
async fn session_run_null_fields_are_absent() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": null, "model": null
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert_eq!(
        lock_or_recover(&caps.mock).last().unwrap().model,
        "deepseek-chat"
    );
}

#[tokio::test]
async fn session_run_blank_provider_is_treated_as_absent() {
    let (resp, caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": "   "
    }))
    .await;
    assert!(resp.get("error").is_none(), "{resp}");
    assert!(
        !lock_or_recover(&caps.mock).is_empty(),
        "default provider must still serve the turn"
    );
}

#[tokio::test]
async fn session_run_non_string_model_is_invalid_params() {
    let (resp, _caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "model": 123
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32602);
    assert_eq!(resp["error"]["data"]["field"], "model");
}

#[tokio::test]
async fn session_run_non_string_provider_is_invalid_params() {
    let (resp, _caps) = call_session_run(serde_json::json!({
        "session_id": "s1", "content": "hi", "provider": {}
    }))
    .await;
    assert_eq!(resp["error"]["code"], -32602);
    assert_eq!(resp["error"]["data"]["field"], "provider");
}

#[tokio::test]
async fn cancel_request_with_session_id_routes_precisely() {
    let (server, module) = repeating_run_server();
    let server = Arc::new(server);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    module.set_notifier(Arc::new(move |p: &PermissionRequestParams, sid: &str| {
        let _ = tx.send((sid.to_string(), p.request_id.clone()));
    }));
    let raw1 = r#"{"jsonrpc":"2.0","id":10,"method":"session/run","params":{"session_id":"s1","content":"run1"}}"#;
    let raw2 = r#"{"jsonrpc":"2.0","id":11,"method":"session/run","params":{"session_id":"s2","content":"run2"}}"#;
    let run1 = {
        let s = Arc::clone(&server);
        tokio::spawn(async move { s.handle_request_from(raw1, "conn-a").await.unwrap() })
    };
    let run2 = {
        let s = Arc::clone(&server);
        tokio::spawn(async move { s.handle_request_from(raw2, "conn-b").await.unwrap() })
    };
    wait_for_pending_run(&server, "s1")
        .await
        .expect("s1 must be in-flight");
    wait_for_pending_run(&server, "s2")
        .await
        .expect("s2 must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (sid, rid) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("s1 must block on Ask")
        .expect("channel closed");
    assert_eq!(sid, "s1", "first Ask must be s1");
    let rid_s1 = Some(rid);
    let cancel =
        r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s2"}}"#;
    let _ = server.handle_request_from(cancel, "conn-a").await.unwrap();
    let resp2 = tokio::time::timeout(std::time::Duration::from_secs(5), run2)
        .await
        .expect("cancelled run must finish")
        .unwrap();
    assert!(
        resp2.contains("-32800"),
        "cancelled must get -32800: {resp2}"
    );
    assert!(
        read_or_recover(&server.registry)
            .pending_runs
            .contains("s1"),
        "s1 stays"
    );
    let rid1 = rid_s1.clone().expect("s1 rid");
    let raw_ok = format!(
        r#"{{"jsonrpc":"2.0","id":12,"method":"permission/respond","params":{{"request_id":"{}","decision":"allow"}}}}"#,
        rid1
    );
    let _ = server.handle_request_from(&raw_ok, "conn-a").await.unwrap();
    let resp1 = tokio::time::timeout(std::time::Duration::from_secs(5), run1)
        .await
        .expect("s1 must finish")
        .unwrap();
    assert!(resp1.contains("\"result\""), "s1 must complete: {resp1}");
}

#[tokio::test]
async fn cancel_request_without_session_id_single_inflight_falls_back() {
    let (server, module) = blocking_run_server();
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    if let PermissionDecision::Ask { request_id } = module.check(&ctx) {
        let module2 = module.clone();
        let rid = request_id.clone();
        let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
        let _ = server.handle_request(raw).await.unwrap();
        assert_eq!(
            handle.await.unwrap(),
            PermissionDecision::Deny {
                reason: "cancelled".into()
            }
        );
    } else {
        panic!("expected Ask");
    }
}

#[tokio::test]
async fn cancel_request_without_session_id_multiple_inflight_is_noop() {
    let (server, module) = blocking_run_server();
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s1".to_string()));
    assert!(write_or_recover(&server.registry).try_acquire_run(&"s2".to_string()));
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask")
    };
    let module2 = module.clone();
    let rid = request_id.clone();
    let handle = tokio::spawn(async move { module2.await_approval(&rid).await });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let raw = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#;
    let _ = server.handle_request(raw).await.unwrap();
    let out = tokio::time::timeout(std::time::Duration::from_millis(200), handle).await;
    assert!(out.is_err(), "ambiguous cancel must NOT resolve approvals");
}

#[tokio::test]
async fn session_new_returns_valid_id() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["result"]["session_id"].is_string());
    assert!(!v["result"]["session_id"].as_str().unwrap().is_empty());
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(
        v["result"]["project_root"],
        cwd.to_str().unwrap(),
        "session/new without project_root must return the canonicalized cwd"
    );
}

/// Temp tree for `session/new` project_root tests (mirrors `temp_dir_traj`).
fn temp_dir_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-root-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// T20: `session/new {project_root}` stores the canonicalized directory on
/// the created session (design item 6).
#[tokio::test]
async fn session_new_with_project_root_persists_it() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let dir = temp_dir_root("persist");
    let canonical = dir.canonicalize().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session/new",
        "params": { "project_root": dir.to_str().unwrap() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "valid project_root must not error: {resp}"
    );
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    assert_eq!(
        v["result"]["project_root"],
        canonical.to_str().unwrap(),
        "session/new must echo the canonicalized request value"
    );
    assert_eq!(store.project_root(&sid), Some(canonical));
    std::fs::remove_dir_all(&dir).ok();
}

/// T20: a relative `project_root` is rejected with -32602 carrying
/// `error.data {path, reason}`; no session is created (validate before create).
#[tokio::test]
async fn session_new_rejects_relative_root_with_32602() {
    let dir = temp_dir_root("reject");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let server = IpcServer::new(Kernel::new(), store.clone());
    let raw =
        r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"project_root":"rel/path"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
    assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
    assert_eq!(v["error"]["data"]["path"], "rel/path");
    assert!(v["result"].is_null(), "no result on error: {resp}");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "invalid project_root must not create a session"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// T20 AC guard: a missing path and a regular file are also rejected with
/// -32602 + `error.data {path, reason}`.
#[tokio::test]
async fn session_new_rejects_missing_and_file_root_with_32602() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let dir = temp_dir_root("badshape");
    let missing = dir.join("missing");
    let file = dir.join("not-a-dir.txt");
    std::fs::write(&file, "x").unwrap();
    for bad in [missing.as_path(), file.as_path()] {
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/new",
            "params": { "project_root": bad.to_str().unwrap() },
        })
        .to_string();
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "must be invalid params: {resp}");
        assert_eq!(v["error"]["data"]["reason"], "invalid_project_root");
        assert_eq!(v["error"]["data"]["path"], bad.to_str().unwrap());
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// T20: the optional `data` key is omitted from the wire when None (additive
/// discipline), and carried when a parameterized error sets it.
#[test]
fn error_object_omits_data_when_none() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: serde_json::json!(9),
        result: None,
        error: Some(JsonRpcError {
            code: -32602,
            message: "bad params".into(),
            data: None,
        }),
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert!(
        v["error"].get("data").is_none(),
        "data key must be omitted when None: {v}"
    );
    assert_eq!(v["error"]["code"], -32602);
    assert_eq!(v["error"]["message"], "bad params");

    let err = DispatchError::invalid_params_with_data(
        "bad root",
        serde_json::json!({ "path": "/x", "reason": "invalid_project_root" }),
    );
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["code"], -32602);
    assert_eq!(v["data"]["reason"], "invalid_project_root");
}

#[tokio::test]
async fn session_run_streams_delta() {
    let server = stub_bus_server();
    let raw = concat!(
        r#"{"jsonrpc":"2.0","id":2,"method":"session/run","#,
        r#""params":{"session_id":"s1","content":"hi"}}"#,
    );
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    // when: session/run responds, then: result carries `content` per SessionRunResult
    assert_eq!(v["result"]["content"], "hello");
    assert!(v["result"].get("response").is_none());
}

#[test]
fn run_reply_omits_identity_when_unknown() {
    let result = SessionRunResult {
        content: "ok".into(),
        tool_calls: None,
        usage: None,
        run_id: None,
        provider: None,
        model: None,
    };
    let value = build_run_reply(&result);
    assert!(
        value.get("provider").is_none(),
        "must be absent, not null: {value}"
    );
    assert!(value.get("model").is_none());
}

#[test]
fn run_reply_carries_identity_when_known() {
    let result = SessionRunResult {
        content: "ok".into(),
        tool_calls: None,
        usage: None,
        run_id: None,
        provider: Some("a".into()),
        model: Some("m".into()),
    };
    let value = build_run_reply(&result);
    assert_eq!(value["provider"], "a");
    assert_eq!(value["model"], "m");
}

#[tokio::test]
async fn session_run_result_includes_usage_when_reported() {
    let server = stub_bus_server_with_usage();
    let raw = r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"], "hello");
    assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
    assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_object());
    assert_eq!(v["error"]["code"], -32601); // Method not found
    assert_eq!(v["id"], 3, "error response must echo request id");
}

#[tokio::test]
async fn cancel_returns_minus_32800_when_tool_never_replies() {
    use agentj_core::module::{KernelModule, Module, ModuleMetadata};
    struct BlockingTool;
    impl Module for BlockingTool {
        fn id(&self) -> String {
            "tool_executor".into()
        }
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: "tool_executor".into(),
                name: "blocking".into(),
                version: "0.1".into(),
                description: "".into(),
                author: "".into(),
            }
        }
        fn init(&mut self, _: &KernelContext, _: &mut ModuleRegister) -> Result<(), ModuleError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), ModuleError> {
            Ok(())
        }
    }
    impl KernelModule for BlockingTool {
        fn handle(
            &mut self,
            msg: &KernelMessage,
        ) -> Result<agentj_core::module::KernelResponse, ModuleError> {
            if let KernelMessage::ToolCallRequest { reply, .. } = msg
                && let Some(tx) = lock_or_recover(reply).take()
            {
                std::mem::forget(tx);
            }
            Ok(agentj_core::module::KernelResponse::Handled)
        }
    }
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    let providers = Arc::new(ProviderRegistry::new());
    providers.register(
        "mock".into(),
        Arc::new(MockModelProvider::new(vec![Ok(ModelEvent::ToolCall(
            ToolCall {
                id: "c1".into(),
                name: "dummy".into(),
                arguments: serde_json::json!({}),
            },
        ))])),
    );
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut register =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(Box::new(BlockingTool), &ctx, &mut register)
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "sys".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    let server = Arc::new(IpcServer::new(kernel, session_dyn));
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        async move { s.handle_request(&run_raw).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = server
        .handle_request(r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5}}"#)
        .await
        .unwrap();
    let resp = run_task.await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32800,
        "cancel must return -32800: {resp}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("RequestCancelled")
    );
}

#[tokio::test]
async fn session_run_missing_params_echoes_id_and_invalid_params() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/run"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["id"], 10,
        "error response must echo request id, got: {resp}"
    );
    assert_eq!(
        v["error"]["code"], -32602,
        "missing params is Invalid params, got: {resp}"
    );
    assert!(v["error"]["message"].is_string());
}

#[tokio::test]
async fn session_run_missing_session_id_echoes_id_and_invalid_params() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(
            r#"{"jsonrpc":"2.0","id":7,"method":"session/run","params":{"content":"hi"}}"#,
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["id"], 7,
        "error response must echo request id, got: {resp}"
    );
    assert_eq!(
        v["error"]["code"], -32602,
        "missing session_id is Invalid params, got: {resp}"
    );
}

#[tokio::test]
async fn invalid_json_returns_parse_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("not json").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32700); // Parse error
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[tokio::test]
async fn schema_invalid_request_echoes_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], 10);
}

#[tokio::test]
async fn schema_invalid_request_echoes_string_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":"abc","params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], "abc");
}

#[tokio::test]
async fn non_object_json_returns_invalid_request_with_null_id() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("42").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600);
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[test]
fn request_id_extraction_accepts_only_jsonrpc_id_types() {
    let cases = [
        (
            serde_json::json!({"id": "abc"}),
            Some(serde_json::json!("abc")),
        ),
        (serde_json::json!({"id": 7}), Some(serde_json::json!(7))),
        (
            serde_json::json!({"id": null}),
            Some(serde_json::Value::Null),
        ),
        (serde_json::json!({}), None),
        (serde_json::json!({"id": {"x": 1}}), None),
        (serde_json::json!({"id": [1]}), None),
        (serde_json::json!({"id": true}), None),
        (serde_json::json!("42"), None),
    ];
    for (value, expected) in cases {
        assert_eq!(extract_request_id(&value), expected, "value: {value}");
    }
}

#[tokio::test]
async fn kernel_completion_is_observable_for_signal_interrupt() {
    // design #13: the CLI needs to interrupt serve_stdio's blocked read_line when
    // SIGINT/SIGTERM arrives — signal_shutdown + awaiting the kernel handle must
    // terminate the loop. Run serve_stdio with a stdin that stays open, signal
    // shutdown from another task, and assert serve_stdio returns.
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    let tx = server.kernel_sender();
    let signal = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Tolerate a closed channel: when the test harness delivers stdin EOF
        // immediately, serve_stdio's graceful EOF path already shut the kernel down.
        let _ = tx.send(KernelMessage::Shutdown);
    });
    // serve_stdio reads stdin (blocked) until the kernel finishes; feed a
    // request first so the loop is live, then the signal task triggers exit.
    let run = tokio::spawn(async move {
        server.serve_stdio().await.unwrap();
    });
    signal.await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .expect("serve_stdio must exit after kernel shutdown")
        .unwrap();
}

type SharedLog = Arc<std::sync::Mutex<Vec<String>>>;

/// T5 fixture: `send` appends to a log the test keeps a handle to (the
/// boxed conn inside the registry cannot be downcast, so logs live outside).
struct FakeConn {
    id: String,
    log: SharedLog,
}

fn fake_conn(id: &str) -> (FakeConn, SharedLog) {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    (
        FakeConn {
            id: id.to_string(),
            log: std::sync::Arc::clone(&log),
        },
        log,
    )
}

#[async_trait]
impl Connection for FakeConn {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Stdio
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        if let Ok(mut log) = self.log.lock() {
            log.push(msg.to_string());
        }
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        None
    }
    fn peer_desc(&self) -> String {
        self.id.clone()
    }
    fn writer(&self) -> ConnectionWriter {
        let log = std::sync::Arc::clone(&self.log);
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(move |msg: &str| {
                if let Ok(mut log) = log.lock() {
                    log.push(msg.to_string());
                }
                Ok(())
            }),
        )
    }
}

/// Test connection driven by an mpsc channel: feed lines via `tx`, read
/// sends from `log`, drop `tx` to EOF (recv then returns None).
struct ScriptedConn {
    id: String,
    rx: tokio::sync::mpsc::Receiver<String>,
    log: SharedLog,
}

fn scripted_conn(id: &str) -> (ScriptedConn, SharedLog, tokio::sync::mpsc::Sender<String>) {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    (
        ScriptedConn {
            id: id.to_string(),
            rx,
            log: std::sync::Arc::clone(&log),
        },
        log,
        tx,
    )
}

#[async_trait]
impl Connection for ScriptedConn {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }
    fn send(&self, msg: &str) -> Result<(), IpcError> {
        if let Ok(mut log) = self.log.lock() {
            log.push(msg.to_string());
        }
        Ok(())
    }
    async fn recv(&mut self) -> Option<Result<String, IpcError>> {
        self.rx.recv().await.map(Ok)
    }
    fn peer_desc(&self) -> String {
        self.id.clone()
    }
    fn writer(&self) -> ConnectionWriter {
        let log = std::sync::Arc::clone(&self.log);
        ConnectionWriter::new(
            self.id.clone(),
            self.kind(),
            self.peer_desc(),
            Arc::new(move |msg: &str| {
                if let Ok(mut log) = log.lock() {
                    log.push(msg.to_string());
                }
                Ok(())
            }),
        )
    }
}

/// Connection-id → sent-message log shared between register_fake_conn and
/// drain_sent (the registry owns the boxed conn; logs live outside it).
static FAKE_LOGS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, SharedLog>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn register_fake_conn(server: &Arc<IpcServer>) -> String {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let id = format!(
        "fake-{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let (conn, log) = fake_conn(&id);
    if let Ok(mut reg) = server.registry.write() {
        reg.register(Box::new(conn));
    }
    if let Ok(mut logs) = FAKE_LOGS.lock() {
        logs.insert(id.clone(), log);
    }
    id
}

fn drain_sent(_server: &Arc<IpcServer>, id: &str) -> Vec<String> {
    FAKE_LOGS
        .lock()
        .ok()
        .and_then(|logs| logs.get(id).cloned())
        .map(|log| lock_or_recover(&log).clone())
        .unwrap_or_default()
}

/// Parsed `$/progress` frames a fake connection received (stream order).
fn progress_frames(server: &Arc<IpcServer>, id: &str) -> Vec<serde_json::Value> {
    drain_sent(server, id)
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .filter(|v| v["method"] == "$/progress")
        .collect()
}

fn extract_sid(resp: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(resp).unwrap();
    v["result"]["session_id"].as_str().unwrap().to_string()
}

/// `request_id` of a `$/permissionRequest` notification.
fn request_id(notification: &str) -> String {
    serde_json::from_str::<serde_json::Value>(notification).unwrap()["params"]["request_id"]
        .as_str()
        .expect("request_id")
        .to_string()
}

/// Wait until `predicate` matches a message the connection has sent.
async fn wait_for_line(log: &SharedLog, predicate: impl Fn(&str) -> bool) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(m) = agentj_core::sync::lock_or_recover(log)
                .iter()
                .find(|m| predicate(m))
            {
                return m.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed out waiting for a connection log line")
}

#[tokio::test]
async fn non_owner_permission_respond_is_rejected() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(move |p: &PermissionRequestParams, _sid: &str| {
        let _ = tx.send(p.request_id.clone());
    }));
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        let conn = a.clone();
        async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must record in-flight session");
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("permission request must arrive");
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
    );
    let resp = server.handle_request_from(&raw, &b).await.unwrap();
    assert!(
        resp.contains(r#""code":-32806"#),
        "non-owner respond must be NotAuthorized: {resp}"
    );
    let _ = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":10,"method":"$/cancelRequest","params":{"id":5}}"#,
            &a,
        )
        .await
        .unwrap();
    let _ = run_task.await;
}

#[tokio::test]
async fn concurrent_run_on_same_session_is_rejected() {
    // stub mock finishes instantly — the blocking fixture keeps run1 in
    // flight deterministically while run2 arrives.
    let (server, _module) = blocking_run_server();
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        let conn = a.clone();
        async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run1 must be in flight");
    let run2 = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"again"}}}}"#
    );
    let r2 = server.handle_request_from(&run2, &a).await.unwrap();
    assert!(
        r2.contains(r#""code":-32805"#),
        "second run must be SessionBusy: {r2}"
    );
    let _ = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":10,"method":"$/cancelRequest","params":{"id":5}}"#,
            &a,
        )
        .await
        .unwrap();
    let _ = run_task.await;
}

/// T23 fixture: accepts any `todo` args; the executor derives the snapshot
/// artifact itself (that production path is what T23 broadcasts).
struct TodoMock;

impl Tool for TodoMock {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "test todo"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        let count = input["items"].as_array().map(Vec::len).unwrap_or(0);
        Ok(serde_json::json!({"ok": true, "count": count}))
    }
}

/// T23 fixture: really writes the file so the executor's pre/post-read pair
/// yields a `file_change` artifact (op `add` for a fresh target).
struct WriteMock;

impl Tool for WriteMock {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "test write"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, agentj_core::error::ToolError> {
        let path = input["path"].as_str().unwrap_or("");
        let content = input["content"].as_str().unwrap_or("");
        std::fs::write(path, content).map_err(|e| {
            agentj_core::error::ToolError::ExecutionFailed {
                name: "write".into(),
                reason: e.to_string(),
            }
        })?;
        Ok(serde_json::json!({"ok": true}))
    }
}

#[test]
fn todo_event_to_protocol_value_roundtrip() {
    use agentj_core::artifacts::{TodoItemArtifact, TodoStatusArtifact};
    use agentj_protocol::streaming::{ProgressValue, TodoItem, TodoStatus};

    let wire = todo_items_into_wire(vec![
        TodoItemArtifact {
            id: "t1".into(),
            content: "first".into(),
            status: TodoStatusArtifact::Pending,
        },
        TodoItemArtifact {
            id: "t2".into(),
            content: "second".into(),
            status: TodoStatusArtifact::InProgress,
        },
        TodoItemArtifact {
            id: "t3".into(),
            content: "third".into(),
            status: TodoStatusArtifact::Completed,
        },
        TodoItemArtifact {
            id: "t4".into(),
            content: "fourth".into(),
            status: TodoStatusArtifact::Cancelled,
        },
    ]);
    assert_eq!(
        wire,
        vec![
            TodoItem {
                id: "t1".into(),
                content: "first".into(),
                status: TodoStatus::Pending,
            },
            TodoItem {
                id: "t2".into(),
                content: "second".into(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                id: "t3".into(),
                content: "third".into(),
                status: TodoStatus::Completed,
            },
            TodoItem {
                id: "t4".into(),
                content: "fourth".into(),
                status: TodoStatus::Cancelled,
            },
        ],
        "all four statuses map 1:1"
    );

    let value = ProgressValue::Todo {
        session_id: "s1".into(),
        run_id: None,
        revision: 7,
        items: wire.clone(),
    };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["kind"], "todo");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["revision"], 7);
    assert_eq!(json["items"][1]["status"], "in_progress");
    let back: ProgressValue = serde_json::from_value(json).unwrap();
    match back {
        ProgressValue::Todo {
            session_id,
            revision,
            items,
            ..
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(revision, 7);
            assert_eq!(items, wire, "items survive the wire roundtrip");
        }
        other => panic!("expected todo, got {other:?}"),
    }
}

#[test]
fn file_change_event_to_protocol_value_roundtrip() {
    use agentj_core::artifacts::{FileChangeArtifact, FileChangeOp};
    use agentj_protocol::streaming::{FileChangeKind, ProgressValue};

    let change = file_change_into_wire(FileChangeArtifact {
        path: "src/lib.rs".into(),
        op: FileChangeOp::Modify,
        additions: 3,
        deletions: 1,
        tool_call_id: Some("c1".into()),
        diff: Some("@@ -1,2 +1,2 @@\n-a\n+b\n".into()),
        diff_truncated: false,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(change.op, FileChangeKind::Modify);
    assert_eq!(change.diff_truncated, None, "false must stay omitted");
    let value = ProgressValue::FileChange {
        session_id: "s1".into(),
        run_id: None,
        change,
    };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["kind"], "file_change");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["change"]["path"], "src/lib.rs");
    assert_eq!(json["change"]["op"], "modify");
    assert_eq!(json["change"]["additions"], 3);
    assert_eq!(json["change"]["deletions"], 1);
    assert_eq!(json["change"]["tool_call_id"], "c1");
    assert!(
        json["change"]["diff"].as_str().unwrap().contains("+b"),
        "diff preserved: {json}"
    );
    assert!(
        json["change"].get("diff_truncated").is_none(),
        "false diff_truncated must be skipped: {json}"
    );
    let back: ProgressValue = serde_json::from_value(json).unwrap();
    match back {
        ProgressValue::FileChange {
            session_id, change, ..
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(change.op, FileChangeKind::Modify);
            assert_eq!(change.diff_truncated, None);
            assert_eq!(change.path, "src/lib.rs");
        }
        other => panic!("expected file_change, got {other:?}"),
    }

    let add = file_change_into_wire(FileChangeArtifact {
        path: "new.txt".into(),
        op: FileChangeOp::Add,
        additions: 1,
        deletions: 0,
        tool_call_id: None,
        diff: None,
        diff_truncated: false,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(add.op, FileChangeKind::Add);
    let add_json = serde_json::to_value(&add).unwrap();
    assert!(add_json.get("tool_call_id").is_none());
    assert!(add_json.get("diff").is_none());

    let delete = file_change_into_wire(FileChangeArtifact {
        path: "gone.txt".into(),
        op: FileChangeOp::Delete,
        additions: 0,
        deletions: 4,
        tool_call_id: Some("c9".into()),
        diff: None,
        diff_truncated: true,
        run_id: None,
        seq: None,
        source: None,
        total_bytes: None,
    });
    assert_eq!(delete.op, FileChangeKind::Delete);
    let delete_json = serde_json::to_value(&delete).unwrap();
    assert_eq!(delete_json["diff_truncated"], true, "truncation surfaced");
}

/// T5: `is_valid_run_id` requires a parseable UUID within the 128-char cap;
/// `sanitize_run_id` maps every reject (including `None`) to `None`.
#[test]
fn run_id_validators_accept_uuids_and_reject_malformed() {
    let valid = Uuid::new_v4().to_string();
    assert!(is_valid_run_id(&valid), "a real UUID is valid: {valid}");
    assert!(!is_valid_run_id("not-a-uuid"));
    assert!(
        !is_valid_run_id(&"x".repeat(129)),
        "over the 128-char cap is invalid"
    );
    assert_eq!(sanitize_run_id(None), None);
    assert_eq!(sanitize_run_id(Some("not-a-uuid".into())), None);
    assert_eq!(sanitize_run_id(Some(valid.clone())), Some(valid));
}

/// T23 AC: a run yielding todo + write artifacts must broadcast both
/// `$/progress` kinds to subscribers and record them once for replay.
#[tokio::test]
async fn subscriber_receives_todo_and_file_change_broadcast() {
    let dir = temp_dir_traj("t23-artifacts");
    let target = std::env::temp_dir().join(format!("aj-ipc-t23-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let mut server = scripted_run_server_with_tools(
        vec![
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_todo".into(),
                    name: "todo".into(),
                    arguments: serde_json::json!({"items": [
                        {"content": "write tests", "status": "in_progress"},
                        {"content": "implement", "status": "pending"}
                    ]}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_write".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::Done),
        ],
        |tools| {
            tools.register(Arc::new(TodoMock));
            tools.register(Arc::new(WriteMock));
        },
    );
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    server.handle_request_from(&run_raw, &a).await.unwrap();

    let frames: Vec<serde_json::Value> = drain_sent(&server, &b)
        .iter()
        .filter_map(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .filter(|v| v["method"] == "$/progress")
        .collect();
    let todo = frames
        .iter()
        .find(|f| f["params"]["value"]["kind"] == "todo")
        .unwrap_or_else(|| panic!("subscriber must receive a todo frame: {frames:?}"));
    assert_eq!(todo["params"]["token"], 3, "token echoes the request id");
    assert_eq!(todo["params"]["value"]["session_id"], sid);
    assert_eq!(todo["params"]["value"]["revision"], 1);
    assert_eq!(todo["params"]["value"]["items"][0]["status"], "in_progress");
    assert_eq!(todo["params"]["value"]["items"][1]["status"], "pending");

    let change = frames
        .iter()
        .find(|f| f["params"]["value"]["kind"] == "file_change")
        .unwrap_or_else(|| panic!("subscriber must receive a file_change frame: {frames:?}"));
    assert_eq!(change["params"]["token"], 3);
    assert_eq!(change["params"]["value"]["session_id"], sid);
    assert_eq!(change["params"]["value"]["change"]["op"], "add");
    assert_eq!(change["params"]["value"]["change"]["path"], path);
    assert_eq!(change["params"]["value"]["change"]["additions"], 2);
    assert_eq!(
        change["params"]["value"]["change"]["tool_call_id"],
        "call_write"
    );
    assert!(
        change["params"]["value"]["change"]["diff"]
            .as_str()
            .unwrap()
            .contains("+b"),
        "diff preserved: {change}"
    );
    assert!(
        change["params"]["value"]["change"]
            .get("diff_truncated")
            .is_none(),
        "false diff_truncated stays omitted: {change}"
    );

    let events = read_traj_events(&dir, &sid);
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "tool_result",
            "todo",
            "tool_call",
            "tool_result",
            "file_change",
            "run_model"
        ],
        "each artifact is recorded exactly once, in stream order"
    );
    let agentj_protocol::trajectory::TrajectoryValue::Todo {
        revision, items, ..
    } = &events[3].value
    else {
        panic!("todo node at index 3: {events:?}")
    };
    assert_eq!(*revision, 1);
    assert_eq!(items[0].content, "write tests");
    assert_eq!(
        items[1].status,
        agentj_protocol::streaming::TodoStatus::Pending
    );
    let agentj_protocol::trajectory::TrajectoryValue::FileChange { change, .. } = &events[6].value
    else {
        panic!("file_change node at index 6: {events:?}")
    };
    assert_eq!(change.op, agentj_protocol::streaming::FileChangeKind::Add);
    assert_eq!(change.path, path);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// T5 AC: a live run broadcasts Todo/FileChange frames tagged with the same
/// `run_id` as `SessionRunResult`, plus per-run `seq`/`source`.
#[tokio::test]
async fn broadcast_includes_run_id() {
    let dir = temp_dir_traj("run-id-broadcast");
    let target = std::env::temp_dir().join(format!("aj-ipc-runid-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let mut server = scripted_run_server_with_tools(
        vec![
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_todo".into(),
                    name: "todo".into(),
                    arguments: serde_json::json!({"items": [
                        {"content": "write tests", "status": "in_progress"}
                    ]}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::ToolCall(
                agentj_core::types::ToolCall {
                    id: "call_write".into(),
                    name: "write".into(),
                    arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
                },
            )),
            Ok(agentj_core::types::ModelEvent::Done),
        ],
        |tools| {
            tools.register(Arc::new(TodoMock));
            tools.register(Arc::new(WriteMock));
        },
    );
    server.trajectory_store = Some(wired_store(&dir));
    let server = Arc::new(server);
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_resp = server.handle_request_from(&run_raw, &a).await.unwrap();
    let run_v: serde_json::Value = serde_json::from_str(&run_resp).unwrap();
    let run_id = run_v["result"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("SessionRunResult must carry run_id: {run_resp}"))
        .to_string();
    assert!(Uuid::parse_str(&run_id).is_ok(), "real UUID: {run_id}");

    let frames = progress_frames(&server, &b);
    let todos: Vec<_> = frames
        .iter()
        .filter(|f| f["params"]["value"]["kind"] == "todo")
        .collect();
    assert!(!todos.is_empty(), "todo frame expected: {frames:?}");
    assert!(
        todos
            .iter()
            .all(|f| f["params"]["value"]["run_id"].as_str() == Some(run_id.as_str())),
        "every todo frame carries the run id: {todos:?}"
    );
    let changes: Vec<_> = frames
        .iter()
        .filter(|f| f["params"]["value"]["kind"] == "file_change")
        .collect();
    assert!(
        !changes.is_empty(),
        "file_change frame expected: {frames:?}"
    );
    for c in &changes {
        let change = &c["params"]["value"]["change"];
        assert_eq!(
            c["params"]["value"]["run_id"].as_str(),
            Some(run_id.as_str()),
            "outer frame run_id: {c}"
        );
        assert_eq!(
            change["run_id"].as_str(),
            Some(run_id.as_str()),
            "wire change run_id: {c}"
        );
        assert_eq!(change["seq"], 1, "first change of the run: {c}");
        assert_eq!(change["source"], "write", "write-sourced change: {c}");
    }
    let recorded = read_traj_events(&dir, &sid);
    let todo_run = recorded.iter().find_map(|e| match &e.value {
        agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. } => Some(run_id.clone()),
        _ => None,
    });
    assert_eq!(
        todo_run.flatten().as_deref(),
        Some(run_id.as_str()),
        "replay node shares the broadcast run id"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// BUG-004 regression: §7.2 — when the client omits `progress_token`, the
/// `$/progress` `params.token` must echo the request id, never null.
#[tokio::test]
async fn progress_broadcast_uses_request_id_token_when_token_omitted() {
    let server = Arc::new(stub_bus_server());
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/subscribe","params":{{"session_id":"{sid}"}}}}"#
            ),
            &b,
        )
        .await
        .unwrap();
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
            &a,
        )
        .await
        .unwrap();
    let b_sent = drain_sent(&server, &b);
    let progress = b_sent
        .iter()
        .find(|m| m.contains("$/progress"))
        .unwrap_or_else(|| panic!("subscriber must receive $/progress: {b_sent:?}"));
    let v: serde_json::Value = serde_json::from_str(progress).unwrap();
    assert!(
        !v["params"]["token"].is_null(),
        "token must never be null: {progress}"
    );
    assert_eq!(
        v["params"]["token"], 2,
        "§7.2: omitted progress_token must fall back to the request id: {progress}"
    );
}

/// BUG-004 edge: a `session/run` notification (no request id) still
/// broadcasts `$/progress`; the token falls back to a deterministic
/// schema-valid number instead of null.
#[tokio::test]
async fn progress_broadcast_notification_without_id_keeps_token_valid() {
    let server = Arc::new(stub_bus_server());
    let a = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    server
        .handle_request_from(
            &format!(
                r#"{{"jsonrpc":"2.0","method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
            ),
            &a,
        )
        .await
        .unwrap();
    let a_sent = drain_sent(&server, &a);
    let progress = a_sent
        .iter()
        .find(|m| m.contains("$/progress"))
        .unwrap_or_else(|| panic!("initiator must receive $/progress: {a_sent:?}"));
    let v: serde_json::Value = serde_json::from_str(progress).unwrap();
    assert!(
        v["params"]["token"].is_i64() || v["params"]["token"].is_string(),
        "token must stay ProgressToken (integer|string), got: {progress}"
    );
}

#[tokio::test]
async fn permission_request_is_routed_to_owner_only() {
    let (server, _module) = blocking_run_server();
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let a = register_fake_conn(&server);
    let b = register_fake_conn(&server);
    let new_resp = server
        .handle_request_from(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#, &a)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run_raw = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    );
    let run_task = tokio::spawn({
        let s = Arc::clone(&server);
        let conn = a.clone();
        async move { s.handle_request_from(&run_raw, &conn).await.unwrap() }
    });
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in flight");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let a_sent = drain_sent(&server, &a);
    let b_sent = drain_sent(&server, &b);
    assert!(
        a_sent
            .iter()
            .any(|m| m.contains("$/permissionRequest") && m.contains("\"origin_client\":\"")),
        "owner A must receive $/permissionRequest with origin_client: {a_sent:?}"
    );
    assert!(
        !b_sent.iter().any(|m| m.contains("$/permissionRequest")),
        "non-owner B must NOT receive $/permissionRequest: {b_sent:?}"
    );
    let _ = server
        .handle_request_from(
            r#"{"jsonrpc":"2.0","id":9,"method":"$/cancelRequest","params":{"id":5}}"#,
            &a,
        )
        .await
        .unwrap();
    let _ = run_task.await;
}

/// BUG-004 (backend half): the owner is the only connection allowed to answer an
/// approval (设计决策 #5), and it is the in-flight `session/run` that waits on it.
/// So the approval must be settled the moment the connection ends — not after
/// `serve_connection_requests` drains that very run, which would leave an
/// orphaned run blocked until the approval timeout.
#[tokio::test]
async fn serve_connection_end_of_input_denies_pending_approval() {
    let (server, module) = blocking_run_server();
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let (conn, log, tx) = scripted_conn("tcp-A");
    let s = Arc::clone(&server);
    let serving = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });

    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |l| l.contains("session_id")).await;
    let sid = extract_sid(&new_resp);
    tx.send(format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    ))
    .await
    .unwrap();
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in flight");
    let approval = wait_for_line(&log, |l| l.contains("$/permissionRequest")).await;
    let rid = request_id(&approval);

    drop(tx); // EOF — the client goes away while the run waits for approval

    // 1) the approval is settled by the disconnect, not by the 10s timeout
    let settled = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while module.view(&rid).is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        settled.is_ok(),
        "owner disconnect must settle the pending approval at once"
    );
    // 2) and the orphaned run is released, so a reconnect can run again
    let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while read_or_recover(&server.registry)
            .pending_runs
            .contains(&sid)
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        released.is_ok(),
        "the run must be released at disconnect, not by the approval timeout"
    );
    let finished = tokio::time::timeout(std::time::Duration::from_secs(5), serving).await;
    assert!(
        finished.is_ok(),
        "serve_connection must return once the orphaned run is released"
    );
    finished.unwrap().unwrap().unwrap();
    let log_dump = lock_or_recover(&log).clone();
    assert!(
        log_dump.iter().any(|l| l.contains("\"id\":2")),
        "the released run must still answer its caller: {log_dump:?}"
    );
}

/// BUG-004 (backend half): the disconnect must also cover approvals raised *while*
/// the run is being drained. The mock script emits two `bash` calls in one model
/// round, so once the first approval is settled at EOF the run immediately asks for
/// a second one — with no owner left it must be denied, not routed to the dead
/// connection (which would stall the run for the whole approval timeout and keep
/// the session busy after a reconnect).
#[tokio::test]
async fn approvals_during_drain_are_denied_at_once() {
    let (server, module) = blocking_run_server_with(2);
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let (conn, log, tx) = scripted_conn("tcp-A");
    let s = Arc::clone(&server);
    let serving = tokio::spawn(async move { s.serve_connection(Box::new(conn)).await });

    tx.send(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#.to_string())
        .await
        .unwrap();
    let new_resp = wait_for_line(&log, |l| l.contains("session_id")).await;
    let sid = extract_sid(&new_resp);
    tx.send(format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":"hi"}}}}"#
    ))
    .await
    .unwrap();
    wait_for_pending_run(&server, &sid)
        .await
        .expect("run must be in flight");
    let approval = wait_for_line(&log, |l| l.contains("$/permissionRequest")).await;
    let rid = request_id(&approval);

    drop(tx); // EOF while the run waits on the first of the two approvals

    // The run can only answer `id:2` once it is done, i.e. once BOTH approvals are
    // settled — and `wait_for_line` gives up after 5s, far below the fixture's 10s
    // approval timeout, so a stall shows up as a failed wait.
    let done = wait_for_line(&log, |l| l.contains("\"id\":2")).await;
    assert!(
        module.view(&rid).is_none(),
        "first approval must be settled"
    );
    assert!(
        !done.contains("\"error\""),
        "the orphaned run must finish normally: {done}"
    );
    let released = tokio::time::timeout(std::time::Duration::from_secs(5), serving).await;
    assert!(released.is_ok(), "serve_connection must return");
    released.unwrap().unwrap().unwrap();
}

/// BUG-004 (backend half): an approval that cannot reach its owner can never be
/// answered, so it must be denied at once instead of stalling the run for the
/// whole approval timeout.
#[tokio::test]
async fn approval_without_reachable_owner_is_denied_immediately() {
    let (server, module) = permission_server_with(std::time::Duration::from_secs(30));
    let server = Arc::new(server);
    server.attach_permission_notifier();
    let ctx = PermissionContext {
        session_id: "s1".into(),
        tool: "bash".into(),
        source: ToolSource::Builtin,
        args: serde_json::json!({"command": "curl x"}),
        workspace_root: std::path::PathBuf::from("/ws"),
    };
    let PermissionDecision::Ask { request_id } = module.check(&ctx) else {
        panic!("expected Ask");
    };
    let decision = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        module.await_approval(&request_id),
    )
    .await
    .expect("no owner must be settled at once, not after the 30s timeout");
    // A deny is the only possible outcome — and it must not be the timeout one.
    // (`register_ask` invokes the notifier synchronously, so the request is
    // already consumed by the time `await_approval` looks it up: whatever the
    // settling caller, that lookup reports a plain deny rather than a stall.)
    let PermissionDecision::Deny { reason } = &decision else {
        panic!("no reachable owner must deny at once, got {decision:?}");
    };
    assert_ne!(
        reason, "timeout",
        "no reachable owner must deny at once, not time out"
    );
    assert!(
        module.view(&request_id).is_none(),
        "the approval must be settled, not left pending"
    );
}

#[derive(Debug)]
struct CfgEmpty;
impl agentj_core::module::ConfigProvider for CfgEmpty {
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

#[derive(Debug)]
struct CfgModel;
impl agentj_core::module::ConfigProvider for CfgModel {
    fn get_str(&self, k: &str) -> Option<String> {
        (k == "model.default_model").then(|| "deepseek-chat".into())
    }
    fn get_int(&self, _k: &str) -> Option<i64> {
        None
    }
    fn providers(&self) -> Vec<(String, String)> {
        vec![("mock".into(), "http://localhost:11434".into())]
    }
}

#[test]
fn kernel_model_info_empty_when_no_context_or_config() {
    let (dm, ps) = kernel_model_info(None);
    assert!(dm.is_none());
    assert!(ps.is_none());
    let (dm2, ps2) = kernel_model_info(Some(&agentj_core::module::KernelContext {
        session: stub_session(),
        tools: Arc::new(agentj_core::module::ToolRegistry::new()),
        providers: Arc::new(agentj_core::module::ProviderRegistry::new()),
        config: Arc::new(CfgEmpty),
    }));
    assert!(dm2.is_none());
    assert!(ps2.is_none());
}

#[derive(Debug)]
struct StubModelsProvider(Vec<String>);
#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for StubModelsProvider {
    async fn chat_stream(
        &self,
        _m: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        unimplemented!()
    }
    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<agentj_core::types::ChatResponse, agentj_core::error::ModelError> {
        unimplemented!()
    }
    fn models(&self) -> Vec<String> {
        self.0.clone()
    }
}

#[test]
fn kernel_model_info_surfaces_registered_provider_models() {
    let registry = agentj_core::module::ProviderRegistry::new();
    registry.register(
        "opencode-go".into(),
        Arc::new(StubModelsProvider(vec!["m1".into()])),
    );
    let (default_model, providers) = kernel_model_info(Some(&agentj_core::module::KernelContext {
        session: stub_session(),
        tools: Arc::new(agentj_core::module::ToolRegistry::new()),
        providers: Arc::new(registry),
        config: Arc::new(CfgModel),
    }));
    assert_eq!(default_model.as_deref(), Some("deepseek-chat"));
    let ps = providers.expect("providers present when a provider is registered");
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].id, "opencode-go");
    assert_eq!(ps[0].models, vec!["m1".to_string()]);
}

#[test]
fn kernel_model_info_reports_empty_models_for_silent_provider() {
    let registry = agentj_core::module::ProviderRegistry::new();
    registry.register("silent".into(), Arc::new(StubModelsProvider(vec![])));
    let (_, providers) = kernel_model_info(Some(&agentj_core::module::KernelContext {
        session: stub_session(),
        tools: Arc::new(agentj_core::module::ToolRegistry::new()),
        providers: Arc::new(registry),
        config: Arc::new(CfgModel),
    }));
    let ps = providers.expect("providers present when a provider is registered");
    assert!(ps[0].models.is_empty());
}

#[test]
fn kernel_model_info_reflects_registered_not_config_listed_providers() {
    // Config lists a provider but the registry is empty: kernel/status must report none.
    let (_, providers) = kernel_model_info(Some(&agentj_core::module::KernelContext {
        session: stub_session(),
        tools: Arc::new(agentj_core::module::ToolRegistry::new()),
        providers: Arc::new(agentj_core::module::ProviderRegistry::new()),
        config: Arc::new(CfgModel),
    }));
    assert!(providers.is_none());
}

fn temp_dir_traj(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-traj-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wired_store(dir: &std::path::Path) -> Arc<crate::trajectory::TrajectoryStore> {
    Arc::new(crate::trajectory::TrajectoryStore::new(
        dir.to_path_buf(),
        None,
        None,
    ))
}

fn read_traj_events(
    dir: &std::path::Path,
    sid: &str,
) -> Vec<agentj_protocol::trajectory::TrajectoryEvent> {
    let path = dir.join(format!("{sid}.trajectory"));
    if !path.exists() {
        return vec![];
    }
    let f = std::fs::File::open(path).unwrap();
    std::io::BufRead::lines(std::io::BufReader::new(f))
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect()
}

fn traj_kinds(events: &[agentj_protocol::trajectory::TrajectoryEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(&e.value).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// T5 fixture: serves `script` on the first `chat_stream` of each run and a
/// bare `Done` afterwards, keyed by the run's user content, so two
/// sequential runs on one session each replay one tool batch.
struct TwoRunMockProvider {
    script: Vec<ModelEvent>,
    first_run_calls: std::sync::atomic::AtomicUsize,
    second_run_calls: std::sync::atomic::AtomicUsize,
}

impl TwoRunMockProvider {
    fn new(script: Vec<ModelEvent>) -> Self {
        Self {
            script,
            first_run_calls: std::sync::atomic::AtomicUsize::new(0),
            second_run_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl agentj_core::module::ModelProvider for TwoRunMockProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        _t: Vec<ToolDefinition>,
        _o: ModelOptions,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<ModelEvent, agentj_core::error::ModelError>>,
        agentj_core::error::ModelError,
    > {
        let run_calls = if messages.iter().any(|m| m.content.contains("second-turn")) {
            &self.second_run_calls
        } else {
            &self.first_run_calls
        };
        let n = run_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events: Vec<Result<ModelEvent, agentj_core::error::ModelError>> = if n == 0 {
            self.script.iter().cloned().map(Ok).collect()
        } else {
            vec![Ok(ModelEvent::Done)]
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn chat(
        &self,
        _m: Vec<ChatMessage>,
        _o: ModelOptions,
    ) -> Result<ChatResponse, agentj_core::error::ModelError> {
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![],
        })
    }
}

fn scripted_run_server(
    script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
) -> IpcServer {
    scripted_run_server_with_tools(script, |_| {})
}

/// Common test-kernel wiring: ToolExecutor + AgentLoop over `provider`.
fn run_server_with_provider(
    provider: Arc<dyn agentj_core::module::ModelProvider>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let session_dyn: Arc<dyn SessionStore> = store.clone();
    let tools = Arc::new(ToolRegistry::new());
    register_tools(&tools);
    let providers = Arc::new(ProviderRegistry::new());
    providers.register("mock".into(), provider);
    let ctx = KernelContext {
        session: session_dyn.clone(),
        tools: tools.clone(),
        providers: providers.clone(),
        config: Arc::new(MockConfig),
    };
    let mut kernel = Kernel::new();
    let mut register =
        ModuleRegister::with_services(tools.clone(), providers.clone(), kernel.services());
    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut register,
        )
        .unwrap();
    let runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();
    IpcServer::new(kernel, session_dyn)
}

/// T23: like `scripted_run_server`, but lets the caller register test tools so
/// tool artifacts (todo / write) flow as real `AgentEvent`s through the bus.
fn scripted_run_server_with_tools(
    script: Vec<Result<agentj_core::types::ModelEvent, agentj_core::error::ModelError>>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    run_server_with_provider(Arc::new(MockModelProvider::new(script)), register_tools)
}

fn two_run_server(
    script: Vec<agentj_core::types::ModelEvent>,
    register_tools: impl FnOnce(&ToolRegistry),
) -> IpcServer {
    run_server_with_provider(Arc::new(TwoRunMockProvider::new(script)), register_tools)
}

#[tokio::test]
async fn run_records_user_thinking_usage_and_keeps_response_shape() {
    let dir = temp_dir_traj("usage");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"], "hello");
    assert_eq!(v["result"]["usage"]["prompt_tokens"], 100);
    assert_eq!(v["result"]["usage"]["completion_tokens"], 20);
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "thinking", "usage", "run_model"]
    );
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[1].seq, 2);
    assert_eq!(events[2].seq, 3);
    let agentj_protocol::trajectory::TrajectoryValue::User { content } = &events[0].value else {
        panic!()
    };
    assert_eq!(content, "hi");
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!()
    };
    assert_eq!(content, "hello");
    assert_eq!(events[2].session_id, "s1");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_records_one_merged_thinking_node_for_two_deltas() {
    let dir = temp_dir_traj("merge");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("hel".into())),
        Ok(agentj_core::types::ModelEvent::Delta("lo".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "thinking", "run_model"],
        "exactly two nodes"
    );
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!("expected one merged thinking node, got {events:?}")
    };
    assert_eq!(content, "hello", "both deltas concatenated into ONE node");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_drops_zero_width_only_deltas_instead_of_recording_blank_thinking() {
    let dir = temp_dir_traj("zwsp");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("\u{200b}".into())),
        Ok(agentj_core::types::ModelEvent::Delta(" \n".into())),
        Ok(agentj_core::types::ModelEvent::Delta("\u{feff}".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "run_model"],
        "no blank thinking node"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_keeps_thinking_that_contains_visible_glyphs() {
    let dir = temp_dir_traj("zwsp-visible");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("\u{200b}".into())),
        Ok(agentj_core::types::ModelEvent::Delta("ok".into())),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["user", "thinking", "run_model"]);
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!("expected a thinking node, got {events:?}")
    };
    assert_eq!(content, "\u{200b}ok");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_visually_blank_covers_zero_width_and_whitespace_only_text() {
    for blank in [
        "",
        " ",
        "\n\t ",
        "\u{200b}",
        "\u{200b}\u{feff}",
        " \u{200b}\n",
    ] {
        assert!(IpcServer::is_visually_blank(blank), "{blank:?}");
    }
    for visible in ["a", "0", "-", "\u{200b}ok", "分析"] {
        assert!(!IpcServer::is_visually_blank(visible), "{visible:?}");
    }
}

#[tokio::test]
async fn run_records_tool_call_and_tool_result_nodes() {
    let dir = temp_dir_traj("tools");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::ToolCall(
            agentj_core::types::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "curl x"}),
            },
        )),
        Ok(agentj_core::types::ModelEvent::Done),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(resp.contains("\"result\""), "{resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec!["user", "tool_call", "tool_result", "run_model"]
    );
    let agentj_protocol::trajectory::TrajectoryValue::ToolCall { tool_call } = &events[1].value
    else {
        panic!("tool_call node at index 1: {events:?}")
    };
    assert_eq!(tool_call.id, "call_1");
    assert_eq!(tool_call.name, "bash");
    assert_eq!(tool_call.arguments["command"], "curl x");
    let agentj_protocol::trajectory::TrajectoryValue::ToolResult {
        tool_call_id,
        content,
    } = &events[2].value
    else {
        panic!("tool_result node at index 2: {events:?}")
    };
    assert_eq!(tool_call_id, "call_1");
    assert!(
        content.starts_with("Error:"),
        "unregistered tool surfaces as executor error CONTENT (C9: content is a plain string): {content}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_records_error_node_on_provider_failure() {
    let dir = temp_dir_traj("boom");
    let mut server = scripted_run_server(vec![
        Ok(agentj_core::types::ModelEvent::Delta("x".into())),
        Err(agentj_core::error::ModelError::Network("boom".into())),
    ]);
    server.trajectory_store = Some(wired_store(&dir));
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    assert!(
        resp.contains("-32603"),
        "internal error, not cancel: {resp}"
    );
    let events = read_traj_events(&dir, "s1");
    assert_eq!(traj_kinds(&events), vec!["user", "thinking", "error"]);
    let agentj_protocol::trajectory::TrajectoryValue::Thinking { content } = &events[1].value
    else {
        panic!()
    };
    assert_eq!(content, "x");
    let agentj_protocol::trajectory::TrajectoryValue::Error { message } = &events[2].value else {
        panic!()
    };
    assert!(
        message.contains("boom"),
        "error node carries the failure: {message}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn cancelled_run_trajectory_has_no_error_or_tail_nodes() {
    let dir = temp_dir_traj("cancel");
    let (mut server, _module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
                .await
                .unwrap()
        }
    });
    wait_for_pending_run(&server, "s1")
        .await
        .expect("run must be in-flight");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = server
        .handle_request(
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":5,"session_id":"s1"}}"#,
        )
        .await
        .unwrap();
    let resp = run.await.unwrap();
    // Cancel should surface -32800; allow a brief retry if race produced tool_result instead.
    if !resp.contains("-32800") {
        // If we got a normal result, the permission deny path won; still verify trajectory
        // has no error/usage nodes and at least user+tool_call.
        let events = read_traj_events(&dir, "s1");
        assert!(
            traj_kinds(&events).starts_with(&["user".to_string(), "tool_call".to_string()]),
            "cancelled run must at least have user+tool_call: {events:?}"
        );
        assert!(
            !traj_kinds(&events).contains(&"error".to_string()),
            "cancelled run must not have error node: {events:?}"
        );
        assert!(
            !traj_kinds(&events).contains(&"usage".to_string()),
            "cancelled run must not have usage node: {events:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
        return;
    }
    let events = read_traj_events(&dir, "s1");
    // Strict expectation when -32800 wins: only user and tool_call, no tails.
    // Allow an extra tool_result with cancelled content if permission path raced,
    // but never error/usage.
    let kinds = traj_kinds(&events);
    assert!(
        kinds == vec!["user", "tool_call"] || kinds == vec!["user", "tool_call", "tool_result"],
        "tool_call was emitted at request-send (pre-Ask); the cancel discards pending thinking and records NO error/usage/permission tail: {events:?}"
    );
    if kinds.len() == 3 {
        let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } =
            &events[2].value
        else {
            panic!("third node must be tool_result: {events:?}");
        };
        assert!(
            content.contains("cancelled"),
            "extra tool_result must be cancelled deny: {content}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn respond_allow_records_permission_node_between_call_and_result() {
    let dir = temp_dir_traj("settle");
    let (mut server, module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(
        move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
            let _ = ask_tx.send(p.request_id.clone());
        },
    ));
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                .await
                .unwrap()
        }
    });
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = ask_rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the run's bash execution must register an Ask");
    let respond = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"allow"}}}}"#
    );
    let resp = server.handle_request(&respond).await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "owner respond accepted: {resp}"
    );
    let resp = run.await.unwrap();
    assert!(resp.contains("\"result\""), "run completed: {resp}");
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "permission",
            "tool_result",
            "run_model"
        ]
    );
    let agentj_protocol::trajectory::TrajectoryValue::Permission {
        request_id,
        tool,
        decision,
    } = &events[2].value
    else {
        panic!("permission node at index 2: {events:?}")
    };
    assert_eq!(request_id, &rid);
    assert_eq!(tool, "bash");
    assert_eq!(
        *decision,
        agentj_protocol::permission::PermissionDecision::Allow
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn respond_deny_records_deny_and_run_still_completes() {
    let dir = temp_dir_traj("deny");
    let (mut server, module) = blocking_run_server();
    server.trajectory_store = Some(wired_store(&dir));
    write_or_recover(&server.registry).set_session_owner(&"s1".to_string(), "stdio");
    let server = Arc::new(server);
    let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    module.set_notifier(Arc::new(
        move |p: &agentj_protocol::permission::PermissionRequestParams, _sid: &str| {
            let _ = ask_tx.send(p.request_id.clone());
        },
    ));
    let run = tokio::spawn({
        let s = Arc::clone(&server);
        async move {
            s.handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"run it"}}"#)
                .await
                .unwrap()
        }
    });
    let rid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(r) = ask_rx.try_recv() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the run's bash execution must register an Ask");
    let respond = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"permission/respond","params":{{"request_id":"{rid}","decision":"deny"}}}}"#
    );
    let resp = server.handle_request(&respond).await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "owner respond accepted: {resp}"
    );
    let resp = run.await.unwrap();
    assert!(
        resp.contains("\"result\""),
        "denied tool is a content error, not a run error: {resp}"
    );
    let events = read_traj_events(&dir, "s1");
    assert_eq!(
        traj_kinds(&events),
        vec![
            "user",
            "tool_call",
            "permission",
            "tool_result",
            "run_model"
        ]
    );
    let agentj_protocol::trajectory::TrajectoryValue::Permission { decision, .. } =
        &events[2].value
    else {
        panic!("permission node at index 2: {events:?}")
    };
    assert_eq!(
        *decision,
        agentj_protocol::permission::PermissionDecision::Deny
    );
    let agentj_protocol::trajectory::TrajectoryValue::ToolResult { content, .. } = &events[3].value
    else {
        panic!("tool_result at index 3: {events:?}")
    };
    assert!(
        content.starts_with("Error:"),
        "executor denial flows into tool_result content (C9): {content}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_list_returns_file_backed_rows() {
    let dir = temp_dir_traj("list");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let session: Arc<dyn SessionStore> = store.clone();
    let server = IpcServer::new(Kernel::new(), session);
    let sid = store.create().unwrap();
    store
        .append(
            &sid,
            ChatMessage {
                role: Role::User,
                content: "x".repeat(150),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    let raw = r#"{"jsonrpc":"2.0","id":4,"method":"session/list"}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let row = &v["result"]["sessions"][0];
    assert_eq!(row["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(row["state"], "active");
    assert!(row["updated_at"].is_u64());
    assert_eq!(row["preview"].as_str().unwrap().len(), 120);
    assert!(row["preview"].as_str().unwrap().starts_with("xxxxx"));
    store.archive(&sid).unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":5,"method":"session/list"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["sessions"][0]["state"], "archived");
    std::fs::remove_dir_all(&dir).ok();
}

/// Regression guard: `session/status` is declared in the registry, schema,
/// SDK and spec (§8.8) and is the existence probe every client fires
/// before `session/subscribe`. It must be dispatched — falling through to
/// `-32601 method not found` breaks every session click in Web/TUI.
#[tokio::test]
async fn session_status_returns_store_status() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/status must be implemented, not -32601: {resp}"
    );
    assert_eq!(v["result"]["status"], "active");
    store.archive(&sid).unwrap();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "archived");
}

/// T14: `session/status` reports the effective project root — the stored
/// canonicalized root when present, else the canonicalized cwd fallback.
#[tokio::test]
async fn dispatch_session_status_reports_effective_project_root() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let dir = temp_dir_root("status-eff-root");
    let canonical = dir.canonicalize().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "session/new",
        "params": { "project_root": dir.to_str().unwrap() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let sid = v["result"]["session_id"].as_str().unwrap().to_string();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "session/status",
        "params": { "session_id": sid },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["result"]["project_root"],
        canonical.to_str().unwrap(),
        "status must report the stored canonicalized root"
    );

    let bare = store.create().unwrap();
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "session/status",
        "params": { "session_id": bare.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(
        v["result"]["project_root"],
        cwd.to_str().unwrap(),
        "status without a stored root must fall back to the canonicalized cwd"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Unknown session id maps to the documented `-32802 SessionNotFound`
/// with `error.data.session_id` (§8.8), never the generic `-32601`.
#[tokio::test]
async fn session_status_unknown_session_returns_32802() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let raw =
        r#"{"jsonrpc":"2.0","id":13,"method":"session/status","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32802,
        "unknown session must be SessionNotFound: {resp}"
    );
    assert_eq!(v["error"]["data"]["session_id"], "nope");
}

/// Regression guard: `session/archive` is declared in the registry/spec
/// §8.9 and exported by the TUI session hook; it must be dispatched.
/// Only active sessions may be archived — repeats are `-32803`.
#[tokio::test]
async fn session_archive_transitions_and_rejects_inactive() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let archive = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 14,
        "method": "session/archive",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&archive).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/archive must be implemented, not -32601: {resp}"
    );
    assert!(v["result"].is_null(), "archive result is null: {resp}");
    let status = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 15,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&status).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "archived");
    let resp = server.handle_request(&archive).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32803,
        "repeat archive must be SessionInactive: {resp}"
    );
    assert_eq!(v["error"]["data"]["status"], "archived");
    let unknown =
        r#"{"jsonrpc":"2.0","id":16,"method":"session/archive","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(unknown).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32802, "unknown archive: {resp}");
}

/// Regression guard: `session/delete` (§8.10) must be dispatched;
/// deleting a deleted session is `-32803 SessionInactive`.
#[tokio::test]
async fn session_delete_transitions_and_rejects_deleted() {
    let store = stub_session();
    let server = IpcServer::new(Kernel::new(), store.clone());
    let sid = store.create().unwrap();
    let delete = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 17,
        "method": "session/delete",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&delete).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v["error"].is_null(),
        "session/delete must be implemented, not -32601: {resp}"
    );
    assert!(v["result"].is_null(), "delete result is null: {resp}");
    let status = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 18,
        "method": "session/status",
        "params": { "session_id": sid.as_str() },
    })
    .to_string();
    let resp = server.handle_request(&status).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["status"], "deleted");
    let resp = server.handle_request(&delete).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(
        v["error"]["code"], -32803,
        "repeat delete must be SessionInactive: {resp}"
    );
    assert_eq!(v["error"]["data"]["status"], "deleted");
    let unknown =
        r#"{"jsonrpc":"2.0","id":19,"method":"session/delete","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(unknown).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32802, "unknown delete: {resp}");
}

#[tokio::test]
async fn session_history_pages_recorded_events() {
    let dir = temp_dir_traj("history");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let raw = r#"{"jsonrpc":"2.0","id":6,"method":"session/history","params":{"session_id":"s1","limit":2}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let page1 = &v["result"];
    assert_eq!(page1["events"].as_array().unwrap().len(), 2);
    assert_eq!(page1["events"][0]["seq"], 1);
    assert_eq!(page1["events"][0]["value"]["kind"], "user");
    assert_eq!(page1["events"][1]["value"]["kind"], "thinking");
    assert_eq!(page1["next_cursor"], 3);
    let raw2 = r#"{"jsonrpc":"2.0","id":7,"method":"session/history","params":{"session_id":"s1","cursor":3,"limit":3}}"#;
    let resp2 = server.handle_request(raw2).await.unwrap();
    let v2: serde_json::Value = serde_json::from_str(&resp2).unwrap();
    assert_eq!(v2["result"]["events"][0]["value"]["kind"], "usage");
    assert_eq!(v2["result"]["events"][1]["value"]["kind"], "run_model");
    assert!(v2["result"].get("next_cursor").is_none(), "no next page");
    std::fs::remove_dir_all(&dir).ok();
}

/// Dispatch one `session/run` and return its `SessionRunResult.run_id`.
async fn run_and_run_id(server: &IpcServer, sid: &str, content: &str, id: u64) -> String {
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"session/run","params":{{"session_id":"{sid}","content":"{content}"}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    v["result"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("run must report a run_id: {resp}"))
        .to_string()
}

/// Mirror of the T5 run filter: Todo/FileChange keep only `run_id`, all
/// other trajectory kinds pass.
fn event_kept_for_run(value: &agentj_protocol::trajectory::TrajectoryValue, run_id: &str) -> bool {
    match value {
        agentj_protocol::trajectory::TrajectoryValue::Todo { run_id: r, .. }
        | agentj_protocol::trajectory::TrajectoryValue::FileChange { run_id: r, .. } => {
            r.as_deref() == Some(run_id)
        }
        _ => true,
    }
}

/// T5 AC: `session/history {run_id}` keeps that run's Todo/FileChange, lets
/// every other kind through, and filters before paginating.
#[tokio::test]
async fn history_filters_by_run_id() {
    let dir = temp_dir_traj("history-run-filter");
    let target = std::env::temp_dir().join(format!("aj-ipc-runfilter-{}.txt", std::process::id()));
    std::fs::remove_file(&target).ok();
    let path = target.to_string_lossy().into_owned();
    let script = vec![
        agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
            id: "call_todo".into(),
            name: "todo".into(),
            arguments: serde_json::json!({"items": [
                {"content": "step", "status": "in_progress"}
            ]}),
        }),
        agentj_core::types::ModelEvent::ToolCall(agentj_core::types::ToolCall {
            id: "call_write".into(),
            name: "write".into(),
            arguments: serde_json::json!({"path": path, "content": "a\nb\n"}),
        }),
        agentj_core::types::ModelEvent::Done,
    ];
    let mut server = two_run_server(script, |tools| {
        tools.register(Arc::new(TodoMock));
        tools.register(Arc::new(WriteMock));
    });
    server.trajectory_store = Some(wired_store(&dir));
    let new_resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    let sid = extract_sid(&new_resp);
    let run1 = run_and_run_id(&server, &sid, "first-turn", 2).await;
    std::fs::remove_file(&target).ok();
    let run2 = run_and_run_id(&server, &sid, "second-turn", 3).await;
    assert_ne!(run1, run2, "each run gets a fresh UUID");

    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","limit":500}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "r1 filter accepted: {resp}");
    let events = v["result"]["events"].as_array().unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|e| e["value"]["kind"].as_str())
        .collect();
    for e in events {
        let kind = e["value"]["kind"].as_str().unwrap();
        if kind == "todo" || kind == "file_change" {
            assert_eq!(
                e["value"]["run_id"].as_str(),
                Some(run1.as_str()),
                "artifacts stay r1-scoped: {e}"
            );
        }
    }
    assert_eq!(kinds.iter().filter(|k| **k == "todo").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "file_change").count(), 1);
    assert_eq!(
        kinds.iter().filter(|k| **k == "user").count(),
        2,
        "non-artifact kinds from both runs pass through"
    );
    assert!(kinds.contains(&"tool_call"));
    assert!(v["result"].get("next_cursor").is_none(), "single page");

    // Filter first, then paginate: a cursor at r2's Todo must skip that
    // node instead of returning it as the first page item.
    let all = read_traj_events(&dir, &sid);
    let r2_todo_seq = all
        .iter()
        .find(|e| {
            matches!(
                &e.value,
                agentj_protocol::trajectory::TrajectoryValue::Todo { run_id, .. }
                    if run_id.as_deref() == Some(run2.as_str())
            )
        })
        .expect("r2 todo recorded")
        .seq;
    let expected = all
        .iter()
        .find(|e| e.seq >= r2_todo_seq && event_kept_for_run(&e.value, &run1))
        .expect("a later kept event exists");
    assert_ne!(expected.seq, r2_todo_seq, "the r2 todo is filtered out");
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"session/history","params":{{"session_id":"{sid}","run_id":"{run1}","cursor":{r2_todo_seq},"limit":1}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let page = v["result"]["events"].as_array().unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0]["seq"], expected.seq);
    assert_eq!(v["result"]["next_cursor"], expected.seq + 1);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&target).ok();
}

/// T5 AC: `session/history` rejects a malformed `run_id` as `-32602` with
/// `data.reason == "invalid_run_id"`; a valid UUID is accepted.
#[tokio::test]
async fn history_invalid_run_id_rejects() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let long = "a".repeat(129);
    for bad in ["not-a-uuid", long.as_str()] {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"session/history","params":{{"session_id":"s1","run_id":"{bad}"}}}}"#
        );
        let resp = server.handle_request(&raw).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602, "rejected: {resp}");
        assert_eq!(
            v["error"]["data"]["reason"], "invalid_run_id",
            "reason tag: {resp}"
        );
    }
    let valid = Uuid::new_v4().to_string();
    let raw = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"session/history","params":{{"session_id":"s1","run_id":"{valid}"}}}}"#
    );
    let resp = server.handle_request(&raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "valid UUID accepted: {resp}");
    assert_eq!(v["result"]["events"], serde_json::json!([]));
}

#[tokio::test]
async fn session_history_unknown_sid_and_no_store_return_empty() {
    let dir = temp_dir_traj("none");
    let server = stub_bus_server_with_usage();
    let raw =
        r#"{"jsonrpc":"2.0","id":8,"method":"session/history","params":{"session_id":"nope"}}"#;
    let resp = server.handle_request(raw).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "unknown sid is not an error: {resp}");
    assert_eq!(v["result"]["events"], serde_json::json!([]));
    assert!(v["result"].get("next_cursor").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_list_mirrors_store_list_order() {
    let dir = temp_dir_traj("order");
    let store = Arc::new(agentj_session::FileBackedStore::new(dir.as_path()).unwrap());
    let session: Arc<dyn SessionStore> = store.clone();
    let server = IpcServer::new(Kernel::new(), session);
    let a = store.create().unwrap();
    store
        .append(
            &a,
            ChatMessage {
                role: Role::User,
                content: "first".into(),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    let b = store.create().unwrap();
    store
        .append(
            &b,
            ChatMessage {
                role: Role::User,
                content: "second".into(),
                tool_call_id: None,
                tool_calls: vec![],
            },
        )
        .unwrap();
    store.archive(&b).unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":9,"method":"session/list"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let rows = v["result"]["sessions"].as_array().expect("sessions array");
    assert_eq!(rows.len(), 2, "both sessions listed: {resp}");
    let entries = store.list().unwrap();
    for (i, e) in entries.iter().enumerate() {
        assert_eq!(rows[i]["session_id"].as_str(), Some(e.session_id.as_str()));
        let want_state = format!("{:?}", e.status).to_lowercase();
        assert_eq!(rows[i]["state"].as_str(), Some(want_state.as_str()));
        assert_eq!(rows[i]["updated_at"].as_u64(), Some(e.updated_at));
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_history_clamps_limit_into_range() {
    let dir = temp_dir_traj("clamp");
    let mut server = stub_bus_server_with_usage();
    server.trajectory_store = Some(wired_store(&dir));
    server
        .handle_request(r#"{"jsonrpc":"2.0","id":2,"method":"session/run","params":{"session_id":"s1","content":"hi"}}"#)
        .await
        .unwrap();
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"method":"session/history","params":{"session_id":"s1","limit":0}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(v["result"]["events"][0]["seq"], 1);
    assert_eq!(
        v["result"]["next_cursor"], 2,
        "window full -> cursor advances"
    );
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":11,"method":"session/history","params":{"session_id":"s1","limit":9999}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["events"].as_array().unwrap().len(), 4);
    assert!(
        v["result"].get("next_cursor").is_none(),
        "nothing left behind"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Temp project root for the T21 `file/list` dispatch tests (std-only).
fn temp_dir_filelist(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-filedisp-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One isolated session whose `project_root` is `root`.
fn file_list_server(root: &std::path::Path) -> (IpcServer, String) {
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let sid = store.create().unwrap();
    store
        .set_project_root(&sid, root.canonicalize().unwrap())
        .unwrap();
    let session: Arc<dyn SessionStore> = store;
    (IpcServer::new(Kernel::new(), session), sid)
}

/// Dispatch one `file/list` request and return the parsed JSON-RPC response.
async fn file_list_call(
    server: &IpcServer,
    sid: &str,
    path: Option<&str>,
    depth: Option<u32>,
) -> serde_json::Value {
    let mut params = serde_json::json!({ "session_id": sid });
    if let Some(p) = path {
        params["path"] = serde_json::json!(p);
    }
    if let Some(d) = depth {
        params["depth"] = serde_json::json!(d);
    }
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "file/list",
        "params": params,
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    serde_json::from_str(&resp).unwrap()
}

/// Query variant of `file_list_call` (the shared helper has no query param).
async fn file_list_search_call(
    server: &IpcServer,
    sid: &str,
    path: Option<&str>,
    query: &str,
) -> serde_json::Value {
    let raw = serde_json::json!({
        "jsonrpc": "2.0", "id": 22, "method": "file/list",
        "params": { "session_id": sid, "path": path, "query": query },
    })
    .to_string();
    serde_json::from_str(&server.handle_request(&raw).await.unwrap()).unwrap()
}

#[tokio::test]
async fn dispatch_file_list_query_returns_ranked_matches_with_total() {
    let root = temp_dir_filelist("query");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "x").unwrap();
    std::fs::write(root.join("README.md"), "x").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_search_call(&server, &sid, None, "main").await;
    assert!(v["error"].is_null(), "query call must succeed: {v}");
    assert_eq!(v["result"]["total"], 1);
    assert_eq!(v["result"]["entries"][0]["path"], "src/main.rs");
    let blank = file_list_search_call(&server, &sid, None, "   ").await;
    assert!(
        blank["result"].get("total").is_none(),
        "blank query = legacy listing"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn dispatch_file_list_lists_root_sorted_excluding_git() {
    let root = temp_dir_filelist("root");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join("a.rs"), "hi").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "valid call must succeed: {v}");
    let entries = v["result"]["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["src", "a.rs"], "dirs first, .git excluded");
    assert_eq!(entries[0]["kind"], "dir");
    assert_eq!(entries[1]["kind"], "file");
    assert_eq!(entries[1]["size"], 2);
    assert_eq!(v["result"]["truncated"], false);
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn dispatch_file_list_maps_errors_to_codes_and_data() {
    let root = temp_dir_filelist("errors");
    std::fs::write(root.join("file.txt"), "x").unwrap();
    let outside = temp_dir_filelist("errors-outside");
    let (server, sid) = file_list_server(&root);

    let v = file_list_call(&server, &sid, Some("../x"), None).await;
    assert_eq!(v["error"]["code"], -32807, "dotdot escape: {v}");
    assert_eq!(v["error"]["data"]["path"], "../x");
    assert_eq!(v["error"]["data"]["reason"], "outside_project_root");

    let abs = outside.to_string_lossy().into_owned();
    let v = file_list_call(&server, &sid, Some(&abs), None).await;
    assert_eq!(v["error"]["code"], -32807, "absolute escape: {v}");
    assert_eq!(v["error"]["data"]["path"], abs);

    let v = file_list_call(&server, &sid, Some("file.txt"), None).await;
    assert_eq!(v["error"]["code"], -32602, "file path: {v}");
    assert_eq!(v["error"]["data"]["path"], "file.txt");
    assert_eq!(v["error"]["data"]["reason"], "not_a_directory");

    let v = file_list_call(&server, &sid, Some("nope"), None).await;
    assert_eq!(v["error"]["code"], -32602, "missing path: {v}");
    assert_eq!(v["error"]["data"]["path"], "nope");
    assert_eq!(v["error"]["data"]["reason"], "not_found");

    let v = file_list_call(&server, &sid, None, Some(9)).await;
    assert_eq!(v["error"]["code"], -32602, "depth 9: {v}");
    assert_eq!(v["error"]["data"]["reason"], "invalid_depth");

    let v = file_list_call(&server, "missing-session", None, None).await;
    assert_eq!(v["error"]["code"], -32802, "unknown session: {v}");

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
}

/// Run a git command in `root`, panicking on any failure (git is available
/// wherever the `git_sample` suite runs).
fn git_run(root: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("git unavailable: {e}"),
    }
}

/// AC: a git repo with modified / untracked / deleted files maps to M/A/D.
/// The deleted entry stays on disk via `git rm --cached` + `.gitignore`,
/// otherwise it would not be reachable through a directory listing.
#[tokio::test]
async fn dispatch_file_list_reports_git_statuses() {
    let root = temp_dir_filelist("git");
    git_run(&root, &["init", "-q"]);
    std::fs::write(root.join("m.txt"), "1\n").unwrap();
    std::fs::write(root.join("d.txt"), "1\n").unwrap();
    git_run(&root, &["add", "m.txt", "d.txt"]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::write(root.join("m.txt"), "1\n2\n").unwrap();
    std::fs::write(root.join("a.txt"), "new\n").unwrap();
    git_run(&root, &["rm", "--cached", "-q", "d.txt"]);
    std::fs::write(root.join(".gitignore"), "d.txt\n").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entry = |name: &str| -> serde_json::Value {
        v["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    assert_eq!(entry("m.txt")["git_status"], "M", "modified: {v}");
    assert_eq!(entry("a.txt")["git_status"], "A", "untracked: {v}");
    assert_eq!(entry("d.txt")["git_status"], "D", "staged delete: {v}");
    std::fs::remove_dir_all(&root).ok();
}

/// AC: a worktree deletion below `src` surfaces as the aggregate `D` on the
/// `src` directory entry even though the deleted file is not listed.
#[tokio::test]
async fn dispatch_file_list_reports_aggregate_dir_status() {
    let root = temp_dir_filelist("git-dir-agg");
    git_run(&root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("src/deep/del.txt"), "1\n").unwrap();
    git_run(&root, &["add", "."]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::remove_file(root.join("src/deep/del.txt")).unwrap(); // worktree delete -> ' D'
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entry = |name: &str| -> serde_json::Value {
        v["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    assert_eq!(
        entry("src")["git_status"],
        "D",
        "deleted descendant surfaces on the dir: {v}"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// Subdirectory listings keep entry paths root-relative so the git lookup
/// (snapshot keys are root-relative) still reports M/A/D.
#[tokio::test]
async fn dispatch_file_list_subdirectory_git_status_is_root_relative() {
    let root = temp_dir_filelist("git-sub");
    git_run(&root, &["init", "-q"]);
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/m.txt"), "1\n").unwrap();
    git_run(&root, &["add", "src/m.txt"]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::write(root.join("src/m.txt"), "1\n2\n").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, Some("src"), None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entries = v["result"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "one file under src: {v}");
    assert_eq!(
        entries[0]["path"], "src/m.txt",
        "entry path must stay root-relative: {v}"
    );
    assert_eq!(
        entries[0]["git_status"], "M",
        "root-relative git lookup must hit: {v}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn dispatch_file_list_unreadable_directory_is_internal_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir_filelist("unreadable");
    let locked = root.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        // Privileged runner: permission bits are not enforced → unexercisable here.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).ok();
        return;
    }
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, Some("locked"), None).await;
    assert_eq!(v["error"]["code"], -32603, "unreadable dir: {v}");
    assert_eq!(v["error"]["data"]["path"], "locked");
    assert!(
        v["error"]["data"]["reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "io reason must be carried: {v}"
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_panicking_dispatch_becomes_an_internal_panic_error() {
    let err = isolate_panic("kernel/status", async { panic!("boom") })
        .await
        .expect_err("a panicking dispatch must not propagate");
    assert_eq!(err.code, -32603);
    assert_eq!(err.message, "InternalPanic");
    let data = err.data.expect("error.data carries the panic details");
    assert_eq!(data["method"], "kernel/status");
    assert_eq!(data["reason"], "boom");
}

#[tokio::test]
async fn a_normal_dispatch_passes_through_the_panic_guard() {
    let ok = isolate_panic("kernel/status", async { Ok(serde_json::json!({ "a": 1 })) })
        .await
        .expect("a successful dispatch keeps its result");
    assert_eq!(ok["a"], 1);
}

/// The route table must stay in lockstep with the protocol registry: the
/// client-callable methods of `protocol/methods.json` (minus the two that
/// are handled outside dispatch: `initialized` is a notification, `exit`
/// closes the transport) are exactly the methods `dispatch_request` routes.
#[test]
fn dispatch_table_matches_methods_json() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/methods.json"
    ))
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut expected: Vec<String> = json["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            matches!(
                m["direction"].as_str(),
                Some("client_to_server") | Some("both")
            )
        })
        .map(|m| m["method"].as_str().unwrap().to_string())
        .filter(|m| m != "initialized" && m != "exit")
        .collect();
    expected.sort();
    let mut actual: Vec<String> = DISPATCH_METHODS.iter().map(|s| s.to_string()).collect();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(DISPATCH_METHODS.len(), 23);
    let unique: std::collections::BTreeSet<&String> = actual.iter().collect();
    assert_eq!(
        actual.len(),
        unique.len(),
        "duplicate method in DISPATCH_METHODS"
    );
}

/// Every method the table advertises must have a real match arm; a method
/// added to `DISPATCH_METHODS` without an arm would answer
/// `method_not_found` while the table claimed it was routed.
#[test]
fn every_dispatch_method_appears_in_the_match() {
    let src = include_str!("../dispatch.rs");
    for method in DISPATCH_METHODS {
        assert!(
            src.contains(&format!("\"{method}\" =>")),
            "{method} has no match arm"
        );
    }
}

/// Build the `fn <name>(` needle at runtime: this assertion lives in
/// `lib.rs`, so an inlined needle would match its own source text.
fn fn_needle(name: &str) -> String {
    format!("fn {name}(")
}

/// The three request-framing entry points live in `dispatch.rs`; `lib.rs`
/// keeps only the module declaration. Pinned so a later edit cannot quietly
/// grow the root file back.
#[test]
fn dispatch_functions_moved_out_of_lib() {
    for name in ["dispatch_request", "handle_request_from", "auth_gate"] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../dispatch.rs").contains(&needle),
            "{name} missing"
        );
    }
}

/// Session handlers (new / list / history / status / archive / delete and
/// the root + status helpers they own) live in `handlers/session.rs`.
#[test]
fn session_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_session_new",
        "resolve_project_root",
        "dispatch_session_list",
        "dispatch_session_history",
        "dispatch_session_status",
        "dispatch_session_archive",
        "dispatch_session_delete",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/session.rs").contains(&needle),
            "{name} missing from session.rs"
        );
    }
}

/// `session/run` streaming, its frame helpers and cancellation live in
/// `handlers/run.rs`.
#[test]
fn run_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_session_run",
        "record_trajectory",
        "flush_thinking",
        "is_visually_blank",
        "build_run_reply",
        "dispatch_cancel_request",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/run.rs").contains(&needle),
            "{name} missing from run.rs"
        );
    }
}

/// Pending-request handlers (`permission/respond`, `question/respond`,
/// owner-disconnect settlement and the question notifier hooks) live in
/// `handlers/pending.rs`.
#[test]
fn pending_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_permission_respond",
        "dispatch_question_respond",
        "cleanup_connection",
        "settle_owner_pending",
        "question_notifier",
        "question_settle_hook",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/pending.rs").contains(&needle),
            "{name} missing from pending.rs"
        );
    }
}

/// Provider handlers (`model/list`, `provider/usage`, the `auth/*` family)
/// and `kernel_model_info` live in `handlers/provider.rs`.
#[test]
fn provider_handlers_moved_out_of_lib() {
    for name in [
        "dispatch_model_list",
        "dispatch_auth_status",
        "dispatch_provider_usage",
        "dispatch_device_start",
        "dispatch_device_poll",
        "dispatch_set_api_key",
        "dispatch_clear_credential",
        "kernel_model_info",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/provider.rs").contains(&needle),
            "{name} missing from provider.rs"
        );
    }
}

/// `file/list` and its error mapping live in `handlers/file.rs`.
#[test]
fn file_handlers_moved_out_of_lib() {
    for name in ["dispatch_file_list", "map_file_list_error"] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/file.rs").contains(&needle),
            "{name} missing from file.rs"
        );
    }
}

#[test]
fn stdio_serving_moved_out_of_lib() {
    let struct_needle = format!("struct {} {{", "StdioConnection");
    for needle in [fn_needle("serve_stdio"), struct_needle] {
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{needle} still in lib.rs"
        );
        assert!(
            include_str!("../stdio.rs").contains(&needle),
            "{needle} missing from stdio.rs"
        );
    }
}

/// The `$/progress` frame builders and `attach_git_statuses` live in `frames.rs`.
#[test]
fn frame_builders_moved_out_of_lib() {
    for name in [
        "build_progress_frame",
        "todo_items_into_wire",
        "attach_git_statuses",
    ] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../frames.rs").contains(&needle),
            "{name} missing from frames.rs"
        );
    }
}
