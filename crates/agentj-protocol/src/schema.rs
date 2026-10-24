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
use crate::provider::{
    ApiKeyLinkError, ApiKeyOutcome, AuthClearCredentialParams, AuthClearCredentialResult,
    AuthDevicePollParams, AuthDevicePollResult, AuthDeviceStartParams, AuthDeviceStartResult,
    AuthSetApiKeyParams, AuthSetApiKeyResult, AuthStatusParams, AuthStatusResult, CredentialInfo,
    CredentialKind, CredentialSource, DeviceAuthState, ModelEntry, ModelListParams,
    ModelListResult, ModelSource, ProviderUsageError, ProviderUsageParams, ProviderUsageResult,
    UsageWindow, UsageWindowKind, UsageWindowStatus,
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
    schema_targets()
        .into_iter()
        .map(|(name, build)| (name.to_string(), build()))
        .collect()
}

/// One schema-producing function: `schema_for!` is a macro, so each target needs its own
/// zero-argument closure to sit in a table.
type SchemaFn = fn() -> Schema;

/// Every type exposed on the wire, as (name, builder). The rows live in the group functions
/// below so no single table outgrows the function-length gate; adding a wire type means
/// adding a row (plus the corresponding entry in `protocol/methods.json`).
fn schema_targets() -> Vec<(&'static str, SchemaFn)> {
    base_targets()
        .into_iter()
        .chain(session_targets())
        .chain(stream_targets())
        .chain(permission_targets())
        .chain(model_targets())
        .chain(auth_targets())
        .chain(usage_targets())
        .chain(device_targets())
        .chain(link_targets())
        .chain(file_targets())
        .chain(question_targets())
        .collect()
}

/// Base envelope, shared value and lifecycle types.
fn base_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("ErrorObject", || schema_for!(ErrorObject)),
        ("Id", || schema_for!(Id)),
        ("NotificationMessage", || schema_for!(NotificationMessage)),
        ("ProgressToken", || schema_for!(ProgressToken)),
        ("RequestMessage", || schema_for!(RequestMessage)),
        ("ResponseMessage", || schema_for!(ResponseMessage)),
        ("Role", || schema_for!(Role)),
        ("ToolCall", || schema_for!(ToolCall)),
        ("ChatMessage", || schema_for!(ChatMessage)),
        ("Attachment", || schema_for!(Attachment)),
        ("ToolDefinition", || schema_for!(ToolDefinition)),
        ("ModuleState", || schema_for!(ModuleState)),
        ("SessionStatus", || schema_for!(SessionStatus)),
        ("ClientInfo", || schema_for!(ClientInfo)),
        ("ServerInfo", || schema_for!(ServerInfo)),
        ("InitializeParams", || schema_for!(InitializeParams)),
        ("InitializeResult", || schema_for!(InitializeResult)),
    ]
}

/// Session lifecycle, history and usage types.
fn session_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("SessionNewResult", || schema_for!(SessionNewResult)),
        ("SessionNewParams", || schema_for!(SessionNewParams)),
        ("SessionRunParams", || schema_for!(SessionRunParams)),
        ("SessionRunResult", || schema_for!(SessionRunResult)),
        ("UsageInfo", || schema_for!(UsageInfo)),
        ("SessionIdParams", || schema_for!(SessionIdParams)),
        ("SessionStatusResult", || schema_for!(SessionStatusResult)),
        ("SessionSubscribeParams", || {
            schema_for!(SessionSubscribeParams)
        }),
        ("SessionSummary", || schema_for!(SessionSummary)),
        ("SessionListParams", || schema_for!(SessionListParams)),
        ("SessionListResult", || schema_for!(SessionListResult)),
        ("SessionHistoryParams", || schema_for!(SessionHistoryParams)),
        ("SessionHistoryResult", || schema_for!(SessionHistoryResult)),
    ]
}

/// Progress, cancel, kernel-status and todo/file-change event types.
fn stream_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("ProgressParams", || schema_for!(ProgressParams)),
        ("ProgressValue", || schema_for!(ProgressValue)),
        ("CancelParams", || schema_for!(CancelParams)),
        ("KernelStatusResult", || schema_for!(KernelStatusResult)),
        ("ModuleInfo", || schema_for!(ModuleInfo)),
        ("TodoItem", || schema_for!(TodoItem)),
        ("TodoStatus", || schema_for!(TodoStatus)),
        ("FileChange", || schema_for!(FileChange)),
        ("FileChangeKind", || schema_for!(FileChangeKind)),
        ("FileChangeSource", || schema_for!(FileChangeSource)),
    ]
}

