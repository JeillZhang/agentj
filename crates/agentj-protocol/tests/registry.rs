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
    assert_eq!(methods.len(), 29, "unexpected method count");
    let find = |name: &str| {
        methods
            .iter()
            .find(|m| m["method"] == name)
            .unwrap_or_else(|| panic!("missing method {name}"))
    };
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
        "model/list",
        "auth/status",
        "provider/usage",
        "auth/deviceStart",
        "auth/devicePoll",
    ] {
        find(expected);
    }
    for (name, params, result) in [
        (
            "auth/setApiKey",
            "AuthSetApiKeyParams",
            "AuthSetApiKeyResult",
        ),
        (
            "auth/clearCredential",
            "AuthClearCredentialParams",
            "AuthClearCredentialResult",
        ),
        (
            "provider/usageHistory",
            "ProviderUsageHistoryParams",
            "ProviderUsageHistoryResult",
        ),
    ] {
        let m = find(name);
        assert_eq!(m["kind"], "request", "{name} kind");
        assert_eq!(m["direction"], "client_to_server", "{name} direction");
        assert_eq!(m["params"], params, "{name} params");
        assert_eq!(m["result"], result, "{name} result");
    }
    assert_eq!(reg["protocol_version"], "0.20.0");
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

    let set_key = find("auth/setApiKey");
    assert_eq!(set_key["kind"], "request");
    assert_eq!(set_key["direction"], "client_to_server");
    assert_eq!(set_key["params"], "AuthSetApiKeyParams");
    assert_eq!(set_key["result"], "AuthSetApiKeyResult");

    let clear = find("auth/clearCredential");
    assert_eq!(clear["kind"], "request");
    assert_eq!(clear["direction"], "client_to_server");
    assert_eq!(clear["params"], "AuthClearCredentialParams");
    assert_eq!(clear["result"], "AuthClearCredentialResult");

    assert_eq!(find("session/new")["params"], "SessionNewParams");
    assert_eq!(reg["protocol_version"], "0.20.0");
}

#[test]
fn protocol_version_and_new_methods_are_registered() {
    let raw = include_str!("../../../protocol/methods.json");
    let reg: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(reg["protocol_version"], "0.20.0");
    let methods = reg["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 29, "unexpected method count");
    let names: Vec<&str> = methods
        .iter()
        .map(|m| m["method"].as_str().unwrap())
        .collect();
    for expected in [
        "model/list",
        "auth/status",
        "provider/usage",
        "auth/deviceStart",
        "auth/devicePoll",
        "auth/setApiKey",
        "auth/clearCredential",
    ] {
        assert!(names.contains(&expected), "missing method {expected}");
    }
    for m in methods.iter().filter(|m| {
        let name = m["method"].as_str().unwrap();
        name.starts_with("auth/") || name.starts_with("model/") || name == "provider/usage"
    }) {
        assert_eq!(m["kind"], "request");
        assert_eq!(m["direction"], "client_to_server");
    }
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

#[test]
fn protocol_version_is_additive_minor() {
    // Pin the const against a literal: reverting `PROTOCOL_VERSION` must fail the suite,
    // not stay green because every other assertion compares against the const itself.
    assert_eq!(agentj_protocol::PROTOCOL_VERSION, "0.20.0");
}

#[test]
fn registry_version_matches_the_constant() {
    let reg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("protocol/methods.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        reg["protocol_version"].as_str().unwrap(),
        agentj_protocol::PROTOCOL_VERSION
    );
}
