//! E2E: approval ownership over the resident TCP server.
//!
//! - `run --attach` is a non-interactive client: it must *report* the approval it denies
//!   instead of silently waiting out the remote timeout.
//! - a second client reclaiming a session must be able to approve, so a dropped owner does
//!   not wedge the run.
//! - a client that does not own the session must not be able to approve.

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(10);
const WARNING_PREFIX: &str = "warning: run --attach cannot answer approvals";

/// A killed-on-drop resident server plus the address it printed.
struct Served {
    child: Child,
    addr: String,
    /// 保留 serve 的 stdout 管道（关掉会让服务端后续打印 panic）。
    _stdout: BufReader<std::process::ChildStdout>,
    _env: common::TempEnv,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start `serve --port 0` on `config_body` and return the listening address.
fn start_served(config_body: &str) -> Served {
    let env = common::TempEnv::new();
    let cfg = env.write_config_file(config_body);
    let mut cmd = env.command();
    cmd.args(["--config", cfg.to_str().unwrap(), "serve", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("spawn serve");
    let mut stdout = BufReader::new(child.stdout.take().expect("serve stdout"));
    let addr = common::read_listening_addr(&mut stdout);
    // 不要 mem::forget(stdout)：把读者留在 Served 里，靠结构体生命周期保活。
    Served {
        child,
        addr,
        _stdout: stdout,
        _env: env,
    }
}

/// Drain lines into a channel so the body can wait with a deadline.
fn spawn_line_reader<R: BufRead + Send + 'static>(mut reader: R) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if tx.send(line.trim_end().to_string()).is_err() {
                        return;
                    }
                }
            }
        }
    });
    rx
}

#[test]
fn attach_reports_and_denies_a_remote_approval_request() {
    let served = start_served(include_str!("fixtures/attach_ask.toml"));
    let started = Instant::now();
    let cfg = served
        ._env
        .write_config_file(include_str!("fixtures/attach_ask.toml"));
    let mut attach = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "run",
            "--attach",
            &format!("tcp://{}", served.addr),
            "please run the tool",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn attach");
    let rx = spawn_line_reader(BufReader::new(
        attach.stderr.take().expect("attach stderr"),
    ));
    let warning = loop {
        match rx.recv_timeout(DEADLINE) {
            Ok(line) if line.starts_with(WARNING_PREFIX) => break line,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                let _ = attach.kill();
                panic!("attach must report the approval it denies, not time out silently");
            }
        }
    };
    assert!(
        warning.contains("bash"),
        "the warning names the tool: {warning}"
    );
    assert!(started.elapsed() < DEADLINE);
    let _ = attach.kill();
    let _ = attach.wait();
}

/// The mock provider scripts one bash call per run; `mode = "ask"` turns it into an approval.
fn ask_config(timeout_ms: u32) -> String {
    format!(
        "[model]\nprovider = \"mock\"\n\n[[model.providers]]\nname = \"mock\"\nkind = \"mock\"\n\
         base_url = \"mock://local\"\napi_key_env = \"\"\n\n[permission]\ntimeout_ms = {timeout_ms}\n\n\
         [permission.tools.bash]\nmode = \"ask\"\n"
    )
}

/// Open a run on the served kernel and return `(session_id, run_id, request_id)` once the
/// approval request has arrived.
fn run_until_approval(
    client: &mut common::SocketClient,
    session_id: &str,
) -> (u64, String) {
    let run_id = client.send(
        "session/run",
        serde_json::json!({"session_id": session_id, "content": "run the tool"}),
    );
    let ask = client.next_frame_until(|v| v["method"] == "$/permissionRequest");
    let rid = ask["params"]["request_id"]
        .as_str()
        .expect("request id")
        .to_string();
    (run_id, rid)
}

#[test]
fn second_client_reclaims_ownership_after_the_owner_disconnects() {
    let served = start_served(&ask_config(30_000));
    let mut a = common::SocketClient::connect(&served.addr);
    let sid = a.call("session/new", serde_json::json!({}))["session_id"]
        .as_str()
        .expect("session id")
        .to_string();
    drop(a); // owner 断连：settle_owner_pending 清 owner，但会话本身仍在

    let mut b = common::SocketClient::connect(&served.addr);
    // A 的 EOF 可能晚于这条请求被处理：owner 若仍指向 A，subscribe 就跳过认领（但本身成功）。
    // 真正的归属转移由随后 session/run 无条件的 claim_run_owner 完成。
    let sub = b.request("session/subscribe", serde_json::json!({"session_id": sid}));
    assert!(sub.get("error").is_none(), "subscribe must be accepted: {sub}");
    let (run_id, rid) = run_until_approval(&mut b, &sid);

    let answered = b.call(
        "permission/respond",
        serde_json::json!({"request_id": rid, "decision": "allow"}),
    );
    assert!(
        answered.get("error").is_none(),
        "the new owner's allow must be accepted: {answered}"
    );

    let done = b.next_frame_until(|v| v["id"] == run_id);
    assert!(
        done.get("error").is_none(),
        "the run must finish once the new owner approves: {done}"
    );
    assert!(done["result"]["content"]
        .as_str()
        .is_some_and(|c| !c.is_empty()));
}

#[test]
fn non_owner_respond_is_rejected() {
    let served = start_served(&ask_config(30_000));
    let mut owner = common::SocketClient::connect(&served.addr);
    let sid = owner.call("session/new", serde_json::json!({}))["session_id"]
        .as_str()
        .expect("session id")
        .to_string();
    let (run_id, rid) = run_until_approval(&mut owner, &sid);

    let mut intruder = common::SocketClient::connect(&served.addr);
    let reply = intruder.request(
        "permission/respond",
        serde_json::json!({"request_id": rid, "decision": "allow"}),
    );
    assert_eq!(
        reply["error"]["code"].as_i64(),
        Some(agentj_protocol::base::error_codes::NOT_AUTHORIZED as i64),
        "non-owner answers must be refused: {reply}"
    );

    owner.call(
        "permission/respond",
        serde_json::json!({"request_id": rid, "decision": "allow"}),
    );
    let done = owner.next_frame_until(|v| v["id"] == run_id);
    assert!(
        done.get("error").is_none(),
        "the owner's allow must still complete the run: {done}"
    );
}