/// Permission request/response types.
fn permission_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("PermissionRequestParams", || {
            schema_for!(PermissionRequestParams)
        }),
        ("PermissionRespondParams", || {
            schema_for!(PermissionRespondParams)
        }),
        ("PermissionRespondResult", || {
            schema_for!(PermissionRespondResult)
        }),
        ("PermissionDecision", || schema_for!(PermissionDecision)),
    ]
}

/// Model listing types.
fn model_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("ModelListParams", || schema_for!(ModelListParams)),
        ("ModelListResult", || schema_for!(ModelListResult)),
        ("ModelEntry", || schema_for!(ModelEntry)),
        ("ModelSource", || schema_for!(ModelSource)),
    ]
}

/// Credential status types.
fn auth_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("AuthStatusParams", || schema_for!(AuthStatusParams)),
        ("AuthStatusResult", || schema_for!(AuthStatusResult)),
        ("CredentialInfo", || schema_for!(CredentialInfo)),
        ("CredentialKind", || schema_for!(CredentialKind)),
        ("CredentialSource", || schema_for!(CredentialSource)),
    ]
}

/// Provider usage quota types.
fn usage_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("ProviderUsageParams", || schema_for!(ProviderUsageParams)),
        ("ProviderUsageResult", || schema_for!(ProviderUsageResult)),
        ("UsageWindow", || schema_for!(UsageWindow)),
        ("UsageWindowKind", || schema_for!(UsageWindowKind)),
        ("UsageWindowStatus", || schema_for!(UsageWindowStatus)),
        ("ProviderUsageError", || schema_for!(ProviderUsageError)),
    ]
}

/// Device-code authorization types.
fn device_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("AuthDeviceStartParams", || {
            schema_for!(AuthDeviceStartParams)
        }),
        ("AuthDeviceStartResult", || {
            schema_for!(AuthDeviceStartResult)
        }),
        ("AuthDevicePollParams", || schema_for!(AuthDevicePollParams)),
        ("AuthDevicePollResult", || schema_for!(AuthDevicePollResult)),
        ("DeviceAuthState", || schema_for!(DeviceAuthState)),
    ]
}

/// Api-key link/clear types.
fn link_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("AuthSetApiKeyParams", || schema_for!(AuthSetApiKeyParams)),
        ("AuthSetApiKeyResult", || schema_for!(AuthSetApiKeyResult)),
        ("ApiKeyOutcome", || schema_for!(ApiKeyOutcome)),
        ("ApiKeyLinkError", || schema_for!(ApiKeyLinkError)),
        ("AuthClearCredentialParams", || {
            schema_for!(AuthClearCredentialParams)
        }),
        ("AuthClearCredentialResult", || {
            schema_for!(AuthClearCredentialResult)
        }),
    ]
}

/// File listing types.
fn file_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("FileListParams", || schema_for!(FileListParams)),
        ("FileListResult", || schema_for!(FileListResult)),
        ("FileEntry", || schema_for!(FileEntry)),
        ("FileKind", || schema_for!(FileKind)),
    ]
}

/// Kernel-to-client question types.
fn question_targets() -> Vec<(&'static str, SchemaFn)> {
    vec![
        ("AskQuestionParams", || schema_for!(AskQuestionParams)),
        ("AskOption", || schema_for!(AskOption)),
        ("QuestionRespondParams", || {
            schema_for!(QuestionRespondParams)
        }),
        ("QuestionRespondResult", || {
            schema_for!(QuestionRespondResult)
        }),
    ]
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
        for name in [
            "AuthSetApiKeyParams",
            "AuthSetApiKeyResult",
            "ApiKeyOutcome",
            "ApiKeyLinkError",
            "AuthClearCredentialParams",
            "AuthClearCredentialResult",
        ] {
            assert!(
                schemas.contains_key(name),
                "schema {name} is not registered"
            );
        }
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
