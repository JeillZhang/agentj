//! `dispatch` area tests.

use super::*;
use crate::test_support::stub_session;
use crate::*;
use agentj_core::kernel::Kernel;

#[test]
fn dispatch_error_is_constructible_from_outside_the_crate() {
    let e = DispatchError::invalid_params_with_data(
        "InvalidParams",
        serde_json::json!({"field": "provider"}),
    );
    assert_eq!(e.code, error_codes::INVALID_PARAMS);
    assert_eq!(e.data.unwrap()["field"], "provider");
    assert!(
        crate::tcp::TcpBindOptions::default()
            .provider_service
            .is_none()
    );
}

/// M1 regression: JSON-RPC 2.0 §2.1 — a notification (request without an
/// id, e.g. $/cancelRequest) must produce NO response line, while a
/// request WITH an id still gets its response.
#[tokio::test]
async fn id_less_notification_gets_no_response() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","method":"session/new"}"#)
        .await
        .unwrap();
    assert!(resp.is_empty(), "notification must not respond: {resp}");
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await
        .unwrap();
    assert!(!resp.is_empty(), "id'd request must respond");
    assert!(resp.contains("\"result\""));
}

/// T20: the optional `data` key is omitted from the wire when None (additive
/// discipline), and carried when a parameterized error sets it.
#[test]
fn error_object_omits_data_when_none() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: serde_json::json!(9),
        result: None,
        error: Some(JsonRpcError {
            code: -32602,
            message: "bad params".into(),
            data: None,
        }),
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert!(
        v["error"].get("data").is_none(),
        "data key must be omitted when None: {v}"
    );
    assert_eq!(v["error"]["code"], -32602);
    assert_eq!(v["error"]["message"], "bad params");

    let err = DispatchError::invalid_params_with_data(
        "bad root",
        serde_json::json!({ "path": "/x", "reason": "invalid_project_root" }),
    );
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["code"], -32602);
    assert_eq!(v["data"]["reason"], "invalid_project_root");
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":3,"method":"bad/method"}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_object());
    assert_eq!(v["error"]["code"], -32601); // Method not found
    assert_eq!(v["id"], 3, "error response must echo request id");
}

#[tokio::test]
async fn invalid_json_returns_parse_error() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("not json").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32700); // Parse error
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[tokio::test]
async fn schema_invalid_request_echoes_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":10,"params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], 10);
}

#[tokio::test]
async fn schema_invalid_request_echoes_string_id_with_invalid_request_code() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server
        .handle_request(r#"{"jsonrpc":"2.0","id":"abc","params":{}}"#)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600); // Invalid Request
    assert_eq!(v["id"], "abc");
}

#[tokio::test]
async fn non_object_json_returns_invalid_request_with_null_id() {
    let server = IpcServer::new(Kernel::new(), stub_session());
    let resp = server.handle_request("42").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32600);
    assert_eq!(v["id"], serde_json::Value::Null);
}

#[test]
fn request_id_extraction_accepts_only_jsonrpc_id_types() {
    let cases = [
        (
            serde_json::json!({"id": "abc"}),
            Some(serde_json::json!("abc")),
        ),
        (serde_json::json!({"id": 7}), Some(serde_json::json!(7))),
        (
            serde_json::json!({"id": null}),
            Some(serde_json::Value::Null),
        ),
        (serde_json::json!({}), None),
        (serde_json::json!({"id": {"x": 1}}), None),
        (serde_json::json!({"id": [1]}), None),
        (serde_json::json!({"id": true}), None),
        (serde_json::json!("42"), None),
    ];
    for (value, expected) in cases {
        assert_eq!(extract_request_id(&value), expected, "value: {value}");
    }
}

#[tokio::test]
async fn a_panicking_dispatch_becomes_an_internal_panic_error() {
    let err = isolate_panic("kernel/status", async { panic!("boom") })
        .await
        .expect_err("a panicking dispatch must not propagate");
    assert_eq!(err.code, -32603);
    assert_eq!(err.message, "InternalPanic");
    let data = err.data.expect("error.data carries the panic details");
    assert_eq!(data["method"], "kernel/status");
    assert_eq!(data["reason"], "boom");
}

#[tokio::test]
async fn a_normal_dispatch_passes_through_the_panic_guard() {
    let ok = isolate_panic("kernel/status", async { Ok(serde_json::json!({ "a": 1 })) })
        .await
        .expect("a successful dispatch keeps its result");
    assert_eq!(ok["a"], 1);
}

/// The route table must stay in lockstep with the protocol registry: the
/// client-callable methods of `protocol/methods.json` (minus the two that
/// are handled outside dispatch: `initialized` is a notification, `exit`
/// closes the transport) are exactly the methods `dispatch_request` routes.
#[test]
fn dispatch_table_matches_methods_json() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/methods.json"
    ))
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut expected: Vec<String> = json["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            matches!(
                m["direction"].as_str(),
                Some("client_to_server") | Some("both")
            )
        })
        .map(|m| m["method"].as_str().unwrap().to_string())
        .filter(|m| m != "initialized" && m != "exit")
        .collect();
    expected.sort();
    let mut actual: Vec<String> = DISPATCH_METHODS.iter().map(|s| s.to_string()).collect();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(DISPATCH_METHODS.len(), 24);
    let unique: std::collections::BTreeSet<&String> = actual.iter().collect();
    assert_eq!(
        actual.len(),
        unique.len(),
        "duplicate method in DISPATCH_METHODS"
    );
}

/// Every method the table advertises must have a real match arm; a method
/// added to `DISPATCH_METHODS` without an arm would answer
/// `method_not_found` while the table claimed it was routed.
#[test]
fn every_dispatch_method_appears_in_the_match() {
    let src = include_str!("../dispatch.rs");
    for method in DISPATCH_METHODS {
        assert!(
            src.contains(&format!("\"{method}\" =>")),
            "{method} has no match arm"
        );
    }
}

/// The three request-framing entry points live in `dispatch.rs`; `lib.rs`
/// keeps only the module declaration. Pinned so a later edit cannot quietly
/// grow the root file back.
#[test]
fn dispatch_functions_moved_out_of_lib() {
    for name in ["dispatch_request", "handle_request_from", "auth_gate"] {
        let needle = fn_needle(name);
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{name} still in lib.rs"
        );
        assert!(
            include_str!("../dispatch.rs").contains(&needle),
            "{name} missing"
        );
    }
}
