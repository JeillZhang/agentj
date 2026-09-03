//! agentj-cli — the `agentj` binary: run a single prompt, or serve the JSON-RPC API
//! over stdio. Production wiring: AgentLoop registers into the Kernel as a module so
//! init/shutdown run in the real process; stdin EOF and signals share the same
//! graceful-shutdown chain (design #13).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};

use agentj_context::ContextPipeline;
use agentj_core::dispatcher::SubAgentService;
use agentj_core::kernel::Kernel;
use agentj_core::module::{
    KernelContext, KernelMessage, ModuleRegister, ProviderRegistry, SessionStore, ToolRegistry,
};
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::TokenUsage;
use agentj_ipc::IpcServer;
use agentj_mcp::McpConnectionManager;
use agentj_model::factory::ProviderFactory;
use agentj_session::FileBackedStore;
use agentj_skills::load_skills;
use agentj_tools::builtins::register_builtins;

mod license;

#[derive(Parser)]
#[command(name = "agentj", version, about = "AgentJ — AI coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[arg(long)]
    license_key: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single prompt and print the final response.
    Run {
        prompt: String,
        /// Attach to a resident kernel instead of an embedded one (tcp://host:port).
        #[arg(long)]
        attach: Option<String>,
    },
    /// Serve the JSON-RPC API over stdio, or over TCP with --port (resident kernel).
    Serve {
        /// Bind a resident multi-client TCP kernel on 127.0.0.1:<port>.
        #[arg(long)]
        port: Option<u16>,
        /// Require this access token in initialize when serving TCP.
        #[arg(long)]
        token: Option<String>,
        /// Max concurrent TCP connections for resident mode.
        #[arg(long, default_value_t = 16)]
        max_connections: usize,
    },
    /// Serve the JSON-RPC API over TCP for the Web Dashboard.
    Dashboard { port: u16 },
}

/// Build the per-tool policy map from the [permission] config section
/// (empty map when absent — the engine falls back to factory defaults).
fn permission_policies_from_config(
    cfg: &agentj_config::Config,
) -> std::collections::HashMap<String, agentj_permission::ToolPolicy> {
    cfg.inner().permission.clone().unwrap_or_default().tools
}

fn resolve_session_dir(cfg: &agentj_config::Config) -> std::path::PathBuf {
    if let Some(dir) = cfg.inner().session.as_ref().and_then(|s| s.dir.clone()) {
        return dir;
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return std::path::Path::new(&xdg).join("agentj").join("sessions");
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::Path::new(&home)
            .join(".local")
            .join("share")
            .join("agentj")
            .join("sessions");
    }
    std::path::PathBuf::from(".agentj").join("sessions")
}

fn record_completed_turn(
    collector: &agentj_stats::UsageCollector,
    sid: String,
    usage: Option<TokenUsage>,
) {
    match usage {
        Some(u) if u.prompt_tokens != 0 || u.completion_tokens != 0 => {
            collector.record_token_usage(sid, u.prompt_tokens, u.completion_tokens)
        }
        _ => collector.record_turn(sid),
    }
}

fn build_ipc_server(
    kernel: Kernel,
    session: Arc<dyn SessionStore>,
    permission_module: Arc<agentj_permission::PermissionModule>,
) -> IpcServer {
    let mut ipc_server = IpcServer::new(kernel, session);
    ipc_server.permission_module = Some(permission_module);
    ipc_server
}

fn spawn_signal_task(sender: agentj_core::kernel::KernelSender) -> tokio::task::JoinHandle<()> {
    // design #13: SIGINT/SIGTERM → same graceful shutdown chain as stdin EOF.
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        let _ = sender.send(KernelMessage::Shutdown);
    })
}

async fn serve_tcp(
    kernel: Kernel,
    session: Arc<dyn SessionStore>,
    permission_module: Arc<agentj_permission::PermissionModule>,
    port: u16,
    token: Option<String>,
    max_connections: usize,
) -> anyhow::Result<()> {
    let mut tcp = agentj_ipc::tcp::TcpIpcServer::bind(
        &format!("127.0.0.1:{port}"),
        kernel,
        session,
        Some(permission_module),
        token,
        max_connections,
    )
    .await?;
    println!("listening on {}", tcp.local_addr()?);
    let signal_task = spawn_signal_task(tcp.kernel_sender());
    // Resident mode: exits when the last client disconnects or a shutdown
    // request arrives (决策 #3 按需常驻).
    tcp.run().await?;
    signal_task.abort();
    Ok(())
}

