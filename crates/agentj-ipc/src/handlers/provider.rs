//! `model/list`, `provider/usage` and the `auth/*` handlers, plus the kernel model
//! info probe that feeds `kernel/status`.
//!
//! Verbatim move out of `lib.rs`: signatures, error codes (`-32603
//! provider_service_unavailable`) and control flow are unchanged; only
//! visibility widened to `pub(crate)` where the route table reaches in.

use crate::{DispatchError, IpcServer, JsonRpcRequest, ProviderService};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// Serialize a subscription result, mapping a (practically impossible) failure to `-32603`.
fn provider_result<T: Serialize>(out: T) -> Result<Value, DispatchError> {
    serde_json::to_value(out).map_err(|e| {
        DispatchError::internal_with_data(
            "SerializationFailed",
            serde_json::json!({ "reason": e.to_string() }),
        )
    })
}

impl IpcServer {
    /// `-32602` and the injected service is never called.
    pub(crate) fn provider_params<T: serde::de::DeserializeOwned>(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<T, DispatchError> {
        let raw = req.params.clone().unwrap_or_else(|| serde_json::json!({}));
        serde_json::from_value(raw).map_err(|e| {
            DispatchError::invalid_params_with_data(
                "InvalidParams",
                serde_json::json!({ "reason": "invalid_params", "detail": e.to_string() }),
            )
        })
    }

    /// The injected handle, or `-32603 provider_service_unavailable` (design D4).
    pub(crate) fn provider_service(&self) -> Result<&Arc<dyn ProviderService>, DispatchError> {
        self.provider_service
            .as_ref()
            .ok_or_else(DispatchError::provider_service_unavailable)
    }

    pub(crate) async fn dispatch_model_list(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.model_list(params).await?)
    }

    pub(crate) async fn dispatch_auth_status(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.auth_status(params).await?)
    }

    pub(crate) async fn dispatch_provider_usage(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.usage(params).await?)
    }

    pub(crate) async fn dispatch_device_start(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.device_start(params).await?)
    }

    pub(crate) async fn dispatch_device_poll(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.device_poll(params).await?)
    }

    pub(crate) async fn dispatch_set_api_key(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.set_api_key(params).await?)
    }

    pub(crate) async fn dispatch_clear_credential(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let svc = self.provider_service()?;
        let params = self.provider_params(req)?;
        provider_result(svc.clear_credential(params).await?)
    }
}

pub(crate) fn kernel_model_info(
    ctx: Option<&agentj_core::module::KernelContext>,
) -> (
    Option<String>,
    Option<Vec<agentj_protocol::streaming::ProviderInfo>>,
) {
    let Some(ctx) = ctx else { return (None, None) };
    let cfg = ctx.config();
    let default_model = cfg.get_str("model.default_model").filter(|s| !s.is_empty());
    let pairs = ctx.providers().list_with_models();
    let providers = (!pairs.is_empty()).then(|| {
        pairs
            .into_iter()
            .map(|(id, models)| agentj_protocol::streaming::ProviderInfo { id, models })
            .collect()
    });
    (default_model, providers)
}
