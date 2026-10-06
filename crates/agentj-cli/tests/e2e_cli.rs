use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn e2e_serve_responds_to_jsonrpc_session_new() {
    // Temp config — avoids touching any real config.toml
    let dir = std::env::temp_dir().join(format!("agentj-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(
        &cfg,
        "[model]\nprovider = \"mock\"\n\n[[model.providers]]\nname = \"mock\"\nkind = \"mock\"\nbase_url = \"mock://local\"\napi_key_env = \"\"\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
        )
        .unwrap();
    } // drop closes stdin → serve_stdio exits on EOF → kernel graceful shutdown

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#""session_id""#),
        "expected session_id in output, got: {stdout}"
    );
}

// design #13: SIGTERM must trigger the same graceful shutdown chain as stdin EOF.
// Unix-only (SIGTERM/kill semantics); cfg(unix) keeps the test portable.
#[cfg(unix)]
#[test]
fn e2e_serve_exits_gracefully_on_sigterm() {
    let dir = std::env::temp_dir().join(format!("agentj-e2e-sig-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("mock_config.toml");
    std::fs::write(
        &cfg,
        "[model]\nprovider = \"mock\"\n\n[[model.providers]]\nname = \"mock\"\nkind = \"mock\"\nbase_url = \"mock://local\"\napi_key_env = \"\"\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"session/new"}}"#
    )
    .unwrap();

    // Wait for the session/new reply before signalling: by then the serve loop — and the
    // SIGTERM handler spawned just before it — is installed, so the signal cannot race
    // process startup (a fixed sleep flaked when startup exceeded it).
    let mut reader = BufReader::new(stdout);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert!(
        first.contains(r#""session_id""#),
        "expected session_id in output, got: {first}"
    );

    // Send SIGTERM (kill -TERM). unsafe: raw pid syscall — safe here, child is ours.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }

    // Drain the rest of stdout so a full pipe cannot deadlock the child, then reap it.
    // stdin stays OPEN — only the signal should end the process.
    let drain = std::thread::spawn(move || {
        let mut reader = reader;
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            line.clear();
        }
    });
    let status = child.wait().unwrap();
    let _ = drain.join();
    assert!(
        status.success(),
        "expected graceful exit 0, got: {status:?}"
    );
    drop(stdin);
}

// ---- T25 E2E: file/list over stdio + scripted mock-workbench run ----

/// Unique temp dir per test (std-only; this crate has no tempfile dev-dependency).
fn e2e_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentj-e2e-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `serve` config with a caller-provided `[model]` block + extra section; `[session] dir`
/// always points into the temp tree so no test touches the real `$HOME` session store.
fn write_serve_config(dir: &Path, model: &str, extra: &str, sessions: &Path) -> PathBuf {
    let cfg = dir.join("mock_config.toml");
    let body = format!(
        "{model}\n\n{extra}\n[session]\ndir = \"{dir}\"\n",
        dir = sessions.display()
    );
    std::fs::write(&cfg, body).unwrap();
    cfg
}

/// Spawn `agentj --config <cfg> serve`; CWD is the project root so relative tool
/// paths fall back to it when a session has no `project_root`.
fn spawn_serve(cfg: &Path, workdir: &Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args(["--config", cfg.to_str().unwrap(), "serve"])
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Read the next JSON line (skips tracing log lines the server writes to stdout).
fn read_json_line<R: BufRead>(stdout: &mut R, line: &mut String) -> bool {
    loop {
        line.clear();
        if stdout.read_line(line).unwrap() == 0 {
            return false; // EOF
        }
        if line.trim_start().starts_with('{') {
            return true;
        }
    }
}

/// Read JSON lines until the response with `id` arrives; returns every JSON line
/// seen (notifications included, in stream order).
fn read_until_response<R: BufRead>(stdout: &mut R, id: u64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for _ in 0..40 {
        assert!(
            read_json_line(stdout, &mut line),
            "EOF before response id {id}; saw {lines:?}"
        );
        let trimmed = line.trim().to_string();
        let is_response = serde_json::from_str::<serde_json::Value>(&trimmed)
            .ok()
            .is_some_and(|v| v["id"] == id);
        lines.push(trimmed);
        if is_response {
            return lines;
        }
    }
    panic!("response id {id} not seen within the line budget: {lines:?}");
}

/// `session/new {project_root}` request line.
fn session_new_request(id: u64, project_root: &Path) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/new",
        "params": { "project_root": project_root.to_str().unwrap() },
    })
    .to_string()
}

