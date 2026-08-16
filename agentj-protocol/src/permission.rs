//! Permission flow wire types (design 决策 #17). Additive-only: these mirror the
//! $/progress token-decoupling pattern — request_id is opaque and independent of
//! the JSON-RPC request id.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}
