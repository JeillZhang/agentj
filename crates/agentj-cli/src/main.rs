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
    ConfigProvider, KernelContext, KernelMessage, ModuleRegister, ProviderRegistry, SessionStore,
    ToolRegistry,
};
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::TokenUsage;
use agentj_ipc::IpcServer;
use agentj_mcp::McpConnectionManager;
use agentj_question::{AskTool, QuestionModule};
use agentj_session::FileBackedStore;
use agentj_skills::load_skills;
use agentj_tools::builtins::register_builtins;

// Runtime (de)registration of the opencode-go provider, shared by startup and the
// credential handlers (design D1/G3): `activate_opencode_go` backs `auth/setApiKey` and
// `deactivate_opencode_go` backs `auth/clearCredential`.
mod activation;
mod auth;
mod license;
mod logging;
mod provider_service;
mod providers;
mod refreshing_provider;

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
        /// Provider name to use for this run only (default: the kernel's default provider).
        #[arg(long)]
        provider: Option<String>,
        /// Model id to use for this run only, e.g. "opencode-go/kimi-k3" (default: model.default_model).
        #[arg(long)]
        model: Option<String>,
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
        /// Directory containing the built Web UI (index.html); overrides auto-discovery.
        #[arg(long)]
        web_dist: Option<std::path::PathBuf>,
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
        /// Directory containing the built Web UI (index.html); overrides auto-discovery.
        #[arg(long)]
        web_dist: Option<std::path::PathBuf>,
    },
    /// Manage subscription provider credentials (login/status/logout).
    Auth {
        #[command(subcommand)]
        action: auth::AuthAction,
    },
}

/// Build the per-tool policy map from the [permission] config section
/// (empty map when absent — the engine falls back to factory defaults).
fn permission_policies_from_config(
    cfg: &agentj_config::Config,
) -> std::collections::HashMap<String, agentj_permission::ToolPolicy> {
    cfg.inner().permission.clone().unwrap_or_default().tools
}

/// Approval wait budget from `[permission] timeout_ms`, defaulting to the
/// design's 60s. Tests lower it so the auto-deny path stays fast.
fn permission_timeout(cfg: &agentj_permission::PermissionConfig) -> std::time::Duration {
    cfg.timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or_else(|| std::time::Duration::from_secs(60))
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
    provider_service: Option<Arc<dyn agentj_ipc::ProviderService>>,
) -> IpcServer {
    let mut ipc_server = IpcServer::new(kernel, session);
    ipc_server.permission_module = Some(permission_module);
    ipc_server.question_module = Some(question_module);
    ipc_server.trajectory_store = trajectory_store;
    ipc_server.provider_service = provider_service;
    ipc_server
}