/// Session id from a `session/new` response line.
fn session_id_from(response: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(response).unwrap();
    value["result"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no session_id in {response}"))
        .to_string()
}

/// T25: `file/list` on a served session resolves entries against `session/new`'s
/// `project_root` — the config-ish file is listed and `.git` is never returned.
#[test]
fn e2e_file_list_root_after_session_new_with_project_root() {
    let dir = e2e_temp_dir("file-list");
    let project = dir.join("project");
    let sessions = dir.join("sessions");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(project.join("workbench_config.toml"), "[model]\n").unwrap();
    let cfg = write_serve_config(
        &dir,
        "[model]\nprovider = \"mock\"\n\n[[model.providers]]\nname = \"mock\"\n\
         kind = \"mock\"\nbase_url = \"mock://local\"\napi_key_env = \"\"",
        "",
        &sessions,
    );

    let mut child = spawn_serve(&cfg, &project);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    writeln!(stdin, "{}", session_new_request(1, &project)).unwrap();
    stdin.flush().unwrap();
    let new_lines = read_until_response(&mut stdout, 1);
    let sid = session_id_from(new_lines.last().unwrap());

    let list = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "file/list",
        "params": { "session_id": sid },
    });
    writeln!(stdin, "{list}").unwrap();
    stdin.flush().unwrap();
    let list_lines = read_until_response(&mut stdout, 2);
    let response: serde_json::Value = serde_json::from_str(list_lines.last().unwrap()).unwrap();
    let entries = response["result"]["entries"].as_array().unwrap();
    let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();

    assert!(
        names.contains(&"workbench_config.toml"),
        "root listing must contain the config-ish file: {names:?}"
    );
    assert!(
        !names.contains(&".git"),
        "root listing must never return .git: {names:?}"
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

/// T25: one scripted `mock-workbench` run emits a `$/progress` todo frame
/// (revision 1) and a `file_change` add frame for `e2e-artifact.txt`.
#[test]
fn e2e_run_emits_todo_and_file_change_frames() {
    let dir = e2e_temp_dir("run-frames");
    let project = dir.join("project");
    let sessions = dir.join("sessions");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&sessions).unwrap();
    let cfg = write_serve_config(
        &dir,
        "[model]\nprovider = \"mock-workbench\"\n\n[[model.providers]]\n\
         name = \"mock-workbench\"\nkind = \"mock-workbench\"\n\
         base_url = \"mock://local\"\napi_key_env = \"\"",
        "[permission.tools.write]\nmode = \"allow\"\n",
        &sessions,
    );

    let mut child = spawn_serve(&cfg, &project);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    writeln!(stdin, "{}", session_new_request(1, &project)).unwrap();
    stdin.flush().unwrap();
    let new_lines = read_until_response(&mut stdout, 1);
    let sid = session_id_from(new_lines.last().unwrap());

    let run = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/run",
        "params": { "session_id": sid, "content": "run the workbench scenario" },
    });
    writeln!(stdin, "{run}").unwrap();
    stdin.flush().unwrap();
    let frames = read_until_response(&mut stdout, 3);
    let output = frames.join("\n");

    assert!(
        frames
            .iter()
            .any(|l| l.contains(r#""kind":"todo""#) && l.contains(r#""revision":1"#)),
        "missing todo revision-1 frame: {output}"
    );
    assert!(
        frames.iter().any(|l| {
            l.contains(r#""kind":"file_change""#)
                && l.contains(r#""op":"add""#)
                && l.contains("e2e-artifact.txt")
        }),
        "missing file_change add frame: {output}"
    );
    assert!(
        project.join("e2e-artifact.txt").is_file(),
        "the write tool must create e2e-artifact.txt under the project root"
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
