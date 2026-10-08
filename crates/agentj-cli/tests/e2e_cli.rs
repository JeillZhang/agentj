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

/// Read stdout lines until the `session/new` reply shows up (startup warnings may precede
/// it), or until EOF. Returns everything read, for an informative assertion message.
#[cfg(unix)]
fn read_until_session_reply(reader: &mut impl BufRead) -> String {
    let mut seen = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        seen.push_str(&line);
        if seen.contains(r#""session_id""#) {
            break;
        }
    }
    seen
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
    // process startup (a fixed sleep flaked when startup exceeded it). Startup warnings
    // (e.g. the built-in opencode-go entry without a key) may precede the reply on stdout,
    // so scan lines until the reply arrives instead of assuming the first line is it.
    let mut reader = BufReader::new(stdout);
    let seen = read_until_session_reply(&mut reader);
    assert!(
        seen.contains(r#""session_id""#),
        "expected session_id in output, got: {seen}"
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

/// Unique temp dir (the crate has no tempfile dev-dependency); also creates sessions/.
fn run_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentj-run-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    dir
}

/// Key-free `mock` provider + auto-allowed `bash`: the scripted mock's first call is a
/// `bash` tool call, so the policy must not ask (same recipe as tests/e2e_serve.rs).
fn write_run_config(dir: &Path) -> PathBuf {
    let cfg = dir.join("run_config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[model]\nprovider = \"mock\"\n\n\
             [[model.providers]]\nname = \"mock\"\nkind = \"mock\"\n\
             base_url = \"mock://local\"\napi_key_env = \"\"\n\n\
             [permission.tools.bash]\nmode = \"allow\"\n\n\
             [session]\ndir = \"{}\"\n",
            dir.join("sessions").display()
        ),
    )
    .unwrap();
    cfg
}

/// `agentj --config <cfg> run <args…>` from the project root, output captured.
fn run_cli(cfg: &Path, workdir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agentj"))
        .arg("--config")
        .arg(cfg)
        .arg("run")
        .args(args)
        .current_dir(workdir)
        .output()
        .expect("agentj run must spawn")
}

/// The response is printed last; tracing logs share stdout, so compare the last line only.
fn last_line(s: &str) -> String {
    s.trim_end()
        .lines()
        .last()
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[test]
fn e2e_run_unknown_provider_fails_with_registered_names() {
    let dir = run_temp_dir("unknown-provider");
    let cfg = write_run_config(&dir);
    let out = run_cli(&cfg, &dir, &["--provider", "nope", "hi"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        last_line(&stdout),
        "done",
        "a failed run must not print a final response, got {stdout:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown provider: nope"), "{stderr}");
    assert!(stderr.contains("mock"), "{stderr}");
}

#[test]
fn e2e_run_blank_provider_falls_back_to_default() {
    let dir = run_temp_dir("blank-provider");
    let cfg = write_run_config(&dir);
    let out = run_cli(&cfg, &dir, &["--provider", "   ", "hi"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(last_line(&String::from_utf8_lossy(&out.stdout)), "done");
}

#[test]
fn e2e_run_model_override_exits_zero() {
    let dir = run_temp_dir("model-override");
    let cfg = write_run_config(&dir);
    let out = run_cli(&cfg, &dir, &["--model", "custom-model", "hi"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // the last line is the response: an echo line would follow it and fail this assertion
    assert_eq!(last_line(&String::from_utf8_lossy(&out.stdout)), "done");
}

#[test]
// Regression pin (also green before this task): the no-flag path keeps printing the response.
fn e2e_run_without_flags_prints_final_response() {
    let dir = run_temp_dir("no-flags");
    let cfg = write_run_config(&dir);
    let out = run_cli(&cfg, &dir, &["hi"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(last_line(&String::from_utf8_lossy(&out.stdout)), "done");
}

// ---- T2: `--attach` carries the run overrides and surfaces JSON-RPC errors ----

/// Reaps the spawned kernel on every exit path, panics included (pattern from tests/e2e_serve.rs).
struct AutoReap(std::process::Child);
impl Drop for AutoReap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Read server stdout lines until the resident kernel prints `listening on <addr>`
/// (copy of tests/e2e_serve.rs::read_listening_addr, which must stay in sync).
fn read_listening_addr<R: BufRead>(reader: &mut R) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut line = String::new();
    while std::time::Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.trim().strip_prefix("listening on ") {
                    return rest.trim().to_string();
                }
            }
            Err(_) => break,
        }
    }
    panic!("resident kernel did not print a listening address");
}

#[test]
fn e2e_run_attach_surfaces_unknown_provider() {
    let dir = run_temp_dir("attach-unknown-provider");
    let cfg = write_run_config(&dir);
    let mut kernel = AutoReap(
        Command::new(env!("CARGO_BIN_EXE_agentj"))
            .args(["--config", cfg.to_str().unwrap(), "serve", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = kernel.0.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    // `reader` stays alive for the rest of the test so kernel log lines keep draining
    let addr = read_listening_addr(&mut reader);

    let errfile = dir.join("attach-stderr.txt");
    let mut attach = Command::new(env!("CARGO_BIN_EXE_agentj"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "run",
            "--attach",
            &format!("tcp://{addr}"),
            "--provider",
            "nope",
            "hi",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&errfile).unwrap()))
        .spawn()
        .unwrap();

    // A swallowed error reply would wait forever; bound the child so the test fails, not hangs.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(s) = attach.try_wait().unwrap() {
            break s;
        }
        if std::time::Instant::now() >= deadline {
            let _ = attach.kill();
            let _ = attach.wait();
            panic!("agentj run --attach hung instead of surfacing the JSON-RPC error");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!(
        status.code(),
        Some(1),
        "stderr: {}",
        std::fs::read_to_string(&errfile).unwrap_or_default()
    );
    let stderr = std::fs::read_to_string(&errfile).unwrap();
    assert!(stderr.contains("unknown provider: nope"), "{stderr}");
}
