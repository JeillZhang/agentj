//! Request dispatch: JSON-RPC framing, the token gate, and the route table.
//!
//! Split out of `lib.rs` so the whole route table sits behind one panic guard
//! and can grow by domain without touching response framing. Everything here is
//! a verbatim move: signatures, visibility, error codes and control flow are
//! unchanged.

use crate::{
    DISPATCH_METHODS, DispatchError, IpcServer, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    isolate_panic, parse_request,
};
use agentj_core::error::IpcError;
use agentj_protocol::base::error_codes;
use serde_json::Value;

impl IpcServer {
    /// Handle a single JSON-RPC request on the stdio sentinel connection
    /// ("stdio") — v0.1 single-client entry, unchanged behavior.
    pub async fn handle_request(&self, raw: &str) -> Result<String, IpcError> {
        self.handle_request_from(raw, "stdio").await
    }

    /// Handle a single JSON-RPC request from connection `conn_id`. Valid
    /// requests echo the request `id` (JSON-RPC 2.0 §5) with the semantically
    /// correct standard code, never falling through to the serve loop.
    /// Malformed requests still get an error line: a syntax error yields
    /// `-32700` with a null id (the id is undetectable), while valid JSON that
    /// fails request schema validation yields `-32600` and echoes the raw `id`
    /// when it is a string, number or null.
    pub async fn handle_request_from(&self, raw: &str, conn_id: &str) -> Result<String, IpcError> {
        let req = match parse_request(raw) {
            Ok(r) => r,
            Err(err_resp) => return Ok(serde_json::to_string(&*err_resp)?),
        };

        let result = if let Some(err) = self.auth_gate(&req, conn_id) {
            Err(err)
        } else {
            isolate_panic(&req.method, self.dispatch_request(&req, conn_id)).await
        };

        let resp = match result {
            Ok(r) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(r),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: e.code,
                    message: e.message,
                    data: e.data,
                }),
            },
        };
        if req.id == Value::Null {
            // JSON-RPC 2.0 §2.1: a notification (no id) must not get a response
            // line. The handler above still ran — the side effect is delivered.
            return Ok(String::new());
        }
        Ok(serde_json::to_string(&resp)?)
    }

    /// Route one already-authorized request to its dispatch method.
    ///
    /// Split out of [`Self::handle_request_from`] so the whole route table can
    /// sit behind a single panic guard (and so the table can later be split by
    /// domain without touching the response framing).
    async fn dispatch_request(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        match req.method.as_str() {
            "initialize" => self.dispatch_initialize(req, conn_id),
            "session/new" => self.dispatch_session_new(req, conn_id),
            "session/run" => self.dispatch_session_run(req, conn_id).await,
            "session/subscribe" => self.dispatch_subscribe(req, conn_id),
            "session/unsubscribe" => self.dispatch_unsubscribe(req, conn_id),
            "kernel/status" => self.dispatch_kernel_status(),
            "session/list" => self.dispatch_session_list(),
            "session/history" => self.dispatch_session_history(req),
            "session/status" => self.dispatch_session_status(req),
            "session/archive" => self.dispatch_session_archive(req),
            "session/delete" => self.dispatch_session_delete(req),
            "file/list" => self.dispatch_file_list(req).await,
            "model/list" => self.dispatch_model_list(req).await,
            "auth/status" => self.dispatch_auth_status(req).await,
            "provider/usage" => self.dispatch_provider_usage(req).await,
            "provider/usageHistory" => self.dispatch_provider_usage_history(req).await,
            "auth/deviceStart" => self.dispatch_device_start(req).await,
            "auth/devicePoll" => self.dispatch_device_poll(req).await,
            "auth/setApiKey" => self.dispatch_set_api_key(req).await,
            "auth/clearCredential" => self.dispatch_clear_credential(req).await,
            "shutdown" => self.dispatch_shutdown(),
            "permission/respond" => self.dispatch_permission_respond(req, conn_id).await,
            "question/respond" => self.dispatch_question_respond(req, conn_id),
            "$/cancelRequest" => self.dispatch_cancel_request(req).await,
            other => {
                // `DISPATCH_METHODS` is the published route table; a method it
                // advertises but this match does not route is a table/match
                // disagreement, which is a bug worth catching in debug builds.
                debug_assert!(
                    !DISPATCH_METHODS.contains(&other),
                    "{other} is advertised in DISPATCH_METHODS but has no match arm"
                );
                Err(DispatchError::method_not_found(other))
            }
        }
    }

    /// Token-mode auth gate: when a token is configured, only connections
    /// that already passed `initialize` may issue business methods. The
    /// `initialize` method itself always reaches `dispatch_initialize` (which
    /// performs the token check and marks the connection authorized). stdio
    /// serves with no token, so this gate is inert on that path.
    fn auth_gate(&self, req: &JsonRpcRequest, conn_id: &str) -> Option<DispatchError> {
        if req.method.as_str() == "initialize" {
            return None;
        }
        let token_configured = self.token.lock().ok().map(|t| t.is_some()).unwrap_or(false);
        if !token_configured {
            return None;
        }
        let authorized = self
            .registry
            .read()
            .ok()
            .map(|r| r.is_authorized(conn_id))
            .unwrap_or(false);
        if !authorized {
            return Some(DispatchError {
                code: error_codes::NOT_AUTHORIZED,
                message: "NotAuthorized".into(),
                data: None,
            });
        }
        None
    }
}
