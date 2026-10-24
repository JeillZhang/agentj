//! `file/list` handler and its error-data mapping.
//!
//! Verbatim move out of `lib.rs`: the `file_list` sandbox call, the git status
//! attachment and every JSON-RPC error code (`-32807` OutsideRoot, `-32602`
//! InvalidDepth) are unchanged; only visibility widened for the route table.

use crate::{DispatchError, IpcServer, JsonRpcRequest, attach_git_statuses, file_list, to_wire};
use agentj_protocol::base::error_codes;
use agentj_protocol::file::FileListParams;
use serde_json::Value;
use std::path::PathBuf;

impl IpcServer {
    /// the session `project_root`, with git porcelain statuses per file entry.
    pub(crate) async fn dispatch_file_list(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params: FileListParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(format!("invalid file/list params: {e}")))?;
        if self.session_store.status(&params.session_id).is_err() {
            return Err(DispatchError {
                code: error_codes::SESSION_NOT_FOUND,
                message: "SessionNotFound".into(),
                data: None,
            });
        }
        let root = match self.session_store.project_root(&params.session_id) {
            Some(root) => root,
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        let query = params
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty());
        let result = match query {
            Some(q) => file_list::search(&root, params.path.as_deref(), q),
            None => file_list::list(&root, params.path.as_deref(), params.depth),
        };
        match result {
            Ok(result) => {
                let result = attach_git_statuses(result, &root).await;
                to_wire(result)
            }
            Err(e) => Err(map_file_list_error(e, &params.path)),
        }
    }
}

/// Map a `file/list` failure onto its JSON-RPC code + `error.data` shape
/// (design error handling).
fn map_file_list_error(
    error: file_list::FileListError,
    requested: &Option<String>,
) -> DispatchError {
    match error {
        file_list::FileListError::NotFound => DispatchError::invalid_params_with_data(
            "file/list: path not found",
            serde_json::json!({ "path": requested, "reason": "not_found" }),
        ),
        file_list::FileListError::NotADirectory => DispatchError::invalid_params_with_data(
            "file/list: path is not a directory",
            serde_json::json!({ "path": requested, "reason": "not_a_directory" }),
        ),
        file_list::FileListError::InvalidDepth => DispatchError::invalid_params_with_data(
            "file/list: depth must be within 1..=8",
            serde_json::json!({ "reason": "invalid_depth" }),
        ),
        file_list::FileListError::OutsideRoot { path } => DispatchError {
            code: error_codes::PATH_NOT_ALLOWED,
            message: "PathNotAllowed".into(),
            data: Some(serde_json::json!({ "path": path, "reason": "outside_project_root" })),
        },
        file_list::FileListError::Io { path, reason } => DispatchError::internal_with_data(
            "file/list: io error",
            serde_json::json!({ "path": path, "reason": reason }),
        ),
    }
}
