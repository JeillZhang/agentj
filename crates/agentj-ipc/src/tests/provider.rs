//! `provider` area tests.

use super::*;
use crate::test_support::{stub_server, stub_session};
use crate::*;
use agentj_core::types::*;
use std::sync::Arc;

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
