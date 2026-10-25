//! Session lifecycle handlers: `session/new`, list, status, archive, delete,
//! history, plus the shared status/root helpers only they use.
//!
//! Verbatim move out of `lib.rs` — signatures, error codes and control flow are
//! unchanged; only the visibility widened to `pub(crate)` so the route table in
//! `dispatch.rs` can reach them.

use crate::{
    DispatchError, IpcServer, JsonRpcRequest, history_events_filtered, is_valid_run_id, to_wire,
};
use agentj_core::types::SessionId;
use agentj_protocol::base::error_codes;
use agentj_protocol::session::{SessionIdParams, SessionNewParams, SessionStatusResult};
use serde_json::Value;

impl IpcServer {
    pub(crate) fn dispatch_session_new(
        &self,
        req: &JsonRpcRequest,
        conn_id: &str,
    ) -> Result<Value, DispatchError> {
        let requested = Self::resolve_project_root(req.params.as_ref())?;
        let effective = match &requested {
            Some(root) => root.clone(),
            None => std::env::current_dir()
                .and_then(|dir| dir.canonicalize())
                .map_err(|e| DispatchError::internal(e.to_string()))?,
        };
        let sid = self
            .session_store
            .create()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        if let Some(root) = requested
            && let Err(e) = self.session_store.set_project_root(&sid, root)
        {
            let _ = self.session_store.delete(&sid);
            return Err(DispatchError::internal_with_data(
                e.to_string(),
                serde_json::json!({ "session_id": sid, "reason": "project_root_persist_failed" }),
            ));
        }
        if let Ok(mut reg) = self.registry.write() {
            reg.set_session_owner(&sid, conn_id);
        }
        Ok(serde_json::json!({
            "session_id": sid,
            "project_root": effective.to_string_lossy(),
        }))
    }

    /// Validate `session/new {project_root}` (design item 6): absent params keep
    /// the legacy session (no root); when present the root must be an absolute
    /// existing directory, returned canonicalized.
    fn resolve_project_root(
        params: Option<&Value>,
    ) -> Result<Option<std::path::PathBuf>, DispatchError> {
        let Some(raw) = params else {
            return Ok(None);
        };
        let parsed: SessionNewParams = serde_json::from_value(raw.clone()).map_err(|_| {
            let bad = raw.get("project_root").unwrap_or(&Value::Null);
            Self::invalid_project_root(bad)
        })?;
        let Some(root) = parsed.project_root else {
            return Ok(None);
        };
        let path = std::path::PathBuf::from(&root);
        if !path.is_absolute() {
            return Err(Self::invalid_project_root(&Value::String(root)));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| Self::invalid_project_root(&Value::String(root.clone())))?;
        if !canonical.is_dir() {
            return Err(Self::invalid_project_root(&Value::String(root)));
        }
        Ok(Some(canonical))
    }

    /// `-32602` + `error.data {path, reason}` for an invalid `session/new`
    /// `project_root` (design item 7).
    fn invalid_project_root(path: &Value) -> DispatchError {
        DispatchError::invalid_params_with_data(
            "invalid project_root: must be an absolute existing directory",
            serde_json::json!({ "path": path, "reason": "invalid_project_root" }),
        )
    }

    pub(crate) fn dispatch_session_list(&self) -> Result<Value, DispatchError> {
        let entries = self
            .session_store
            .list()
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        let sessions: Vec<Value> = entries
            .iter()
            .map(|e| {
                let state = match e.status {
                    agentj_core::module::SessionStatus::Active => "active",
                    agentj_core::module::SessionStatus::Archived => "archived",
                    agentj_core::module::SessionStatus::Deleted => "deleted",
                };
                let mut obj = serde_json::json!({
                    "session_id": e.session_id,
                    "updated_at": e.updated_at,
                    "state": state,
                });
                if let Some(p) = &e.preview {
                    obj["preview"] = serde_json::json!(p);
                }
                obj
            })
            .collect();
        Ok(serde_json::json!({ "sessions": sessions }))
    }