async fn run_attached(url: &str, prompt: &str) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let target = url.strip_prefix("tcp://").unwrap_or(url);
    let stream = tokio::net::TcpStream::connect(target).await?;
    let (r, mut w) = stream.into_split();
    let mut reader = BufReader::new(r);

    w.write_all(br#"{"jsonrpc":"2.0","id":1,"method":"session/new"}"#)
        .await?;
    w.write_all(b"\n").await?;
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    if line.is_empty() {
        anyhow::bail!("connection closed before session/new completed");
    }
    let v: serde_json::Value = serde_json::from_str(line.trim())?;
    let sid = v["result"]["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session/new failed: {line}"))?
        .to_string();

    let run_line = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"session/run","params":{{"session_id":"{sid}","content":{}}}}}"#,
        serde_json::to_string(prompt)?,
    );
    w.write_all(run_line.as_bytes()).await?;
    w.write_all(b"\n").await?;
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            anyhow::bail!("connection closed before session/run completed");
        }
        let v: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(text) = v["result"]["content"].as_str() {
            println!("{text}");
            break;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let config =
        agentj_config::Config::load(Some(cli.config.into())).map_err(|e| anyhow::anyhow!("{e}"))?;
    let gate: Option<(&str, &str)> = match &cli.command {
        Some(Commands::Run { attach: None, .. }) => Some(("run", "run")),
        Some(Commands::Run {
            attach: Some(_), ..
        }) => None,
        Some(Commands::Serve { .. }) => Some(("serve", "serve")),
        Some(Commands::Dashboard { .. }) => Some(("dashboard", "dashboard")),
        None => None,
    };
    if let Some((entitlement, command_name)) = gate {
        license::check_license_gate(
            &config,
            cli.license_key.as_deref(),
            entitlement,
            command_name,
        )?;
    }
    let tools = Arc::new(ToolRegistry::new());
    register_builtins(&tools);
    let providers = Arc::new(ProviderRegistry::new());
    // Keep a concrete Arc before the ctx move: spawn_expiry_loop is NOT
    // object-safe (self: Arc<Self>) so it must be called on the concrete type.
    let session_dir = resolve_session_dir(&config);
    let session_store = Arc::new(FileBackedStore::new(&session_dir)?);
    let session: Arc<dyn SessionStore> = session_store.clone() as Arc<dyn SessionStore>;

    // design 4.3: instantiate each configured provider through the factory — kind
    // decides the wire format (openai-compat / responses / anthropic / opencode-go).
    // api_key_env is resolved from the environment per provider entry.
    for pcfg in &config.inner().model.providers {
        let api_key = pcfg
            .api_key_env
            .as_ref()
            .and_then(|env| std::env::var(env).ok())
            .unwrap_or_default();
        let provider = ProviderFactory::create(&pcfg.kind, &pcfg.name, &pcfg.base_url, api_key)?;
        providers.register(pcfg.name.clone(), provider);
    }

    // Extract before `config` is boxed into the KernelContext — its config field
    // is Arc<dyn ConfigProvider>, which erases the concrete Config type.
    let permission_cfg = config.inner().permission.clone().unwrap_or_default();
    permission_cfg
        .validate()
        .expect("invalid [permission] config");
    let permission_policies = permission_policies_from_config(&config);

    let ctx = KernelContext {
        session: session.clone(),
        tools,
        providers,
        config: Arc::new(config),
    };

    let mut kernel = Kernel::new();
    let mut register = ModuleRegister::with_services(
        Arc::clone(&ctx.tools),
        Arc::clone(&ctx.providers),
        kernel.services(),
    );
    let usage = Arc::new(agentj_stats::UsageCollector::new());
    kernel.register_module(
        Box::new(ToolExecutorModule::new(ctx.clone())),
        &ctx,
        &mut register,
    )?;
    let mut runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are AgentJ, an AI coding assistant.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );
    {
        let collector_clone = Arc::clone(&usage);
        runner.set_on_turn_complete(Arc::new(move |sid, usage| {
            record_completed_turn(&collector_clone, sid.clone(), usage)
        }));
    }
    kernel.register_module(
        Box::new(agentj_core::agent_loop_module::AgentLoopModule::new(
            runner.clone(),
        )),
        &ctx,
        &mut register,
    )?;
    kernel.services().register_kernel_async_service(
        "subagent".into(),
        Arc::new(SubAgentService::new(runner.clone())),
    );
    let session_for_ipc: Arc<dyn SessionStore> = session.clone();

    // Permission wiring (design #17): construct from [permission] config with
    // fail-fast rule validation, register with the kernel so graceful shutdown
    // clears grants + pending approvals, install the checker on the ToolRegistry,
    // and hand the module to the IPC server for permission/respond.
    let permission_module = Arc::new(agentj_permission::PermissionModule::new(
        permission_policies,
        std::time::Duration::from_secs(60),
    ));
    ctx.tools.set_permission_checker(permission_module.clone());
    kernel.register_module(
        Box::new(permission_module.as_ref().clone()),
        &ctx,
        &mut register,
    )?;

    // v0.3 ecosystem wiring (T11): take shared handles before the kernel is moved
    // into IpcServer, then spawn the MCP idle reaper + session expiry loop as
    // background tasks and load skills for observability (registration deferred
    // to the manifest-compat research).
    let events = kernel.events();
    let mcp_manager = Arc::new(McpConnectionManager::new(
        Arc::clone(&ctx.tools),
        Duration::from_secs(60),
        Duration::from_millis(500),
        5,
        events.clone(),
    ));
    tokio::spawn({
        let m = mcp_manager.clone();
        async move {
            loop {
                m.reap_idle().await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    });
    session_store.spawn_expiry_loop(
        events.clone(),
        Duration::from_secs(3600),
        Duration::from_secs(60),
    );

    let skills = load_skills(Path::new(".")).unwrap_or_default();
    for skill in &skills {
        tracing::info!(skill = %skill.name, priority = ?skill.priority, "loaded");
    }

    match cli.command {
        Some(Commands::Run { prompt, attach }) => {
            if let Some(url) = attach {
                run_attached(&url, &prompt).await?;
            } else {
                let ipc_server =
                    build_ipc_server(kernel, session_for_ipc.clone(), permission_module);
                let signal_task = spawn_signal_task(ipc_server.kernel_sender());
                let sid = ipc_server.create_session()?;
                let mut rx = ipc_server.run_turn_stream(&sid, &prompt);
                let mut out = String::new();
                while let Some(ev) = rx.recv().await {
                    match ev {
                        agentj_core::types::AgentEvent::Delta(d) => out.push_str(&d),
                        agentj_core::types::AgentEvent::Done { final_response, .. } => {
                            out = final_response;
                            break;
                        }
                        agentj_core::types::AgentEvent::Error(e) => {
                            eprintln!("error: {e}");
                            break;
                        }
                        _ => {}
                    }
                }
                println!("{out}");
                ipc_server.signal_shutdown()?;
                drop(signal_task);
            }
        }
        Some(Commands::Serve { port: None, .. }) => {
            let mut ipc_server = build_ipc_server(kernel, session_for_ipc, permission_module);
            let signal_task = spawn_signal_task(ipc_server.kernel_sender());
            ipc_server.serve_stdio().await?;
            signal_task.abort();
        }
        Some(Commands::Serve {
            port: Some(port),
            token,
            max_connections,
        }) => {
            serve_tcp(
                kernel,
                session_for_ipc,
                permission_module,
                port,
                token,
                max_connections,
            )
            .await?;
        }
        Some(Commands::Dashboard { port }) => {
            serve_tcp(kernel, session_for_ipc, permission_module, port, None, 16).await?;
        }
        None => {
            println!("Use `agentj run \"your prompt\"` or `agentj serve`");
            let ipc_server = build_ipc_server(kernel, session_for_ipc, permission_module);
            ipc_server.signal_shutdown()?;
        }
    }
    // The serve loop reads stdin through tokio's blocking pool; dropping the
    // runtime would wait for that thread while a client keeps stdin open.
    // Graceful shutdown already ran above — exit directly.
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Local helper: Config::load reads a file, so materialize the TOML at a
    // per-process temp path. Thread id disambiguates parallel test threads,
    // which share the process id (the brief's pid-only path races).
    fn load_test_config(toml: &str) -> agentj_config::Config {
        let dir = std::env::temp_dir().join(format!(
            "agentj-cli-cfg-{:?}-{}",
            std::thread::current().id(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.toml");
        std::fs::write(&path, toml).unwrap();
        agentj_config::Config::load(Some(path)).unwrap()
    }

    #[test]
    fn permission_module_built_from_config() {
        let cfg = load_test_config(
            r#"
            [permission.tools.bash]
            mode = "ask"
            [[permission.tools.bash.command_rules]]
            effect = "allow"
            prefix = "npm test"
        "#,
        );
        let policies = permission_policies_from_config(&cfg);
        assert_eq!(policies.len(), 1);
        assert!(policies.contains_key("bash"));
        assert_eq!(policies["bash"].command_rules[0].prefix, "npm test");
    }

    #[test]
    fn permission_module_defaults_without_section() {
        let cfg = load_test_config("");
        let policies = permission_policies_from_config(&cfg);
        assert!(policies.is_empty()); // engine falls back to factory defaults
    }

    #[test]
    fn invalid_permission_config_fails_fast() {
        let cfg = load_test_config(
            r#"
            [permission.tools.write]
            mode = "ask"
            [[permission.tools.write.path_rules]]
            effect = "allow"
            pattern = "[**"
        "#,
        );
        assert!(
            cfg.inner()
                .permission
                .clone()
                .unwrap_or_default()
                .validate()
                .is_err()
        );
    }

    use std::sync::OnceLock;
    static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn resolve_session_dir_prefers_config() {
        let _g = env_lock();
        let cfg = load_test_config("[session]\ndir = \"/cfg/sess\"\n");
        assert_eq!(resolve_session_dir(&cfg).to_string_lossy(), "/cfg/sess");
    }

    #[test]
    fn resolve_session_dir_falls_back_to_xdg() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/xdg");
        }
        unsafe {
            std::env::set_var("HOME", "/home/u");
        }
        let cfg = load_test_config("");
        let p = resolve_session_dir(&cfg);
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
        unsafe {
            std::env::remove_var("HOME");
        }
        assert_eq!(p.to_string_lossy(), "/xdg/agentj/sessions");
    }

    #[test]
    fn resolve_session_dir_falls_back_to_home() {
        let _g = env_lock();
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
        unsafe {
            std::env::set_var("HOME", "/home/u");
        }
        let cfg = load_test_config("");
        let p = resolve_session_dir(&cfg);
        unsafe {
            std::env::remove_var("HOME");
        }
        assert_eq!(p.to_string_lossy(), "/home/u/.local/share/agentj/sessions");
    }

    #[test]
    fn record_completed_turn_with_usage_records_tokens() {
        let c = agentj_stats::UsageCollector::new();
        record_completed_turn(
            &c,
            "s1".into(),
            Some(TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20,
            }),
        );
        let t = c.get_session_totals("s1").unwrap();
        assert_eq!(
            (t.prompt_tokens, t.completion_tokens, t.turns),
            (100, 20, 1)
        );
    }

    #[test]
    fn record_completed_turn_with_zero_usage_falls_back_to_turn() {
        let c = agentj_stats::UsageCollector::new();
        record_completed_turn(&c, "s1".into(), Some(TokenUsage::default()));
        let t = c.get_session_totals("s1").unwrap();
        assert_eq!((t.prompt_tokens, t.completion_tokens, t.turns), (0, 0, 1));
    }

    #[test]
    fn record_completed_turn_with_none_falls_back_to_turn() {
        let c = agentj_stats::UsageCollector::new();
        record_completed_turn(&c, "s1".into(), None);
        let t = c.get_session_totals("s1").unwrap();
        assert_eq!((t.prompt_tokens, t.completion_tokens, t.turns), (0, 0, 1));
    }
}
