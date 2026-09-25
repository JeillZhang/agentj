// agentj-protocol/tests/registry.rs
use std::fs;
use std::path::Path;

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn registry_lists_all_v01_methods() {
    let reg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("protocol/methods.json")).unwrap(),
    )
    .unwrap();
    let methods = reg["methods"].as_array().unwrap();
    let names: Vec<&str> = methods
        .iter()
        .map(|m| m["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods.len(), 21, "unexpected method count");
    for expected in [
        "initialize",
        "initialized",
        "shutdown",
        "exit",
        "session/new",
        "session/run",
        "session/status",
        "session/archive",
        "session/delete",
        "kernel/status",
        "$/cancelRequest",
        "$/progress",
        "$/permissionRequest",
        "permission/respond",
        "session/subscribe",
        "session/unsubscribe",
        "session/list",
        "session/history",
        "file/list",
        "$/askQuestion",
        "question/respond",
    ] {
        assert!(names.contains(&expected), "missing method {expected}");
    }
}

#[test]
fn registry_extension_method_shapes() {
    let reg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("protocol/methods.json")).unwrap(),
    )
    .unwrap();
    let methods = reg["methods"].as_array().unwrap();
    let find = |name: &str| {
        methods
            .iter()
            .find(|m| m["method"] == name)
            .unwrap_or_else(|| panic!("missing method {name}"))
    };

    let file_list = find("file/list");
    assert_eq!(file_list["kind"], "request");
    assert_eq!(file_list["direction"], "client_to_server");
    assert_eq!(file_list["params"], "FileListParams");
    assert_eq!(file_list["result"], "FileListResult");

    let ask = find("$/askQuestion");
    assert_eq!(ask["kind"], "notification");
    assert_eq!(ask["direction"], "server_to_client");
    assert_eq!(ask["params"], "AskQuestionParams");
    assert!(ask["result"].is_null());

    let respond = find("question/respond");
    assert_eq!(respond["kind"], "request");
    assert_eq!(respond["direction"], "client_to_server");
    assert_eq!(respond["params"], "QuestionRespondParams");
    assert_eq!(respond["result"], "QuestionRespondResult");

    assert_eq!(find("session/new")["params"], "SessionNewParams");
    assert_eq!(reg["protocol_version"], "0.15.0");
}

#[test]
fn registry_schema_refs_resolve_to_committed_files() {
    let reg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("protocol/methods.json")).unwrap(),
    )
    .unwrap();
    for m in reg["methods"].as_array().unwrap() {
        for field in ["params", "result"] {
            if let Some(ty) = m.get(field).and_then(|v| v.as_str()) {
                let path = repo_root().join(format!("schemas/{ty}.json"));
                assert!(
                    path.exists(),
                    "schema ref {ty} (method {}) missing",
                    m["method"]
                );
            }
        }
    }
}

#[test]
fn registry_directions_are_valid() {
    let reg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("protocol/methods.json")).unwrap(),
    )
    .unwrap();
    for m in reg["methods"].as_array().unwrap() {
        let dir = m["direction"].as_str().unwrap();
        assert!(
            dir == "client_to_server" || dir == "server_to_client" || dir == "both",
            "bad direction for {}",
            m["method"]
        );
    }
}
