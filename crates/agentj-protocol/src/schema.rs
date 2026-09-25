//! Schema generation registry.

use schemars::Schema;
use schemars::schema_for;
use std::collections::BTreeMap;

use crate::base::{
    ErrorObject, Id, NotificationMessage, ProgressToken, RequestMessage, ResponseMessage,
};
use crate::file::{FileEntry, FileKind, FileListParams, FileListResult};
use crate::lifecycle::{ClientInfo, InitializeParams, InitializeResult, ServerInfo};
use crate::permission::{
    PermissionDecision, PermissionRequestParams, PermissionRespondParams, PermissionRespondResult,
};
use crate::question::{AskOption, AskQuestionParams, QuestionRespondParams, QuestionRespondResult};
use crate::session::{
    SessionHistoryParams, SessionHistoryResult, SessionIdParams, SessionListParams,
    SessionListResult, SessionNewParams, SessionNewResult, SessionRunParams, SessionRunResult,
    SessionStatusResult, SessionSubscribeParams, SessionSummary, UsageInfo,
};
use crate::streaming::{
    CancelParams, FileChange, FileChangeKind, FileChangeSource, KernelStatusResult, ModuleInfo,
    ProgressParams, ProgressValue, TodoItem, TodoStatus,
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
    m.insert("SessionNewParams".into(), schema_for!(SessionNewParams));
    m.insert("SessionRunParams".into(), schema_for!(SessionRunParams));
    m.insert("SessionRunResult".into(), schema_for!(SessionRunResult));
    m.insert("UsageInfo".into(), schema_for!(UsageInfo));
    m.insert("SessionIdParams".into(), schema_for!(SessionIdParams));
    m.insert(
        "SessionStatusResult".into(),
        schema_for!(SessionStatusResult),
    );
    m.insert(
        "SessionSubscribeParams".into(),
        schema_for!(SessionSubscribeParams),
    );
    m.insert("SessionSummary".into(), schema_for!(SessionSummary));
    m.insert("SessionListParams".into(), schema_for!(SessionListParams));
    m.insert("SessionListResult".into(), schema_for!(SessionListResult));
    m.insert(
        "SessionHistoryParams".into(),
        schema_for!(SessionHistoryParams),
    );
    m.insert(
        "SessionHistoryResult".into(),
        schema_for!(SessionHistoryResult),
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
    m.insert("FileListParams".into(), schema_for!(FileListParams));
    m.insert("FileListResult".into(), schema_for!(FileListResult));
    m.insert("FileEntry".into(), schema_for!(FileEntry));
    m.insert("FileKind".into(), schema_for!(FileKind));
    m.insert("AskQuestionParams".into(), schema_for!(AskQuestionParams));
    m.insert("AskOption".into(), schema_for!(AskOption));
    m.insert(
        "QuestionRespondParams".into(),
        schema_for!(QuestionRespondParams),
    );
    m.insert(
        "QuestionRespondResult".into(),
        schema_for!(QuestionRespondResult),
    );
    m.insert("TodoItem".into(), schema_for!(TodoItem));
    m.insert("TodoStatus".into(), schema_for!(TodoStatus));
    m.insert("FileChange".into(), schema_for!(FileChange));
    m.insert("FileChangeKind".into(), schema_for!(FileChangeKind));
    m.insert("FileChangeSource".into(), schema_for!(FileChangeSource));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn schemas_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
    }

    #[test]
    fn drift_registered_types_are_known() {
        let schemas = generate_schemas();
        assert!(schemas.contains_key("RequestMessage"));
        assert!(schemas.contains_key("InitializeParams"));
        assert!(schemas.contains_key("SessionRunParams"));
        assert!(schemas.contains_key("ProgressValue"));
        assert!(schemas.contains_key("KernelStatusResult"));
        assert!(schemas.contains_key("SessionSubscribeParams")); // new
        assert!(schemas.contains_key("PermissionRequestParams"));
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
