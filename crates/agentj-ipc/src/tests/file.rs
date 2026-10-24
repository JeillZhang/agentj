//! `file` area tests.

use super::*;
use crate::*;
use agentj_core::kernel::Kernel;
use agentj_session::InMemoryStore;
use std::sync::Arc;

/// Temp project root for the T21 `file/list` dispatch tests (std-only).
fn temp_dir_filelist(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aj-ipc-filedisp-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One isolated session whose `project_root` is `root`.
fn file_list_server(root: &std::path::Path) -> (IpcServer, String) {
    let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::new());
    let sid = store.create().unwrap();
    store
        .set_project_root(&sid, root.canonicalize().unwrap())
        .unwrap();
    let session: Arc<dyn SessionStore> = store;
    (IpcServer::new(Kernel::new(), session), sid)
}

/// Dispatch one `file/list` request and return the parsed JSON-RPC response.
async fn file_list_call(
    server: &IpcServer,
    sid: &str,
    path: Option<&str>,
    depth: Option<u32>,
) -> serde_json::Value {
    let mut params = serde_json::json!({ "session_id": sid });
    if let Some(p) = path {
        params["path"] = serde_json::json!(p);
    }
    if let Some(d) = depth {
        params["depth"] = serde_json::json!(d);
    }
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "file/list",
        "params": params,
    })
    .to_string();
    let resp = server.handle_request(&raw).await.unwrap();
    serde_json::from_str(&resp).unwrap()
}

/// Query variant of `file_list_call` (the shared helper has no query param).
async fn file_list_search_call(
    server: &IpcServer,
    sid: &str,
    path: Option<&str>,
    query: &str,
) -> serde_json::Value {
    let raw = serde_json::json!({
        "jsonrpc": "2.0", "id": 22, "method": "file/list",
        "params": { "session_id": sid, "path": path, "query": query },
    })
    .to_string();
    serde_json::from_str(&server.handle_request(&raw).await.unwrap()).unwrap()
}

#[tokio::test]
async fn dispatch_file_list_query_returns_ranked_matches_with_total() {
    let root = temp_dir_filelist("query");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "x").unwrap();
    std::fs::write(root.join("README.md"), "x").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_search_call(&server, &sid, None, "main").await;
    assert!(v["error"].is_null(), "query call must succeed: {v}");
    assert_eq!(v["result"]["total"], 1);
    assert_eq!(v["result"]["entries"][0]["path"], "src/main.rs");
    let blank = file_list_search_call(&server, &sid, None, "   ").await;
    assert!(
        blank["result"].get("total").is_none(),
        "blank query = legacy listing"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn dispatch_file_list_lists_root_sorted_excluding_git() {
    let root = temp_dir_filelist("root");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join("a.rs"), "hi").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "valid call must succeed: {v}");
    let entries = v["result"]["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["src", "a.rs"], "dirs first, .git excluded");
    assert_eq!(entries[0]["kind"], "dir");
    assert_eq!(entries[1]["kind"], "file");
    assert_eq!(entries[1]["size"], 2);
    assert_eq!(v["result"]["truncated"], false);
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn dispatch_file_list_maps_errors_to_codes_and_data() {
    let root = temp_dir_filelist("errors");
    std::fs::write(root.join("file.txt"), "x").unwrap();
    let outside = temp_dir_filelist("errors-outside");
    let (server, sid) = file_list_server(&root);

    let v = file_list_call(&server, &sid, Some("../x"), None).await;
    assert_eq!(v["error"]["code"], -32807, "dotdot escape: {v}");
    assert_eq!(v["error"]["data"]["path"], "../x");
    assert_eq!(v["error"]["data"]["reason"], "outside_project_root");

    let abs = outside.to_string_lossy().into_owned();
    let v = file_list_call(&server, &sid, Some(&abs), None).await;
    assert_eq!(v["error"]["code"], -32807, "absolute escape: {v}");
    assert_eq!(v["error"]["data"]["path"], abs);

    let v = file_list_call(&server, &sid, Some("file.txt"), None).await;
    assert_eq!(v["error"]["code"], -32602, "file path: {v}");
    assert_eq!(v["error"]["data"]["path"], "file.txt");
    assert_eq!(v["error"]["data"]["reason"], "not_a_directory");

    let v = file_list_call(&server, &sid, Some("nope"), None).await;
    assert_eq!(v["error"]["code"], -32602, "missing path: {v}");
    assert_eq!(v["error"]["data"]["path"], "nope");
    assert_eq!(v["error"]["data"]["reason"], "not_found");

    let v = file_list_call(&server, &sid, None, Some(9)).await;
    assert_eq!(v["error"]["code"], -32602, "depth 9: {v}");
    assert_eq!(v["error"]["data"]["reason"], "invalid_depth");

    let v = file_list_call(&server, "missing-session", None, None).await;
    assert_eq!(v["error"]["code"], -32802, "unknown session: {v}");

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
}

/// Run a git command in `root`, panicking on any failure (git is available
/// wherever the `git_sample` suite runs).
fn git_run(root: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("git unavailable: {e}"),
    }
}

