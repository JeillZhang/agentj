//! JSON-RPC 2.0 envelope plumbing and the method-dispatch error type.
//!
//! Split out of `lib.rs`: request parsing (with the `-32700`/`-32600` split),
//! response framing, param helpers and `DispatchError`. Everything here is a
//! verbatim move: signatures, visibility and wire shape are unchanged.

use agentj_protocol::base::error_codes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String, // JSON-RPC 2.0 envelope version — required by serde, value not inspected
    /// Notifications carry no id (JSON-RPC 2.0 §2) — defaults to Null.
    #[serde(default)]
    pub(crate) id: Value,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcResponse {
    pub(crate) jsonrpc: String,
    pub(crate) id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcError {
    pub(crate) code: i32,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<Value>,
}

/// Parse `raw` into a [`JsonRpcRequest`] or, on failure, build the JSON-RPC
/// 2.0 error response (JSON-RPC 2.0 §5). A syntax error is `-32700` with a
/// null id; valid JSON that fails request schema validation is `-32600` with
/// the raw request id echoed when it is a string, number or null.
pub(crate) fn parse_request(raw: &str) -> Result<JsonRpcRequest, Box<JsonRpcResponse>> {
    match serde_json::from_str::<JsonRpcRequest>(raw) {
        Ok(req) => Ok(req),
        Err(e) => {
            let (code, id, message) = match serde_json::from_str::<Value>(raw) {
                Ok(value) => (
                    error_codes::INVALID_REQUEST,
                    extract_request_id(&value),
                    format!("Invalid Request: {e}"),
                ),
                Err(_) => (error_codes::PARSE_ERROR, None, format!("Parse error: {e}")),
            };
            Err(Box::new(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: id.unwrap_or(Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code,
                    message,
                    data: None,
                }),
            }))
        }
    }
}

/// Extract a JSON-RPC 2.0 id from a raw JSON value. Only string, number and
/// null ids are valid; absent or invalid types (object/array/bool) yield
/// `None` so the error response falls back to a null id.
pub(crate) fn extract_request_id(value: &Value) -> Option<Value> {
    match value.get("id") {
        Some(id @ (Value::String(_) | Value::Number(_) | Value::Null)) => Some(id.clone()),
        _ => None,
    }
}

/// Optional string param for `session/run` (design 错误处理): a missing key or
/// JSON `null` means "absent"; a string is trimmed and a blank result also
/// means "absent" (same semantics as `file/list {query}`); any other JSON type
/// is `-32602` with the offending field name in `error.data`.
pub(crate) fn optional_trimmed(params: &Value, key: &str) -> Result<Option<String>, DispatchError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
        }
        Some(_) => Err(DispatchError::invalid_params_with_data(
            format!("{key} must be a string"),
            serde_json::json!({ "field": key }),
        )),
    }
}

/// Serialize a protocol value into a response body.
///
/// serde on our own wire types practically cannot fail, but a `panic!` inside a
/// request would drop the connection, so the impossible case becomes `-32603`.
pub(crate) fn to_wire<T: serde::Serialize>(value: T) -> Result<Value, DispatchError> {
    serde_json::to_value(value)
        .map_err(|e| DispatchError::internal(format!("response serialization failed: {e}")))
}

/// Method-dispatch failure carrying the JSON-RPC 2.0 standard error code
/// (design 决策 #4). `handle_request` echoes the request `id` alongside this
/// code so every error path round-trips the correlation id. `data` carries
/// parameterized error details (`error.data`, design 决策 #5) and stays off
/// the wire when None.
#[derive(Debug, Serialize)]
pub struct DispatchError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl DispatchError {
    pub(crate) fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }
    pub fn invalid_params_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: Some(data),
        }
    }

    /// Uninjected handle: the seven subscription methods report this (`design D4`).
    pub(crate) fn provider_service_unavailable() -> Self {
        Self::internal_with_data(
            "ProviderServiceUnavailable",
            serde_json::json!({ "reason": "provider_service_unavailable" }),
        )
    }
    pub(crate) fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
            data: None,
        }
    }
    pub(crate) fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
            data: None,
        }
    }
    pub(crate) fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: None,
        }
    }
    pub fn internal_with_data(msg: impl Into<String>, data: Value) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: Some(data),
        }
    }
}