    /// Wire mapping shared by `session/status` / `archive` / `delete`.
    fn wire_session_status(
        status: agentj_core::module::SessionStatus,
    ) -> agentj_protocol::types::SessionStatus {
        use agentj_protocol::types::SessionStatus as WireStatus;

        match status {
            agentj_core::module::SessionStatus::Active => WireStatus::Active,
            agentj_core::module::SessionStatus::Archived => WireStatus::Archived,
            agentj_core::module::SessionStatus::Deleted => WireStatus::Deleted,
        }
    }

    /// Shared existence probe: unknown sessions map to `-32802 SessionNotFound`.
    fn require_session_status(
        &self,
        session_id: &SessionId,
    ) -> Result<agentj_core::module::SessionStatus, DispatchError> {
        self.session_store
            .status(session_id)
            .map_err(|_| DispatchError {
                code: error_codes::SESSION_NOT_FOUND,
                message: "SessionNotFound".into(),
                data: Some(serde_json::json!({ "session_id": session_id })),
            })
    }

    /// `-32803 SessionInactive` with the offending `{session_id, status}` data.
    fn session_inactive(
        session_id: &SessionId,
        status: agentj_core::module::SessionStatus,
    ) -> DispatchError {
        DispatchError {
            code: error_codes::SESSION_INACTIVE,
            message: "SessionInactive".into(),
            data: Some(serde_json::json!({
                "session_id": session_id,
                "status": Self::wire_session_status(status),
            })),
        }
    }

    /// `session/status` (spec §8.8): the existence probe clients fire before
    /// `session/subscribe`.
    pub(crate) fn dispatch_session_status(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        let project_root = self
            .session_store
            .project_root(&params.session_id)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|d| d.canonicalize().ok())
            })
            .map(|p| p.to_string_lossy().into_owned());
        let result = SessionStatusResult {
            status: Self::wire_session_status(status),
            project_root,
        };
        to_wire(result)
    }

    /// `session/archive` (spec §8.9): only an active session may be archived.
    pub(crate) fn dispatch_session_archive(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        if status != agentj_core::module::SessionStatus::Active {
            return Err(Self::session_inactive(&params.session_id, status));
        }
        self.session_store
            .archive(&params.session_id)
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        Ok(Value::Null)
    }

    /// `session/delete` (spec §8.10): archived sessions may be deleted;
    /// deleting a deleted session is `-32803 SessionInactive`.
    pub(crate) fn dispatch_session_delete(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params: SessionIdParams = serde_json::from_value(
            req.params
                .clone()
                .ok_or_else(|| DispatchError::invalid_params("missing params"))?,
        )
        .map_err(|e| DispatchError::invalid_params(e.to_string()))?;
        let status = self.require_session_status(&params.session_id)?;
        if status == agentj_core::module::SessionStatus::Deleted {
            return Err(Self::session_inactive(&params.session_id, status));
        }
        self.session_store
            .delete(&params.session_id)
            .map_err(|e| DispatchError::internal(e.to_string()))?;
        Ok(Value::Null)
    }

    pub(crate) fn dispatch_session_history(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<Value, DispatchError> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| DispatchError::invalid_params("missing params"))?;
        let sid = params["session_id"]
            .as_str()
            .ok_or_else(|| DispatchError::invalid_params("missing session_id"))?;
        let raw_run_id = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(rid) = &raw_run_id
            && !is_valid_run_id(rid)
        {
            return Err(DispatchError::invalid_params_with_data(
                "InvalidParams",
                serde_json::json!({"reason": "invalid_run_id"}),
            ));
        }
        let limit = params["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
        let cursor = params["cursor"].as_u64();
        let (events, next) = match &self.trajectory_store {
            Some(store) => {
                history_events_filtered(store, sid, raw_run_id.as_deref(), cursor, limit)
                    .map_err(|e| DispatchError::internal(e.to_string()))?
            }
            None => (vec![], None),
        };
        let result = agentj_protocol::session::SessionHistoryResult {
            events,
            next_cursor: next,
        };
        to_wire(result)
    }
}
