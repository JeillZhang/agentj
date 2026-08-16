//! Schema generation registry.

use schemars::Schema;
use schemars::schema_for;
use std::collections::BTreeMap;

use crate::base::{
    ErrorObject, Id, NotificationMessage, ProgressToken, RequestMessage, ResponseMessage,
};
use crate::lifecycle::{ClientInfo, InitializeParams, InitializeResult, ServerInfo};
use crate::permission::{
    PermissionDecision, PermissionRequestParams, PermissionRespondParams, PermissionRespondResult,
};
use crate::session::{
    SessionIdParams, SessionNewResult, SessionRunParams, SessionRunResult, SessionStatusResult,
};
use crate::streaming::{
    CancelParams, KernelStatusResult, ModuleInfo, ProgressParams, ProgressValue,
};
use crate::types::{
    Attachment, ChatMessage, ModuleState, Role, SessionStatus, ToolCall, ToolDefinition,
};

/// Map of type name -> JSON Schema, the single machine source of truth.
pub fn generate_schemas() -> BTreeMap<String, Schema> {
    let mut m = BTreeMap::new();
    m.insert("Id".into(), schema_for!(Id));
    m.insert("ProgressToken".into(), schema_for!(ProgressToken));
    m.insert("RequestMessage".into(), schema_for!(RequestMessage));
    m.insert(
        "NotificationMessage".into(),
        schema_for!(NotificationMessage),
    );
    m.insert("ResponseMessage".into(), schema_for!(ResponseMessage));
    m.insert("ErrorObject".into(), schema_for!(ErrorObject));
    m.insert("Role".into(), schema_for!(Role));
    m.insert("ToolCall".into(), schema_for!(ToolCall));
    m.insert("ChatMessage".into(), schema_for!(ChatMessage));
    m.insert("Attachment".into(), schema_for!(Attachment));
    m.insert("ToolDefinition".into(), schema_for!(ToolDefinition));
    m.insert("ModuleState".into(), schema_for!(ModuleState));
    m.insert("SessionStatus".into(), schema_for!(SessionStatus));
    m.insert("ClientInfo".into(), schema_for!(ClientInfo));
    m.insert("ServerInfo".into(), schema_for!(ServerInfo));
    m.insert("InitializeParams".into(), schema_for!(InitializeParams));
    m.insert("InitializeResult".into(), schema_for!(InitializeResult));
    m.insert("SessionNewResult".into(), schema_for!(SessionNewResult));
    m.insert("SessionRunParams".into(), schema_for!(SessionRunParams));
    m.insert("SessionRunResult".into(), schema_for!(SessionRunResult));
    m.insert("SessionIdParams".into(), schema_for!(SessionIdParams));
    m.insert(
        "SessionStatusResult".into(),
        schema_for!(SessionStatusResult),
    );
    m.insert("ProgressParams".into(), schema_for!(ProgressParams));
    m.insert("ProgressValue".into(), schema_for!(ProgressValue));
    m.insert("CancelParams".into(), schema_for!(CancelParams));
    m.insert("KernelStatusResult".into(), schema_for!(KernelStatusResult));
    m.insert("ModuleInfo".into(), schema_for!(ModuleInfo));
    m.insert(
        "PermissionRequestParams".into(),
        schema_for!(PermissionRequestParams),
    );
    m.insert(
        "PermissionRespondParams".into(),
        schema_for!(PermissionRespondParams),
    );
    m.insert(
        "PermissionRespondResult".into(),
        schema_for!(PermissionRespondResult),
    );
    m.insert("PermissionDecision".into(), schema_for!(PermissionDecision));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn schemas_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas")
    }

    #[test]
    fn drift_registered_types_are_known() {
        let schemas = generate_schemas();
        assert!(schemas.contains_key("RequestMessage"));
        assert!(schemas.contains_key("InitializeParams"));
        assert!(schemas.contains_key("SessionRunParams"));
        assert!(schemas.contains_key("ProgressValue"));
        assert!(schemas.contains_key("KernelStatusResult"));
    }

    #[test]
    fn drift_committed_schemas_match_generated() {
        let generated = generate_schemas();
        let dir = schemas_dir();
        assert!(
            dir.is_dir(),
            "schemas/ directory missing — run `cargo run --bin gen_schemas`"
        );
        for (name, schema) in &generated {
            let path = dir.join(format!("{name}.json"));
            let committed: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(&path)
                    .unwrap_or_else(|_| panic!("missing committed schema for {name}")),
            )
            .unwrap();
            let regenerated = serde_json::to_value(schema).unwrap();
            assert_eq!(
                committed, regenerated,
                "drift detected for {name} — re-run `cargo run --bin gen_schemas`"
            );
        }
    }
}
