//! `file/list` wire types (design 决策 #3 roots + `-32807` sandbox). @since 0.13.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::SessionId;

/// `file/list` params — enumerate a directory relative to the session
/// `project_root`. @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileListParams {
    pub session_id: SessionId,
    /// Directory path relative to the project root; absent = root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Enumeration depth 1..=8 (default 1, server-validated). @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Case-insensitive substring filter; empty/whitespace is treated as absent. @since 0.14.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// `file/list` result — entries capped at 2000; `truncated` flags the cap.
/// @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileListResult {
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
    /// Full match count discovered by a query scan (lower bound when truncated).
    /// @since 0.14.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// One directory entry (`path` relative to the project root, no `./` prefix).
/// @since 0.13.0
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileEntry {
    /// Entry name without separators.
    pub name: String,
    /// Path relative to the project root.
    pub path: String,
    pub kind: FileKind,
    /// File size in bytes; `None` for directories. @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Modification time in epoch milliseconds. @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<u64>,
    /// Porcelain single-letter status; `None` = clean / not a git repo.
    /// @since 0.13.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitFileStatus>,
}

/// Entry kind. `Unknown` tolerates forward values (serde(other)); the schema
/// lists only the known values. @since 0.13.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Dir,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

/// Porcelain single-letter git status (M/A/U/D). `Unknown` tolerates forward
/// values (serde(other)); the schema lists only the known letters. @since 0.13.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum GitFileStatus {
    #[serde(rename = "M")]
    M,
    #[serde(rename = "A")]
    A,
    #[serde(rename = "U")]
    U,
    #[serde(rename = "D")]
    D,
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_list_params_roundtrip_omits_none() {
        let p = FileListParams {
            session_id: "s1".into(),
            path: None,
            depth: Some(2),
            query: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["session_id"], "s1");
        assert_eq!(v["depth"], 2);
        assert!(v.get("path").is_none());
    }

    #[test]
    fn file_list_params_query_is_additive() {
        let legacy: FileListParams =
            serde_json::from_value(serde_json::json!({"session_id": "s1"})).unwrap();
        assert!(legacy.query.is_none());
        let with_query = FileListParams {
            session_id: "s1".into(),
            path: None,
            depth: None,
            query: Some("src".into()),
        };
        let json = serde_json::to_value(&with_query).unwrap();
        assert_eq!(json["query"], "src");
        let none = FileListParams {
            session_id: "s1".into(),
            path: None,
            depth: None,
            query: None,
        };
        assert!(serde_json::to_value(none).unwrap().get("query").is_none());
    }

    #[test]
    fn file_kind_unknown_tolerated() {
        let k: FileKind = serde_json::from_str("\"socket\"").unwrap();
        assert_eq!(k, FileKind::Unknown);
    }

    #[test]
    fn file_kind_serializes_snake_case() {
        assert_eq!(serde_json::to_value(FileKind::File).unwrap(), "file");
        assert_eq!(serde_json::to_value(FileKind::Dir).unwrap(), "dir");
        let back: FileKind = serde_json::from_str("\"dir\"").unwrap();
        assert_eq!(back, FileKind::Dir);
    }

    #[test]
    fn git_file_status_maps_single_letters() {
        let s = serde_json::to_value(GitFileStatus::U).unwrap();
        assert_eq!(s, "U");
    }

    #[test]
    fn git_file_status_roundtrips_all_letters_and_tolerates_unknown() {
        for (status, letter) in [
            (GitFileStatus::M, "M"),
            (GitFileStatus::A, "A"),
            (GitFileStatus::U, "U"),
            (GitFileStatus::D, "D"),
        ] {
            let v = serde_json::to_value(status).unwrap();
            assert_eq!(v, letter);
            let back: GitFileStatus = serde_json::from_value(v).unwrap();
            assert_eq!(back, status);
        }
        let unknown: GitFileStatus = serde_json::from_str("\"X\"").unwrap();
        assert_eq!(unknown, GitFileStatus::Unknown);
    }

    #[test]
    fn file_entry_omits_none_optionals() {
        let e = FileEntry {
            name: "main.rs".into(),
            path: "src/main.rs".into(),
            kind: FileKind::File,
            size: Some(42),
            mtime_ms: None,
            git_status: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["name"], "main.rs");
        assert_eq!(v["path"], "src/main.rs");
        assert_eq!(v["kind"], "file");
        assert_eq!(v["size"], 42);
        assert!(v.get("mtime_ms").is_none());
        assert!(v.get("git_status").is_none());
        let back: FileEntry = serde_json::from_value(v).unwrap();
        assert_eq!(back.size, Some(42));
        assert!(back.mtime_ms.is_none() && back.git_status.is_none());
    }

    #[test]
    fn file_entry_dir_null_size_and_git_status_null_serde() {
        let e = FileEntry {
            name: "src".into(),
            path: "src".into(),
            kind: FileKind::Dir,
            size: None,
            mtime_ms: Some(1_700_000_000_000),
            git_status: Some(GitFileStatus::M),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "dir");
        assert!(v.get("size").is_none());
        assert_eq!(v["mtime_ms"], 1_700_000_000_000u64);
        assert_eq!(v["git_status"], "M");
    }

    #[test]
    fn file_list_result_roundtrip() {
        let r = FileListResult {
            entries: vec![FileEntry {
                name: "lib.rs".into(),
                path: "src/lib.rs".into(),
                kind: FileKind::File,
                size: None,
                mtime_ms: None,
                git_status: Some(GitFileStatus::A),
            }],
            truncated: true,
            total: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["entries"][0]["path"], "src/lib.rs");
        assert_eq!(v["entries"][0]["git_status"], "A");
        assert_eq!(v["truncated"], true);
        let back: FileListResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert!(back.truncated);
    }

    #[test]
    fn file_list_result_total_is_omitted_when_none() {
        let none = FileListResult {
            entries: vec![],
            truncated: false,
            total: None,
        };
        assert!(serde_json::to_value(&none).unwrap().get("total").is_none());
        let some = FileListResult {
            entries: vec![],
            truncated: false,
            total: Some(7),
        };
        assert_eq!(serde_json::to_value(some).unwrap()["total"], 7);
    }

    #[test]
    fn file_types_generate_json_schema() {
        let params = serde_json::to_value(schemars::schema_for!(FileListParams)).unwrap();
        assert_eq!(params["type"], "object");
        for key in ["session_id", "path", "depth"] {
            assert!(params["properties"].get(key).is_some(), "missing {key}");
        }
        let status = serde_json::to_value(schemars::schema_for!(GitFileStatus)).unwrap();
        let values = status["enum"].as_array().unwrap();
        assert_eq!(values.len(), 4, "Unknown must be schemars(skip)");
        let entry = serde_json::to_value(schemars::schema_for!(FileEntry)).unwrap();
        assert!(entry["properties"].get("git_status").is_some());
    }
}
