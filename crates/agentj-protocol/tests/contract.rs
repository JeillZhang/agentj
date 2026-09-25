// agentj-protocol/tests/contract.rs
use agentj_protocol::base::{
    Id, NotificationMessage, ProgressToken, RequestMessage, ResponseMessage,
};
use agentj_protocol::file::FileListParams;
use agentj_protocol::lifecycle::{ClientInfo, InitializeParams, InitializeResult, ServerInfo};
use agentj_protocol::permission::PermissionRequestParams;
use agentj_protocol::question::{
    AskOption, AskQuestionParams, QuestionRespondParams, QuestionRespondResult,
};
use agentj_protocol::session::{
    SessionNewResult, SessionRunParams, SessionRunResult, SessionSubscribeParams, UsageInfo,
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
            project_root: None,
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
        usage: None,
        run_id: None,
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
fn session_run_result_usage_serializes_when_present() {
    let r = SessionRunResult {
        content: "done".into(),
        tool_calls: None,
        usage: Some(UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 20,
        }),
        run_id: None,
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["usage"]["prompt_tokens"], 100);
    assert_eq!(json["usage"]["completion_tokens"], 20);
}

#[test]
fn session_run_result_usage_absent_when_none() {
    let r = SessionRunResult {
        content: "done".into(),
        tool_calls: None,
        usage: None,
        run_id: None,
    };
    let json = serde_json::to_value(&r).unwrap();
    assert!(json.get("usage").is_none());
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

#[test]
fn file_list_request_wire_shape() {
    let req = RequestMessage {
        jsonrpc: "2.0".into(),
        id: Id::Number(1),
        method: "file/list".into(),
        params: Some(
            serde_json::to_value(FileListParams {
                session_id: "s1".into(),
                path: Some("src".into()),
                depth: Some(1),
                query: None,
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(req).unwrap();
    assert_eq!(json["method"], "file/list");
    assert_eq!(json["params"]["session_id"], "s1");
    assert_eq!(json["params"]["path"], "src");
    assert_eq!(json["params"]["depth"], 1);
}

#[test]
fn ask_question_notification_wire_shape() {
    let n = NotificationMessage {
        jsonrpc: "2.0".into(),
        method: "$/askQuestion".into(),
        params: Some(
            serde_json::to_value(AskQuestionParams {
                question_id: "q-1".into(),
                session_id: "s1".into(),
                question: "Which color?".into(),
                header: None,
                options: vec![
                    AskOption {
                        id: "red".into(),
                        label: "Red".into(),
                        hint: None,
                    },
                    AskOption {
                        id: "blue".into(),
                        label: "Blue".into(),
                        hint: Some("cool".into()),
                    },
                ],
                allow_multiple: false,
                allow_other: true,
                timeout_s: None,
            })
            .unwrap(),
        ),
    };
    let json = serde_json::to_value(n).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["method"], "$/askQuestion");
    assert_eq!(json["params"]["options"].as_array().unwrap().len(), 2);
    assert_eq!(json["params"]["allow_multiple"], false);
    assert_eq!(json["params"]["allow_other"], true);
    assert!(json["params"].get("header").is_none());
    assert!(json["params"].get("timeout_s").is_none());
}

#[test]
fn question_respond_result_wire_shape() {
    let req = RequestMessage {
        jsonrpc: "2.0".into(),
        id: Id::Number(3),
        method: "question/respond".into(),
        params: Some(
            serde_json::to_value(QuestionRespondParams {
                question_id: "q-1".into(),
                answers: vec!["red".into()],
                skipped: false,
            })
            .unwrap(),
        ),
    };
    let req_json = serde_json::to_value(req).unwrap();
    assert_eq!(req_json["method"], "question/respond");
    assert_eq!(req_json["params"]["question_id"], "q-1");
    assert_eq!(req_json["params"]["answers"][0], "red");

    let resp = ResponseMessage::Success {
        jsonrpc: "2.0".into(),
        id: Id::Number(3),
        result: serde_json::to_value(QuestionRespondResult { accepted: true }).unwrap(),
    };
    let json = serde_json::to_value(resp).unwrap();
    assert_eq!(json["result"]["accepted"], true);
}
