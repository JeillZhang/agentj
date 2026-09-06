//! Permission flow wire types (design 决策 #17). Additive-only: these mirror the
//! $/progress token-decoupling pattern — request_id is opaque and independent of
//! the JSON-RPC request id.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::SessionId;

/// S→C notification: a tool call is awaiting user approval (`Ask` decision).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRequestParams {
    /// Opaque id decoupled from the JSON-RPC request id (like $/progress token).
    pub request_id: String,
    /// Tool name, e.g. "bash".
    pub tool: String,
    /// Raw tool arguments (contains `cmd` for bash, `path` for file tools).
    pub args: serde_json::Value,
    /// Human-readable reason (tool default mode / matched rule).
    pub reason: String,
    /// Connection id of the client that initiated the run (design 决策 #5).
    /// Absent in single-client (stdio) mode — additive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_client: Option<String>,
    /// Session owning the approval (client-side UI attribution; additive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>, // @since 0.6.0
}

/// C→S request: the client's decision on a pending permission request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRespondParams {
    pub request_id: String,
    pub decision: PermissionDecision,
}

/// `permission/respond` success result — empty object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRespondResult {}

/// Decision value in `PermissionRespondParams`. `Unknown` tolerates forward
/// values (serde(other)); schema lists only the known values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    AllowAlways,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_request_params_serialize_snake_case() {
        let p = PermissionRequestParams {
            request_id: "r1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"cmd": "ls"}),
            reason: "tool default mode is ask".into(),
            origin_client: None,
            session_id: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["request_id"], "r1");
        assert_eq!(json["tool"], "bash");
        assert_eq!(json["args"]["cmd"], "ls");
        assert_eq!(json["reason"], "tool default mode is ask");
    }

    #[test]
    fn permission_respond_params_roundtrip() {
        let p = PermissionRespondParams {
            request_id: "r1".into(),
            decision: PermissionDecision::Allow,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["decision"], "allow");
        let back: PermissionRespondParams = serde_json::from_value(json).unwrap();
        assert!(matches!(back.decision, PermissionDecision::Allow));
    }

    #[test]
    fn permission_decision_allow_always_roundtrip() {
        let p = PermissionRespondParams {
            request_id: "r1".into(),
            decision: PermissionDecision::AllowAlways,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["decision"], "allow_always");
    }

    #[test]
    fn probe_allow_always_bool_is_silently_dropped() {
        // B-102 regression lock: an `allow_always` boolean is not part of the wire
        // contract — it is silently dropped, so the decision stays Allow.
        let v: PermissionRespondParams = serde_json::from_value(serde_json::json!({
            "request_id": "r1",
            "decision": "allow",
            "allow_always": true,
        }))
        .unwrap();
        assert!(matches!(v.decision, PermissionDecision::Allow));
        let schema = serde_json::to_value(schemars::schema_for!(PermissionRespondParams)).unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert!(!props.contains_key("allow_always"));
        let ok: PermissionRespondParams = serde_json::from_value(serde_json::json!({
            "request_id": "r1",
            "decision": "allow_always",
        }))
        .unwrap();
        assert!(matches!(ok.decision, PermissionDecision::AllowAlways));
    }

    #[test]
    fn permission_decision_unknown_kind_falls_back() {
        // Forward compatibility: an unknown decision value must deserialize
        // to the Unknown variant instead of failing (serde(other)).
        let v: PermissionDecision = serde_json::from_str("\"maybe\"").unwrap();
        assert!(matches!(v, PermissionDecision::Unknown));
    }

    #[test]
    fn permission_respond_result_serialize() {
        let r = PermissionRespondResult {};
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.is_object() && json.as_object().unwrap().is_empty());
    }

    #[test]
    fn permission_types_generate_json_schema() {
        let rp = schemars::schema_for!(PermissionRequestParams);
        assert_eq!(serde_json::to_value(&rp).unwrap()["type"], "object");
        let rr = schemars::schema_for!(PermissionRespondParams);
        assert_eq!(serde_json::to_value(&rr).unwrap()["type"], "object");
    }

    #[test]
    fn permission_request_params_with_origin_client() {
        let p = PermissionRequestParams {
            request_id: "req-1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
            reason: "tool default mode is ask".into(),
            origin_client: Some("conn-a".into()),
            session_id: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["origin_client"], "conn-a");
    }

    #[test]
    fn permission_request_params_without_origin_client_omits_field() {
        let p = PermissionRequestParams {
            request_id: "req-1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
            reason: "tool default mode is ask".into(),
            origin_client: None,
            session_id: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("origin_client").is_none());
    }

    #[test]
    fn permission_request_params_with_session_id() {
        let p = PermissionRequestParams {
            request_id: "req-1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
            reason: "tool default mode is ask".into(),
            origin_client: Some("conn-a".into()),
            session_id: Some("s-1".into()),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["session_id"], "s-1");
    }

    #[test]
    fn permission_request_params_without_session_id_omits_field() {
        let p = PermissionRequestParams {
            request_id: "req-1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
            reason: "tool default mode is ask".into(),
            origin_client: None,
            session_id: None,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("session_id").is_none());
    }
}
