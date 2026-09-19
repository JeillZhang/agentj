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
use agentj_question::{AskTool, QuestionModule};
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
        /// Bind a resident multi-client TCP kernel on <host>:<port> (default host 127.0.0.1).
        #[arg(long)]
        port: Option<u16>,
        /// Bind address; use 0.0.0.0 to accept remote/LAN clients.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Require this access token in initialize when serving TCP.
        #[arg(long)]
        token: Option<String>,
        /// Max concurrent TCP connections for resident mode.
        #[arg(long, default_value_t = 16)]
        max_connections: usize,
    },
    /// Serve the JSON-RPC API over TCP for the Web Dashboard.
    Dashboard {
        /// TCP port to listen on for the Web Dashboard.
        #[arg(long)]
        port: u16,
        /// Bind address (default 127.0.0.1); use 0.0.0.0 for LAN/remote clients.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Require this access token in initialize when serving remote clients.
        #[arg(long)]
        token: Option<String>,
    },
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
    question_module: Arc<QuestionModule>,
    trajectory_store: Option<Arc<agentj_ipc::trajectory::TrajectoryStore>>,
) -> IpcServer {
    let mut ipc_server = IpcServer::new(kernel, session);
    ipc_server.permission_module = Some(permission_module);
    ipc_server.question_module = Some(question_module);
    ipc_server.trajectory_store = trajectory_store;
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

fn spawn_tcp_signal_task(shared: Arc<IpcServer>) -> tokio::task::JoinHandle<()> {
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
        let _ = shared.signal_shutdown();
        #[cfg(unix)]
        {
            let mut sigterm2 =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm2.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        std::process::exit(130);
    })
}

/// TCP listener options bundled to keep `serve_tcp` within the argument-count gate.
struct TcpServeOptions {
    host: String,
    port: u16,
    token: Option<String>,
    max_connections: usize,
    trajectory_store: Option<Arc<agentj_ipc::trajectory::TrajectoryStore>>,
}

/// Whether `host` refers to the loopback interface (the local-trust default).
fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    host.trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Bracket bare IPv6 literals so they compose into valid `host:port` pairs.
fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// Warn when a non-loopback bind has no token — the kernel would be exposed
/// to the network without access control (design: 127.0.0.1 local-trust model).
fn insecure_bind_warning(host: &str, token: Option<&str>) -> Option<String> {
    if is_loopback_host(host) || token.is_some_and(|t| !t.is_empty()) {
        return None;
    }
    Some(format!(
        "warning: binding {host} without --token exposes the kernel to the network; add --token for access control"
    ))
}

