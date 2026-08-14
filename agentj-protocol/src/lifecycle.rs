use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// `initialize` request params (design 决策 #2).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitializeResult {
    pub protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<serde_json::Value>,
    pub server_info: ServerInfo,
}

// `initialized`, `shutdown`, `exit` carry no params — represented as
// `params: null` in the registry, no dedicated structs.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_params_serialize() {
        let p = InitializeParams {
            protocol_version: "0.1.0".into(),
            capabilities: Some(serde_json::json!({"streaming": true})),
            client_info: Some(ClientInfo { name: "cli".into(), version: "0.1.0".into() }),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["protocol_version"], "0.1.0");
        assert_eq!(json["capabilities"]["streaming"], true);
        assert_eq!(json["client_info"]["name"], "cli");
    }

    #[test]
    fn initialize_result_roundtrip() {
        let r = InitializeResult {
            protocol_version: "0.1.0".into(),
            capabilities: Some(serde_json::json!({"streaming": true})),
            server_info: ServerInfo { name: "agentj".into(), version: "0.1.0".into() },
        };
        let back: InitializeResult = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.protocol_version, "0.1.0");
        assert_eq!(back.server_info.name, "agentj");
    }

    #[test]
    fn lifecycle_types_generate_json_schema() {
        let s = schemars::schema_for!(InitializeParams);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["type"], "object");
        assert!(json["properties"].get("protocol_version").is_some());
    }
}
