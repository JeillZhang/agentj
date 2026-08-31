// agentj-protocol/tests/contract.rs
use agentj_protocol::base::{
    Id, NotificationMessage, ProgressToken, RequestMessage, ResponseMessage,
};
use agentj_protocol::lifecycle::{ClientInfo, InitializeParams, InitializeResult, ServerInfo};
use agentj_protocol::permission::PermissionRequestParams;
use agentj_protocol::session::{
    SessionNewResult, SessionRunParams, SessionRunResult, SessionSubscribeParams,
};
use agentj_protocol::streaming::{ProgressParams, ProgressValue};
use agentj_protocol::types::ToolCall;

#[test]
fn initialize_request_wire_shape() {
    let req = RequestMessage {
        jsonrpc: "2.0".into(),
        id: Id::Number(1),
        method: "initialize".into(),
        params: Some(
            serde_json::to_value(InitializeParams {
                protocol_version: "0.1.0".into(),
                capabilities: Some(serde_json::json!({"streaming": true})),
                client_info: Some(ClientInfo {
                    name: "cli".into(),
                    version: "0.1.0".into(),
                }),
                token: None,
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(req).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
    assert_eq!(json["method"], "initialize");
    assert_eq!(json["params"]["protocol_version"], "0.1.0");
    assert_eq!(json["params"]["client_info"]["name"], "cli");
}

#[test]
fn initialize_response_wire_shape() {
    let resp = ResponseMessage::Success {
        jsonrpc: "2.0".into(),
        id: Id::Number(1),
        result: serde_json::to_value(InitializeResult {
            protocol_version: "0.1.0".into(),
            capabilities: Some(serde_json::json!({"streaming": true})),
            server_info: ServerInfo {
                name: "agentj".into(),
                version: "0.1.0".into(),
            },
            connection_id: None,
        })
        .unwrap(),
    };
    let json = serde_json::to_value(resp).unwrap();
    assert_eq!(json["result"]["server_info"]["name"], "agentj");
}

#[test]
fn session_run_request_wire_shape() {
    let req = RequestMessage {
        jsonrpc: "2.0".into(),
        id: Id::Str("req-1".into()),
        method: "session/run".into(),
        params: Some(
            serde_json::to_value(SessionRunParams {
                session_id: "s1".into(),
                content: "hi".into(),
                attachments: None,
                progress_token: Some(ProgressToken::Str("tok-1".into())),
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(req).unwrap();
    assert_eq!(json["params"]["progress_token"], "tok-1");
}

#[test]
fn session_new_response_wire_shape() {
    let resp = ResponseMessage::Success {
        jsonrpc: "2.0".into(),
        id: Id::Number(2),
        result: serde_json::to_value(SessionNewResult {
            session_id: "s1".into(),
        })
        .unwrap(),
    };
    let json = serde_json::to_value(resp).unwrap();
    assert_eq!(json["result"]["session_id"], "s1");
}

#[test]
fn progress_notification_wire_shape() {
    let n = NotificationMessage {
        jsonrpc: "2.0".into(),
        method: "$/progress".into(),
        params: Some(
            serde_json::to_value(ProgressParams {
                token: ProgressToken::Str("tok-1".into()),
                value: ProgressValue::Text {
                    delta: "hel".into(),
                },
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(n).unwrap();
    assert_eq!(json["method"], "$/progress");
    assert_eq!(json["params"]["value"]["kind"], "text");
    assert_eq!(json["params"]["value"]["delta"], "hel");
}

#[test]
fn tool_call_progress_wire_shape() {
    let n = NotificationMessage {
        jsonrpc: "2.0".into(),
        method: "$/progress".into(),
        params: Some(
            serde_json::to_value(ProgressParams {
                token: ProgressToken::Number(9),
                value: ProgressValue::ToolCall {
                    tool_call: ToolCall {
                        id: "c1".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({}),
                    },
                },
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(n).unwrap();
    assert_eq!(json["params"]["value"]["kind"], "tool_call");
    assert_eq!(json["params"]["value"]["tool_call"]["name"], "bash");
}

#[test]
fn session_run_result_roundtrip() {
    let r = SessionRunResult {
        content: "done".into(),
        tool_calls: None,
    };
    let back: SessionRunResult = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back.content, "done");
    assert!(back.tool_calls.is_none());
}

#[test]
fn session_subscribe_request_wire_shape() {
    let req = RequestMessage {
        jsonrpc: "2.0".into(),
        id: Id::Number(11),
        method: "session/subscribe".into(),
        params: Some(
            serde_json::to_value(SessionSubscribeParams {
                session_id: "s1".into(),
                progress_token: Some(ProgressToken::Str("tok-sub".into())),
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(req).unwrap();
    assert_eq!(json["method"], "session/subscribe");
    assert_eq!(json["params"]["progress_token"], "tok-sub");
}

#[test]
fn permission_request_notification_wire_shape_with_origin_client() {
    let n = NotificationMessage {
        jsonrpc: "2.0".into(),
        method: "$/permissionRequest".into(),
        params: Some(
            serde_json::to_value(PermissionRequestParams {
                request_id: "req-9".into(),
                tool: "bash".into(),
                args: serde_json::json!({"command": "ls"}),
                reason: "tool default ask".into(),
                origin_client: Some("conn-a".into()),
                session_id: Some("s-1".into()),
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(n).unwrap();
    assert_eq!(json["method"], "$/permissionRequest");
    assert_eq!(json["params"]["origin_client"], "conn-a");
    assert_eq!(json["params"]["session_id"], "s-1");
}