async fn serve_tcp(
    kernel: Kernel,
    session: Arc<dyn SessionStore>,
    permission_module: Arc<agentj_permission::PermissionModule>,
    question_module: Arc<QuestionModule>,
    options: TcpServeOptions,
    mcp_manager: Arc<McpConnectionManager>,
) -> anyhow::Result<()> {
    let warning = insecure_bind_warning(&options.host, options.token.as_deref());
    let mut tcp = agentj_ipc::tcp::TcpIpcServer::bind(
        &format!("{}:{}", bracket_ipv6(&options.host), options.port),
        kernel,
        session,
        Some(permission_module),
        Some(question_module),
        agentj_ipc::tcp::TcpBindOptions {
            token: options.token,
            max_connections: options.max_connections,
            trajectory_store: options.trajectory_store,
            web_dist: None,
        },
    )
    .await?;
    let local = tcp.local_addr()?;
    println!("listening on {local}");
    println!(
        "websocket on ws://{}:{}/ws",
        bracket_ipv6(&options.host),
        local.port()
    );
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    let signal_task = spawn_tcp_signal_task(tcp.shared_server());
    // Resident mode: exits when the last client disconnects or a shutdown
    // request arrives (决策 #3 按需常驻).
    tcp.run().await?;
    signal_task.abort();
    mcp_manager.close_all().await;
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
    let question_module = Arc::new(QuestionModule::new());
    let tools = Arc::new(ToolRegistry::new());
    register_builtins(&tools);
    tools.register(Arc::new(AskTool::new(question_module.clone())));
    let providers = Arc::new(ProviderRegistry::new());
    // Keep a concrete Arc before the ctx move: spawn_expiry_loop is NOT
    // object-safe (self: Arc<Self>) so it must be called on the concrete type.
    let session_dir = resolve_session_dir(&config);
    let session_cfg = config.inner().session.as_ref();
    let trajectory_store = Arc::new(agentj_ipc::trajectory::TrajectoryStore::new(
        session_dir.clone(),
        session_cfg.and_then(|s| s.trajectory_max_events),
        session_cfg.and_then(|s| s.trajectory_max_bytes),
    ));
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
    let mcp_servers = config.inner().mcp.clone();

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
        "你是 AgentJ，一位专业的 AI 编程助手。请始终使用中文回答，代码注释和解释均使用中文，保持专业、简洁。".into(),
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
    if let Some(mcp_cfg) = mcp_servers.as_ref() {
        for srv in &mcp_cfg.servers {
            let spec = agentj_mcp::McpServerSpec {
                command: srv.command.clone(),
                args: srv.args.clone(),
                env: srv.env.clone(),
            };
            let m = Arc::clone(&mcp_manager);
            if let Err(e) = m.connect(srv.name.clone(), spec).await {
                tracing::warn!("mcp server {} connect failed: {e}", srv.name);
            }
        }
    }
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
                mcp_manager.close_all().await;
            } else {
                let ipc_server = build_ipc_server(
                    kernel,
                    session_for_ipc.clone(),
                    permission_module,
                    question_module.clone(),
                    Some(Arc::clone(&trajectory_store)),
                );
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
                mcp_manager.close_all().await;
            }
        }
        Some(Commands::Serve { port: None, .. }) => {
            let mut ipc_server = build_ipc_server(
                kernel,
                session_for_ipc,
                permission_module,
                question_module.clone(),
                Some(Arc::clone(&trajectory_store)),
            );
            let signal_task = spawn_signal_task(ipc_server.kernel_sender());
            ipc_server.serve_stdio().await?;
            signal_task.abort();
            mcp_manager.close_all().await;
        }
        Some(Commands::Serve {
            port: Some(port),
            host,
            token,
            max_connections,
        }) => {
            serve_tcp(
                kernel,
                session_for_ipc,
                permission_module,
                question_module.clone(),
                TcpServeOptions {
                    host,
                    port,
                    token,
                    max_connections,
                    trajectory_store: Some(Arc::clone(&trajectory_store)),
                },
                mcp_manager.clone(),
            )
            .await?;
        }
        Some(Commands::Dashboard { port, host, token }) => {
            serve_tcp(
                kernel,
                session_for_ipc,
                permission_module,
                question_module.clone(),
                TcpServeOptions {
                    host,
                    port,
                    token,
                    max_connections: 16,
                    trajectory_store: Some(Arc::clone(&trajectory_store)),
                },
                mcp_manager.clone(),
            )
            .await?;
        }
        None => {
            println!("Use `agentj run \"your prompt\"` or `agentj serve`");
            let ipc_server = build_ipc_server(
                kernel,
                session_for_ipc,
                permission_module,
                question_module.clone(),
                Some(Arc::clone(&trajectory_store)),
            );
            ipc_server.signal_shutdown()?;
            mcp_manager.close_all().await;
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

    #[test]
    fn serve_host_defaults_to_loopback() {
        let cli = Cli::try_parse_from(["agentj", "serve", "--port", "4096"]).expect("parse");
        match cli.command {
            Some(Commands::Serve { host, .. }) => assert_eq!(host, "127.0.0.1"),
            _ => panic!("expected serve subcommand"),
        }
    }

    #[test]
    fn serve_host_accepts_remote_bind() {
        let cli = Cli::try_parse_from(["agentj", "serve", "--port", "4096", "--host", "0.0.0.0"])
            .expect("parse");
        match cli.command {
            Some(Commands::Serve { host, .. }) => assert_eq!(host, "0.0.0.0"),
            _ => panic!("expected serve subcommand"),
        }
    }

    #[test]
    fn dashboard_host_defaults_to_loopback() {
        let cli = Cli::try_parse_from(["agentj", "dashboard", "--port", "4096"]).expect("parse");
        match cli.command {
            Some(Commands::Dashboard { host, .. }) => assert_eq!(host, "127.0.0.1"),
            _ => panic!("expected dashboard subcommand"),
        }
    }

    #[test]
    fn dashboard_accepts_token_flag() {
        let cli =
            Cli::try_parse_from(["agentj", "dashboard", "--port", "4096", "--token", "s3cret"])
                .expect("parse");
        match cli.command {
            Some(Commands::Dashboard { token, .. }) => assert_eq!(token.as_deref(), Some("s3cret")),
            _ => panic!("expected dashboard subcommand"),
        }
    }

    #[test]
    fn loopback_host_detection_covers_localhost_and_loopback_ips() {
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(is_loopback_host(host), "{host} should be loopback");
        }
        for host in ["0.0.0.0", "192.168.3.31"] {
            assert!(!is_loopback_host(host), "{host} should not be loopback");
        }
    }

    #[test]
    fn insecure_bind_warning_flags_exposed_bind_without_token() {
        assert!(insecure_bind_warning("0.0.0.0", None).is_some());
        assert!(insecure_bind_warning("0.0.0.0", Some("t")).is_none());
        assert!(insecure_bind_warning("0.0.0.0", Some("")).is_some());
        assert!(insecure_bind_warning("127.0.0.1", None).is_none());
    }

    #[test]
    fn bracket_ipv6_wraps_bare_literals_only() {
        assert_eq!(bracket_ipv6("::1"), "[::1]");
        assert_eq!(bracket_ipv6("[::1]"), "[::1]");
        assert_eq!(bracket_ipv6("0.0.0.0"), "0.0.0.0");
        assert_eq!(bracket_ipv6("192.168.3.31"), "192.168.3.31");
    }
}