/// Project the effective provider list into the service's own config type. Shares
/// `providers::service_cfgs` with the startup assembly (design D5: single derivation).
fn provider_cfgs(config: &agentj_config::Config) -> Vec<provider_service::ProviderCfg> {
    providers::service_cfgs(config.inner())
        .into_iter()
        .map(|p| provider_service::ProviderCfg {
            name: p.name,
            kind: p.kind,
            base_url: p.base_url,
            api_key_env: p.api_key_env,
        })
        .collect()
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
    web_dist: Option<std::path::PathBuf>,
    trajectory_store: Option<Arc<agentj_ipc::trajectory::TrajectoryStore>>,
    /// Optional subscription handle (design D4); `run` passes none.
    provider_service: Option<Arc<dyn agentj_ipc::ProviderService>>,
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

/// Four-tier web dist discovery: explicit flag/env (stop on invalid) then cwd/exe-relative.
fn resolve_web_dist(
    explicit: Option<std::path::PathBuf>,
    env: Option<std::path::PathBuf>,
    cwd: &Path,
    exe_dir: &Path,
) -> Option<std::path::PathBuf> {
    for (candidate, source) in [(explicit, "--web-dist"), (env, "AGENTJ_WEB_DIST")] {
        let Some(dir) = candidate else { continue };
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
        eprintln!(
            "warning: {source} path {} has no index.html; static web UI disabled",
            dir.display()
        );
        return None; // explicit source: stop discovery, never silently serve another dir
    }
    [
        cwd.join("agentj-ui/apps/web/dist"),
        exe_dir.join("../share/agentj/web"),
        exe_dir.join("web"),
    ]
    .into_iter()
    .find(|d| d.join("index.html").is_file())
}

/// Gather the process-level discovery inputs and resolve the web dist directory.
fn discover_web_dist(explicit: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let env = std::env::var_os("AGENTJ_WEB_DIST").map(std::path::PathBuf::from);
    resolve_web_dist(explicit, env, &cwd, &exe_dir)
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
    let web_dist = discover_web_dist(options.web_dist.clone());
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
            web_dist: web_dist.clone(),
            provider_service: options.provider_service,
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
    match &web_dist {
        Some(dir) => println!(
            "web ui on http://{}:{}/ (dist: {})",
            bracket_ipv6(&options.host),
            local.port(),
            dir.display()
        ),
        None => eprintln!(
            "warning: web dist not found; build it via `npm --prefix agentj-ui run build \
             --workspace=@agentj/web` or pass --web-dist <PATH>"
        ),
    }
    for warning in tcp.web_config().startup_warnings() {
        eprintln!("{warning}");
    }
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

/// NDJSON `session/run` request for the attached path: `provider`/`model` are
/// only serialized when set, so an absent flag leaves the remote default alone.
fn run_params_json(sid: &str, prompt: &str, provider: Option<&str>, model: Option<&str>) -> String {
    let mut params = serde_json::Map::new();
    params.insert("session_id".into(), serde_json::Value::from(sid));
    params.insert("content".into(), serde_json::Value::from(prompt));
    if let Some(p) = provider {
        params.insert("provider".into(), serde_json::Value::from(p));
    }
    if let Some(m) = model {
        params.insert("model".into(), serde_json::Value::from(m));
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/run",
        "params": serde_json::Value::Object(params),
    })
    .to_string()
}

/// Human-readable line for a JSON-RPC error reply, or `None` for a non-error
/// reply. `-32603` (internal error) is the kernel's generic failure carrier,
/// so its code adds no information; any other code is appended.
fn run_error_line(v: &serde_json::Value) -> Option<String> {
    let err = v.get("error")?;
    let msg = err["message"].as_str().unwrap_or_default();
    let code = err["code"].as_i64().unwrap_or_default();
    Some(if code == -32603 {
        format!("error: {msg}")
    } else {
        format!("error: {msg} (code {code})")
    })
}

async fn run_attached(
    url: &str,
    prompt: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<Option<String>> {
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

    let run_line = run_params_json(&sid, prompt, provider, model);
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
        // Checked before the result branch: an error reply has no
        // `result.content`, so ignoring it would spin here forever.
        if let Some(failure) = run_error_line(&v) {
            return Ok(Some(failure));
        }
        if let Some(text) = v["result"]["content"].as_str() {
            println!("{text}");
            break;
        }
    }
    Ok(None)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let cli = Cli::parse();

    if let Some(Commands::Auth { action }) = &cli.command {
        return auth::run_auth(action, &cli.config).await;
    }

    let config =
        agentj_config::Config::load(Some(cli.config.into())).map_err(|e| anyhow::anyhow!("{e}"))?;
    let gate: Option<(&str, &str)> = match &cli.command {
        Some(Commands::Run { attach: None, .. }) => Some(("run", "run")),
        Some(Commands::Run {
            attach: Some(_), ..
        }) => None,
        Some(Commands::Serve { .. }) => Some(("serve", "serve")),
        Some(Commands::Dashboard { .. }) => Some(("dashboard", "dashboard")),
        Some(Commands::Auth { .. }) => None,
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

    // design 4.3 + D7: assemble providers with credential resolution; keyless real
    // providers are skipped (never an empty Bearer) and an empty registry is fatal.
    // `run --attach` talks to a remote kernel, so local providers are not required.
    let attached = matches!(
        &cli.command,
        Some(Commands::Run {
            attach: Some(_),
            ..
        })
    );
    if !attached {
        let auth_store =
            agentj_auth::AuthStore::with_default_path().map_err(|e| anyhow::anyhow!("{e}"))?;
        let live_base = agentj_model::catalog::ModelCatalog::builtin().base_url;
        let console_base = config
            .get_str("model.console_base_url")
            .unwrap_or_else(|| "https://opencode.ai/console".into());
        let assembly = providers::build_providers(
            config.inner(),
            None,
            &auth_store,
            || {
                let base = live_base.clone();
                async move {
                    agentj_model::catalog_source::fetch_live_models(&base, None)
                        .await
                        .map(Some)
                        .map_err(|e| e.to_string())
                }
            },
            &console_base,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        for warning in &assembly.warnings {
            tracing::warn!("{warning}");
        }
        for (name, provider) in assembly.providers {
            providers.register(name, provider);
        }
    }

    // design D4: only the IPC-serving commands expose the subscription methods; `run` has no
    // IPC server and `run --attach` targets a remote kernel whose own handle serves them.
    let inject_subscription = matches!(
        &cli.command,
        Some(Commands::Serve { .. }) | Some(Commands::Dashboard { .. })
    );
    let provider_service: Option<Arc<dyn agentj_ipc::ProviderService>> = if inject_subscription {
        let store =
            agentj_auth::AuthStore::with_default_path().map_err(|e| anyhow::anyhow!("{e}"))?;
        let console_base = config
            .get_str("model.console_base_url")
            .unwrap_or_else(|| "https://opencode.ai/console".into());
        let default_model = config.get_str("model.default_model").unwrap_or_default();
        let svc = provider_service::CliProviderService::new(
            store,
            provider_cfgs(&config),
            config.get_str("model.provider").unwrap_or_default(),
            console_base,
            config.get_int("model.quota_warn_percent").unwrap_or(80) as u8,
            providers.clone(),
        )
        .with_default_model(default_model);
        Some(Arc::new(svc))
    } else {
        None
    };

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
        permission_timeout(&permission_cfg),
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
        Some(Commands::Auth { .. }) => {}
        Some(Commands::Run {
            prompt,
            attach,
            provider,
            model,
        }) => {
            if let Some(url) = attach {
                let failure =
                    run_attached(&url, &prompt, provider.as_deref(), model.as_deref()).await?;
                mcp_manager.close_all().await;
                if let Some(line) = failure {
                    eprintln!("{line}");
                    std::process::exit(1);
                }
            } else {
                let ipc_server = build_ipc_server(
                    kernel,
                    session_for_ipc.clone(),
                    permission_module,
                    question_module.clone(),
                    Some(Arc::clone(&trajectory_store)),
                    provider_service.clone(),
                );
                let signal_task = spawn_signal_task(ipc_server.kernel_sender());
                let sid = ipc_server.create_session()?;
                let overrides = agentj_core::types::RunOverrides {
                    provider: provider.clone(),
                    model: model.clone(),
                };
                let mut rx = ipc_server.run_turn_stream_with(&sid, &prompt, overrides);
                let mut out = String::new();
                let mut failed: Option<String> = None;
                while let Some(ev) = rx.recv().await {
                    match ev {
                        agentj_core::types::AgentEvent::Delta(d) => out.push_str(&d),
                        agentj_core::types::AgentEvent::Done { final_response, .. } => {
                            out = final_response;
                            break;
                        }
                        agentj_core::types::AgentEvent::Error(e) => {
                            failed = Some(e);
                            break;
                        }
                        _ => {}
                    }
                }
                // Shut down before reporting: the failed path exits the process, which
                // would otherwise strand the embedded kernel and its signal task.
                ipc_server.signal_shutdown()?;
                drop(signal_task);
                mcp_manager.close_all().await;
                if let Some(e) = failed {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
                println!("{out}");
            }
        }
        Some(Commands::Serve { port: None, .. }) => {
            let mut ipc_server = build_ipc_server(
                kernel,
                session_for_ipc,
                permission_module,
                question_module.clone(),
                Some(Arc::clone(&trajectory_store)),
                provider_service.clone(),
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
            web_dist,
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
                    web_dist,
                    trajectory_store: Some(Arc::clone(&trajectory_store)),
                    provider_service: provider_service.clone(),
                },
                mcp_manager.clone(),
            )
            .await?;
        }
        Some(Commands::Dashboard {
            port,
            host,
            token,
            web_dist,
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
                    max_connections: 16,
                    web_dist,
                    trajectory_store: Some(Arc::clone(&trajectory_store)),
                    provider_service: provider_service.clone(),
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
                provider_service.clone(),
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
    fn permission_timeout_defaults_to_60s_and_honors_config() {
        let bare = load_test_config("");
        let bare = bare.inner().permission.clone().unwrap_or_default();
        assert_eq!(
            permission_timeout(&bare),
            std::time::Duration::from_secs(60)
        );

        let tuned = load_test_config("[permission]\ntimeout_ms = 800");
        let tuned = tuned.inner().permission.clone().unwrap_or_default();
        assert_eq!(
            permission_timeout(&tuned),
            std::time::Duration::from_millis(800)
        );
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
    fn run_parses_provider_and_model_flags() {
        let cli = Cli::try_parse_from([
            "agentj",
            "run",
            "--provider",
            "opencode-go",
            "--model",
            "m1",
            "hi",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Run {
                prompt,
                attach,
                provider,
                model,
            }) => {
                assert_eq!(prompt, "hi");
                assert!(attach.is_none());
                assert_eq!(provider.as_deref(), Some("opencode-go"));
                assert_eq!(model.as_deref(), Some("m1"));
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_flags_absent_are_none() {
        let cli = Cli::try_parse_from(["agentj", "run", "hi"]).unwrap();
        match cli.command {
            Some(Commands::Run {
                provider, model, ..
            }) => {
                assert!(provider.is_none() && model.is_none());
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_model_without_provider_is_accepted() {
        let cli = Cli::try_parse_from(["agentj", "run", "--model", "m1", "hi"]).unwrap();
        match cli.command {
            Some(Commands::Run {
                provider, model, ..
            }) => {
                assert!(provider.is_none());
                assert_eq!(model.as_deref(), Some("m1"));
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_provider_without_value_is_usage_error() {
        assert!(Cli::try_parse_from(["agentj", "run", "--provider"]).is_err());
    }

    #[test]
    fn run_blank_provider_is_passed_through() {
        let cli = Cli::try_parse_from(["agentj", "run", "--provider", "   ", "hi"]).unwrap();
        match cli.command {
            Some(Commands::Run { provider, .. }) => assert_eq!(provider.as_deref(), Some("   ")),
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_empty_provider_is_passed_through() {
        let cli = Cli::try_parse_from(["agentj", "run", "--provider", "", "hi"]).unwrap();
        match cli.command {
            Some(Commands::Run { provider, .. }) => assert_eq!(provider.as_deref(), Some("")),
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn run_blank_model_is_passed_through() {
        let cli = Cli::try_parse_from(["agentj", "run", "--model", "   ", "hi"]).unwrap();
        match cli.command {
            Some(Commands::Run { model, .. }) => assert_eq!(model.as_deref(), Some("   ")),
            _ => panic!("expected run subcommand"),
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

    // std temp dirs (no new dev-deps; CLI has no tempfile).
    fn temp_dist_dir(tag: &str, with_index: bool) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentj-cli-web-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if with_index {
            std::fs::write(dir.join("index.html"), "<html></html>").unwrap();
        }
        dir
    }

    #[test]
    fn resolve_web_dist_prefers_explicit_and_stops_on_invalid() {
        let explicit = temp_dist_dir("explicit", true);
        let cwd = temp_dist_dir("cwd-valid", true);
        assert_eq!(
            resolve_web_dist(Some(explicit.clone()), None, &cwd, &cwd),
            Some(explicit.clone())
        );
        let bad = temp_dist_dir("explicit-bad", false);
        assert_eq!(
            resolve_web_dist(Some(bad), None, &cwd, &cwd),
            None,
            "invalid explicit source must stop discovery"
        );
        std::fs::remove_dir_all(&explicit).ok();
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn resolve_web_dist_falls_back_to_cwd_and_env_order() {
        let cwd = temp_dist_dir("cwd-order", false);
        let env_dir = temp_dist_dir("env-order", true);
        assert_eq!(
            resolve_web_dist(None, Some(env_dir.clone()), &cwd, &cwd),
            Some(env_dir.clone())
        );
        let cwd_valid = temp_dist_dir("cwd-valid2", true);
        let cwd_layout = temp_dist_dir("cwd-layout", false);
        let nested = cwd_layout.join("agentj-ui/apps/web/dist");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("index.html"), "<html></html>").unwrap();
        assert_eq!(
            resolve_web_dist(None, None, &cwd_layout, &cwd_valid),
            Some(nested.clone())
        );
        std::fs::remove_dir_all(&cwd).ok();
        std::fs::remove_dir_all(&env_dir).ok();
        std::fs::remove_dir_all(&cwd_valid).ok();
        std::fs::remove_dir_all(&cwd_layout).ok();
    }

    #[test]
    fn resolve_web_dist_uses_exe_relative_candidate() {
        let base = std::env::temp_dir().join(format!("agentj-cli-exe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let exe_dir = base.join("bin");
        let share = base.join("share/agentj/web");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&share).unwrap();
        std::fs::write(share.join("index.html"), "<html></html>").unwrap();
        let empty_cwd = base.join("nowhere");
        let found = resolve_web_dist(None, None, &empty_cwd, &exe_dir);
        assert_eq!(
            found.map(|p| p.canonicalize().unwrap()),
            Some(share.canonicalize().unwrap()),
            "exe-relative ../share/agentj/web must be discovered"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn serve_and_dashboard_accept_web_dist_flag() {
        let cli =
            Cli::try_parse_from(["agentj", "serve", "--port", "0", "--web-dist", "/tmp/dist"])
                .unwrap();
        match cli.command {
            Some(Commands::Serve { web_dist, .. }) => {
                assert_eq!(web_dist.as_deref(), Some(std::path::Path::new("/tmp/dist")))
            }
            _ => panic!("expected serve subcommand"),
        }
        let cli = Cli::try_parse_from([
            "agentj",
            "dashboard",
            "--port",
            "4096",
            "--web-dist",
            "/tmp/dist",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Dashboard { web_dist, .. }) => {
                assert_eq!(web_dist.as_deref(), Some(std::path::Path::new("/tmp/dist")))
            }
            _ => panic!("expected dashboard subcommand"),
        }
    }

    #[test]
    fn run_params_json_omits_absent_overrides() {
        let line = run_params_json("s1", "hi", None, None);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "session/run");
        assert_eq!(v["id"], 2);
        assert_eq!(v["params"].as_object().unwrap().len(), 2);
        assert!(v["params"].get("provider").is_none());
        assert!(v["params"].get("model").is_none());
    }

    #[test]
    fn run_params_json_carries_overrides() {
        let line = run_params_json("s1", "say \"hi\"", Some("opencode-go"), None);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["params"]["provider"], "opencode-go");
        assert_eq!(v["params"]["content"], "say \"hi\"");
        assert!(v["params"].get("model").is_none());
    }

    #[test]
    fn run_error_line_omits_code_for_internal() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"boom"}}"#,
        )
        .unwrap();
        assert_eq!(run_error_line(&v).as_deref(), Some("error: boom"));
    }

    #[test]
    fn run_error_line_includes_other_codes() {
        let cancelled: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32800,"message":"boom"}}"#,
        )
        .unwrap();
        assert_eq!(
            run_error_line(&cancelled).as_deref(),
            Some("error: boom (code -32800)")
        );
        let invalid: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"boom"}}"#,
        )
        .unwrap();
        assert_eq!(
            run_error_line(&invalid).as_deref(),
            Some("error: boom (code -32602)")
        );
    }

    #[test]
    fn run_error_line_is_none_for_result() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":2,"result":{"content":"x"}}"#).unwrap();
        assert!(run_error_line(&v).is_none());
    }
}
