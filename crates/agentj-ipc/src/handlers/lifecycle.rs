//! Per-domain request handlers, moved out of `lib.rs` one domain at a time.
//!
//! Every function here is a verbatim move: signatures, visibility, error codes
//! and control flow are unchanged. The route table itself lives in
//! `dispatch.rs`; these modules only host the `impl IpcServer` method bodies.

use crate::{DispatchError, IpcServer, JsonRpcRequest, kernel_model_info, to_wire};
use agentj_protocol::base::error_codes;
use agentj_protocol::lifecycle::InitializeParams;
use agentj_protocol::session::{SessionIdParams, SessionSubscribeParams};
use serde_json::Value;
use std::sync::atomic::Ordering;

impl IpcServer {
    pub(crate) fn dispatch_initialize(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: InitializeParams =
            serde_json::from_value(req.params.clone().unwrap_or(serde_json::json!({})))
                .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(expected) = self.token.lock()
            && let Some(expected) = expected.as_deref()
            && params.token.as_deref().unwrap_or("") != expected
        {
            return Err(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
                data: None,
            });
        }
        // Successful auth: the connection may now issue business methods on a
        // token-gated server (the non-initialize gate checks this registry).
        if let Ok(mut reg) = self.registry.write() {
            reg.mark_authorized(conn_id);
        }
        let product_version = env!("CARGO_PKG_VERSION");
        Ok(serde_json::json!({
            "protocol_version": agentj_protocol::PROTOCOL_VERSION,
            "capabilities": { "multi_client": true },
            "server_info": { "name": "agentj", "version": product_version },
            "connection_id": conn_id,
        }))
    }

    pub(crate) fn dispatch_subscribe(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: SessionSubscribeParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(mut reg) = self.registry.write() {
            // A run outlives a client disconnect (spec 8.11): ownership is dropped
            // when the owning connection ends, so the first connection to
            // re-attach to a session nobody owns becomes its owner again — only an
            // owner may answer approvals, otherwise every subsequent tool approval
            // in the still-running turn would be denied for lack of a responder.
            // An existing owner is never stolen from.
            if reg.session_owner(&params.session_id).is_none() {
                reg.set_session_owner(&params.session_id, conn_id);
            }
            reg.subscribe(&params.session_id, conn_id);
        }
        Ok(serde_json::json!({}))
    }

    pub(crate) fn dispatch_unsubscribe(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        if let Ok(mut reg) = self.registry.write() {
            reg.unsubscribe(&params.session_id, conn_id);
        }
        Ok(serde_json::json!({}))
    }

    pub(crate) fn dispatch_shutdown(&self) -> Result<Value, DispatchError> {
        self.signal_shutdown()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        self.shutdown_requested.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "status": "ok" }))
    }

    pub(crate) fn dispatch_kernel_status(&self) -> Result<Value, DispatchError> {
        let modules: Vec<Value> = self
            .kernel
            .modules()
            .iter()
            .map(|(id, state)| {
                serde_json::json!({
                    "id": id,
                    "state": format!("{state:?}").to_lowercase()
                })
            })
            .collect();
        let (default_model, providers) = kernel_model_info(self.kernel.ctx().as_ref());
        let mut body = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "modules": modules,
        });
        if let Some(model) = default_model {
            body["default_model"] = serde_json::json!(model);
        }
        if let Some(ps) = providers {
            body["providers"] = to_wire(ps)?;
        }
        Ok(body)
    }
}
