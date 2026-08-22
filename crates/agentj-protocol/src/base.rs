//! JSON-RPC 2.0 base protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request/notification id. `Null` is used only in error responses
/// where the request id could not be determined (e.g. parse error — JSON-RPC 2.0 §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    Str(String),
    Null,
}

/// Streaming progress token — independent of the request id (design 决策 #3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ProgressToken {
    Number(i64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RequestMessage {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotificationMessage {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response: exactly one of `result` (success) or `error` (failure).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ResponseMessage {
    Success {
        jsonrpc: String,
        id: Id,
        result: serde_json::Value,
    },
    Error {
        jsonrpc: String,
        id: Id,
        error: ErrorObject,
    },
}

/// Protocol error codes — JSON-RPC reserved range plus the agentj block
/// (design 决策 #4). The JSON-RPC `-32000..-32099` server-error range is
/// implementation-defined and reserved (documented in the spec, no constant).
pub mod error_codes {
    // JSON-RPC 2.0 reserved
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    // agentj reserved block
    pub const REQUEST_CANCELLED: i32 = -32800;
    pub const SERVER_NOT_INITIALIZED: i32 = -32801;
    pub const SESSION_NOT_FOUND: i32 = -32802;
    pub const SESSION_INACTIVE: i32 = -32803;
    pub const INVALID_VERSION: i32 = -32804;
    pub const SESSION_BUSY: i32 = -32805; // same session already running (design 错误处理)
    pub const NOT_AUTHORIZED: i32 = -32806; // non-owner permission/respond or $/cancelRequest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_to_jsonrpc_2_0() {
        let req = RequestMessage {
            jsonrpc: "2.0".into(),
            id: Id::Number(1),
            method: "session/new".into(),
            params: Some(serde_json::json!({})),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "session/new");
        assert!(json.get("params").is_some());
    }

    #[test]
    fn notification_has_no_id() {
        let n = NotificationMessage {
            jsonrpc: "2.0".into(),
            method: "initialized".into(),
            params: Some(serde_json::json!({})),
        };
        let json = serde_json::to_value(&n).unwrap();
        assert!(json.get("id").is_none());
        assert_eq!(json["method"], "initialized");
    }

    #[test]
    fn response_success_and_error_are_distinct() {
        let ok = ResponseMessage::Success {
            jsonrpc: "2.0".into(),
            id: Id::Number(1),
            result: serde_json::json!({"session_id": "s1"}),
        };
        let err = ResponseMessage::Error {
            jsonrpc: "2.0".into(),
            id: Id::Number(1),
            error: ErrorObject {
                code: error_codes::INVALID_PARAMS,
                message: "bad".into(),
                data: None,
            },
        };
        let ok_json = serde_json::to_value(&ok).unwrap();
        let err_json = serde_json::to_value(&err).unwrap();
        assert!(ok_json.get("result").is_some());
        assert!(ok_json.get("error").is_none());
        assert!(err_json.get("error").is_some());
        assert!(err_json.get("result").is_none());
    }

    #[test]
    fn error_response_with_data_and_null_id_roundtrips() {
        // Parse-error responses carry id=null (JSON-RPC 2.0 §5) and optional data.
        let err = ResponseMessage::Error {
            jsonrpc: "2.0".into(),
            id: Id::Null,
            error: ErrorObject {
                code: error_codes::PARSE_ERROR,
                message: "invalid json".into(),
                data: Some(serde_json::json!({"line": 3})),
            },
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["id"], serde_json::Value::Null);
        assert_eq!(json["error"]["code"], -32700);
        assert_eq!(json["error"]["data"]["line"], 3);
    }

    #[test]
    fn error_codes_have_reserved_values() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::REQUEST_CANCELLED, -32800);
        assert_eq!(error_codes::SERVER_NOT_INITIALIZED, -32801);
        assert_eq!(error_codes::SESSION_NOT_FOUND, -32802);
        assert_eq!(error_codes::SESSION_INACTIVE, -32803);
        assert_eq!(error_codes::INVALID_VERSION, -32804);
        assert_eq!(error_codes::SESSION_BUSY, -32805); // new
        assert_eq!(error_codes::NOT_AUTHORIZED, -32806); // new
    }

    #[test]
    fn id_accepts_number_string_and_null() {
        let num: Id = serde_json::from_value(serde_json::json!(42)).unwrap();
        let s: Id = serde_json::from_value(serde_json::json!("abc")).unwrap();
        let n: Id = serde_json::from_value(serde_json::json!(null)).unwrap();
        assert!(matches!(num, Id::Number(42)));
        assert!(matches!(s, Id::Str(_)));
        assert!(matches!(n, Id::Null));
    }

    #[test]
    fn progress_token_untagged_number_and_string() {
        let n: ProgressToken = serde_json::from_value(serde_json::json!(7)).unwrap();
        let s: ProgressToken = serde_json::from_value(serde_json::json!("tok")).unwrap();
        assert!(matches!(n, ProgressToken::Number(7)));
        assert!(matches!(s, ProgressToken::Str(_)));
    }

    #[test]
    fn base_types_generate_json_schema() {
        let s = schemars::schema_for!(RequestMessage);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["type"], "object");
        assert!(json["properties"].get("jsonrpc").is_some());
    }
}