/// AC: a git repo with modified / untracked / deleted files maps to M/A/D.
/// The deleted entry stays on disk via `git rm --cached` + `.gitignore`,
/// otherwise it would not be reachable through a directory listing.
#[tokio::test]
async fn dispatch_file_list_reports_git_statuses() {
    let root = temp_dir_filelist("git");
    git_run(&root, &["init", "-q"]);
    std::fs::write(root.join("m.txt"), "1\n").unwrap();
    std::fs::write(root.join("d.txt"), "1\n").unwrap();
    git_run(&root, &["add", "m.txt", "d.txt"]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::write(root.join("m.txt"), "1\n2\n").unwrap();
    std::fs::write(root.join("a.txt"), "new\n").unwrap();
    git_run(&root, &["rm", "--cached", "-q", "d.txt"]);
    std::fs::write(root.join(".gitignore"), "d.txt\n").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entry = |name: &str| -> serde_json::Value {
        v["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    assert_eq!(entry("m.txt")["git_status"], "M", "modified: {v}");
    assert_eq!(entry("a.txt")["git_status"], "A", "untracked: {v}");
    assert_eq!(entry("d.txt")["git_status"], "D", "staged delete: {v}");
    std::fs::remove_dir_all(&root).ok();
}

/// AC: a worktree deletion below `src` surfaces as the aggregate `D` on the
/// `src` directory entry even though the deleted file is not listed.
#[tokio::test]
async fn dispatch_file_list_reports_aggregate_dir_status() {
    let root = temp_dir_filelist("git-dir-agg");
    git_run(&root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("src/deep/del.txt"), "1\n").unwrap();
    git_run(&root, &["add", "."]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::remove_file(root.join("src/deep/del.txt")).unwrap(); // worktree delete -> ' D'
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, None, None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entry = |name: &str| -> serde_json::Value {
        v["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    assert_eq!(
        entry("src")["git_status"],
        "D",
        "deleted descendant surfaces on the dir: {v}"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// Subdirectory listings keep entry paths root-relative so the git lookup
/// (snapshot keys are root-relative) still reports M/A/D.
#[tokio::test]
async fn dispatch_file_list_subdirectory_git_status_is_root_relative() {
    let root = temp_dir_filelist("git-sub");
    git_run(&root, &["init", "-q"]);
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/m.txt"), "1\n").unwrap();
    git_run(&root, &["add", "src/m.txt"]);
    git_run(
        &root,
        &[
            "-c",
            "user.name=AgentJ",
            "-c",
            "user.email=agentj@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::write(root.join("src/m.txt"), "1\n2\n").unwrap();
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, Some("src"), None).await;
    assert!(v["error"].is_null(), "call must succeed: {v}");
    let entries = v["result"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "one file under src: {v}");
    assert_eq!(
        entries[0]["path"], "src/m.txt",
        "entry path must stay root-relative: {v}"
    );
    assert_eq!(
        entries[0]["git_status"], "M",
        "root-relative git lookup must hit: {v}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn dispatch_file_list_unreadable_directory_is_internal_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir_filelist("unreadable");
    let locked = root.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        // Privileged runner: permission bits are not enforced → unexercisable here.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).ok();
        return;
    }
    let (server, sid) = file_list_server(&root);
    let v = file_list_call(&server, &sid, Some("locked"), None).await;
    assert_eq!(v["error"]["code"], -32603, "unreadable dir: {v}");
    assert_eq!(v["error"]["data"]["path"], "locked");
    assert!(
        v["error"]["data"]["reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "io reason must be carried: {v}"
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&root).ok();
}

/// `file/list` and its error mapping live in `handlers/file.rs`.
#[test]
fn file_handlers_moved_out_of_lib() {
    for name in ["dispatch_file_list", "map_file_list_error"] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../handlers/file.rs").contains(&needle),
            "{name} missing from file.rs"
        );
    }
}
