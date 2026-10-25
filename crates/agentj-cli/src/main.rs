//! agentj-cli — the `agentj` binary: run a single prompt, or serve the JSON-RPC API
//! over stdio. Production wiring: AgentLoop registers into the Kernel as a module so
//! init/shutdown run in the real process; stdin EOF and signals share the same
//! graceful-shutdown chain (design #13).

use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};

use agentj_core::DefaultContextPipeline;
use agentj_core::dispatcher::SubAgentService;
use agentj_core::kernel::Kernel;
use agentj_core::module::{
    ConfigProvider, KernelContext, KernelMessage, ModuleRegister, ProviderRegistry, SessionStore,
    ToolRegistry,
};
use agentj_core::skills::load_skills;
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::TokenUsage;
use agentj_ipc::IpcServer;
use agentj_mcp::McpConnectionManager;
use agentj_question::{AskTool, QuestionModule};
use agentj_session::FileBackedStore;
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
mod usage_history;

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
        /// Approve every permission request without asking (scripts, CI).
        #[arg(long, short = 'y')]
        yes: bool,
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

/// Approval wait budget from `[permission] timeout_ms`, clamped to 100..=600_000
/// (0, a negative value or anything else below the minimum clamps up to 100 with a
/// warning, not an error). The field is a signed `Option<i64>` so a negative raw
/// value survives deserialization and can be reported as written.
fn permission_timeout(cfg: &agentj_permission::PermissionConfig) -> std::time::Duration {
    let (budget, warning) = clamp_permission_timeout(cfg.timeout_ms);
    if let Some(line) = warning {
        eprintln!("{line}");
    }
    budget
}

/// Pure clamp + warning text for a raw `[permission] timeout_ms`, so the warning
/// line (it echoes the RAW value the user wrote) is unit-testable exactly.
fn clamp_permission_timeout(raw: Option<i64>) -> (std::time::Duration, Option<String>) {
    let raw = raw.unwrap_or(60_000);
    let clamped = raw.clamp(100, 600_000);
    let warning = (clamped != raw)
        .then(|| format!("warning: permission.timeout_ms {raw} clamped to {clamped}"));
    (std::time::Duration::from_millis(clamped as u64), warning)
}

/// Inject the configured question budget onto the `QuestionModule`, read through
/// the CLAMPING accessor (`[agent] question_timeout_s` is an unclamped raw i64,
/// so `0`/`-5`/`999999` must clamp before the u64 cast). Called ONCE at the
/// construction site before command dispatch so every assembly path — `run`, the
/// three serve modes, dashboard and the `None` arm — inherits the same default
/// (design D4): on the serve paths a notifier exists, so without this default an
/// `ask` omitting `timeout_s` would block forever.
fn apply_question_timeout(module: &QuestionModule, config: &agentj_config::Config) {
    module.set_default_timeout_s(Some(config.question_timeout_s()));
}

/// Map one stdin approval line to a decision. `y`/`yes` allow; anything else —
/// including EOF (closed pipe / Ctrl-D), represented here as `None` — denies.
/// Pure so the TTY approver's mapping is unit-tested without a TTY.
fn decision_from_input(input: Option<String>) -> agentj_permission::ApprovalDecision {
    let allow = input
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "y" | "yes"));
    if allow {
        agentj_permission::ApprovalDecision::Allow
    } else {
        agentj_permission::ApprovalDecision::Deny
    }
}

/// Argument fields a compact approval prompt shows verbatim, most actionable
/// first (a raw JSON dump buries the string the operator has to judge).
const APPROVAL_ARG_KEYS: [&str; 3] = ["command", "pattern", "path"];

/// Cap on the argument summary so a huge command cannot flood the terminal.
const APPROVAL_ARG_MAX_CHARS: usize = 200;

/// One-line, length-bounded summary of a permission request's arguments: the
/// first recognised high-signal field as a bare string (`command` for bash,
/// `pattern` for glob/grep, `path` for the file tools), else the compact JSON
/// object. Whitespace collapses to single spaces so the prompt stays on one line.
fn compact_args(args: &serde_json::Value) -> String {
    let raw = APPROVAL_ARG_KEYS
        .iter()
        .find_map(|key| args.get(key).and_then(serde_json::Value::as_str))
        .map_or_else(|| args.to_string(), str::to_string);
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(APPROVAL_ARG_MAX_CHARS) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}

/// The interactive approval prompt printed to stderr before the decision is read:
/// the tool name, a compact one-line argument summary and the engine's reason.
/// Pure so the exact prompt is unit-tested without a TTY.
fn approval_prompt(tool: &str, args: &serde_json::Value, reason: &str) -> String {
    format!(
        "approve {tool}? {}\n  reason: {reason}\n[y/N] ",
        compact_args(args)
    )
}

/// Read one approval line from stdin (the interactive TTY approver). Runs on its
/// own thread — never on the permission request path — so a pending human answer
/// cannot block the approval timeout. EOF (closed pipe) denies without hanging.
fn read_decision_from_stdin() -> agentj_permission::ApprovalDecision {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => agentj_permission::ApprovalDecision::Deny,
        Ok(_) => decision_from_input(Some(line)),
        Err(_) => agentj_permission::ApprovalDecision::Deny,
    }
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

/// Pure resolution of the metrics state directory (design D4): an explicit
/// `[web] state_dir` wins, then `$XDG_DATA_HOME/agentj`, then
/// `$HOME/.local/share/agentj`; `None` means "keep the counters in memory".
///
/// An *empty* configured `state_dir` is treated as unset: `PathBuf::from("")` is not a
/// directory anyone can write to, but it used to win the chain and silently hand the
/// callers a path that resolves to the process CWD.
fn metrics_state_dir_from(
    cfg: &agentj_config::Config,
    xdg: Option<&str>,
    home: Option<&str>,
) -> Option<std::path::PathBuf> {
    let explicit = cfg
        .inner()
        .web
        .as_ref()
        .and_then(|web| web.state_dir.clone())
        .filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = explicit {
        return Some(dir);
    }
    if let Some(xdg) = xdg.filter(|value| !value.is_empty()) {
        return Some(Path::new(xdg).join("agentj"));
    }
    let home = home.filter(|value| !value.is_empty())?;
    Some(Path::new(home).join(".local").join("share").join("agentj"))
}

fn resolve_metrics_state_dir(cfg: &agentj_config::Config) -> Option<std::path::PathBuf> {
    metrics_state_dir_from(
        cfg,
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// Pure mapping from `[web]` to the ipc-layer integrity knobs; an absent section
/// keeps `IntegrityOptions::default()` (60s / 8 / on / 60s).
fn resolve_integrity_options(cfg: &agentj_config::Config) -> agentj_ipc::web::IntegrityOptions {
    let Some(web) = cfg.inner().web.as_ref() else {
        return agentj_ipc::web::IntegrityOptions::default();
    };
    agentj_ipc::web::IntegrityOptions {
        spot_interval: std::time::Duration::from_secs(web.clamped_spot_interval_secs()),
        spot_sample: web.clamped_spot_sample(),
        verify_on_request: web.verify_on_request,
        verify_cache_ttl: std::time::Duration::from_secs(web.clamped_verify_cache_ttl_secs()),
    }
}

/// Warn when forwarding trust is enabled on a non-loopback bind: the trust
/// predicate is the direct peer, so remote clients' headers stay ignored.
fn proxy_trust_warning(host: &str, trust: bool) -> Option<String> {
    if !trust || is_loopback_host(host) {
        return None;
    }
    Some(format!(
        "warning: [web] trust_proxy_headers is enabled but {host} is not loopback; forwarded headers \
         are ignored unless the direct peer is loopback (put a same-host proxy in front)"
    ))
}

fn metrics_state_warning(dir: Option<&Path>) -> Option<String> {
    dir.is_none().then(|| {
        "warning: no metrics state directory (set XDG_DATA_HOME, HOME or [web] state_dir); login \
         counters stay in memory and reset on restart"
            .to_string()
    })
}

/// `[web] state_dir` → `$XDG_DATA_HOME/agentj/usage` → `$HOME/.local/share/agentj/usage`.
/// Mirrors `metrics_state_dir_from`; the extra `usage/` keeps the sample files out of the
/// metrics directory, and `usage.history_enabled = false` means "no directory at all".
fn usage_history_dir_from(
    cfg: &agentj_config::Config,
    xdg: Option<&str>,
    home: Option<&str>,
) -> Option<std::path::PathBuf> {
    if cfg.get_bool("usage.history_enabled") == Some(false) {
        return None;
    }
    metrics_state_dir_from(cfg, xdg, home).map(|dir| dir.join("usage"))
}

fn resolve_usage_history_dir(cfg: &agentj_config::Config) -> Option<std::path::PathBuf> {
    usage_history_dir_from(
        cfg,
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// `usage.history_days` / `usage.history_max_samples` with the store's own defaults as the
/// fallback; `UsageHistory::with_limits` clamps whatever the config supplied.
fn usage_history_limits(cfg: &agentj_config::Config) -> (i64, usize) {
    let days = cfg
        .get_int("usage.history_days")
        .unwrap_or(usage_history::DEFAULT_RETENTION_DAYS);
    let max_samples = cfg
        .get_int("usage.history_max_samples")
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(usage_history::DEFAULT_MAX_SAMPLES);
    (days, max_samples)
}

fn usage_history_warning(cfg: &agentj_config::Config, dir: Option<&Path>) -> Option<String> {
    if cfg.get_bool("usage.history_enabled") == Some(false) || dir.is_some() {
        return None;
    }
    Some(
        "warning: no usage history directory (set XDG_DATA_HOME, HOME or [web] state_dir); \
         usage history is disabled until one exists"
            .to_string(),
    )
}

fn record_completed_turn(
    collector: &agentj_core::UsageCollector,
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
    //
    // BUG-019: create — and therefore install — the OS signal handlers *synchronously*
    // here, before this function returns. `tokio::signal` installs the process-global
    // handler on the first `signal()` call; `tokio::spawn` is lazy, so if the streams
    // were created inside the async block, a signal delivered before the task's first
    // poll would hit the default disposition and kill the process with the signal's wait
    // status (15 for SIGTERM) instead of running the graceful shutdown chain. The caller
    // invokes this before `serve_stdio()`, so the handler is guaranteed live before the
    // serve loop can answer any request. `SignalKind::interrupt()` is the SIGINT
    // equivalent of `ctrl_c()` and lets us install both handlers up front.
    #[cfg(unix)]
    let (mut sigint, mut sigterm) = (
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap(),
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap(),
    );
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = sigint.recv() => {}
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
    // BUG-019: install the first-wait handlers synchronously before spawning, exactly as
    // in `spawn_signal_task`; the caller runs this before `tcp.run()` enters its loop.
    //
    // The second wait keeps its streams created lazily *inside* the task. Preparing both
    // pairs up front would make a single SIGTERM notify `sigint2`/`sigterm2` as well, so
    // the force-exit path would fire immediately instead of on a second signal (verified:
    // exit 130 vs 0 — a semantic regression). The second pair is only reached after the
    // first signal has been received, i.e. once the task is already running, so it has no
    // handler-installation race.
    #[cfg(unix)]
    let (mut sigint, mut sigterm) = (
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap(),
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap(),
    );
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = sigint.recv() => {}
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
            let (mut sigint2, mut sigterm2) = (
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap(),
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap(),
            );
            tokio::select! {
                _ = sigint2.recv() => {}
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
    /// `[web] trust_proxy_headers`: trust forwarded headers from a loopback peer.
    trust_proxy_headers: bool,
    /// `[web] secure_cookies`: mark issued session cookies `Secure` (TLS in front).
    secure_cookies: bool,
    /// `[web] state_dir` (or the XDG/HOME fallback) for the persisted limiter state.
    metrics_state_dir: Option<std::path::PathBuf>,
    /// Startup warning when the persisted usage history has no directory; it rides the same
    /// `[web] state_dir` chain, under `usage/`.
    usage_history_warning: Option<String>,
    /// Optional subscription handle (design D4); `run` passes none.
    provider_service: Option<Arc<dyn agentj_ipc::ProviderService>>,
    /// `[web]` spot cadence + per-request verification knobs.
    integrity: agentj_ipc::web::IntegrityOptions,
    /// `Config::validate()` output: every clamped/ignored knob, printed with the banner.
    config_warnings: Vec<String>,
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
    let warnings = serve_warnings(&options);
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
            trust_proxy_headers: options.trust_proxy_headers,
            secure_cookies: options.secure_cookies,
            metrics_state_dir: options.metrics_state_dir.clone(),
            provider_service: options.provider_service,
            integrity: options.integrity,
        },
    )
    .await?;
    let local = tcp.local_addr()?;
    // BUG-019: the shutdown handlers must be installed *before* the readiness output.
    // Supervisors act on `listening on` immediately (read line, send SIGTERM); if the
    // handler were installed afterwards, that SIGTERM would still hit the default
    // disposition and kill the process without a graceful shutdown.
    let signal_task = spawn_tcp_signal_task(tcp.shared_server());
    println!("listening on {local}");
    emit_serve_banner(
        &tcp.web_config(),
        &web_dist,
        &options.host,
        local.port(),
        warnings,
        &options.config_warnings,
    );
    // Resident mode: exits when the last client disconnects or a shutdown
    // request arrives (design #3, on-demand resident).
    let run_result = tcp.run().await;
    // Tear down unconditionally: a serve-loop error must not skip the metrics flush.
    shutdown_tcp_server(&tcp, signal_task, &mcp_manager, run_result).await
}

/// The config-level startup warnings in banner order: insecure bind, metrics state,
/// proxy trust, usage history.
fn serve_warnings(options: &TcpServeOptions) -> [Option<String>; 4] {
    [
        insecure_bind_warning(&options.host, options.token.as_deref()),
        metrics_state_warning(options.metrics_state_dir.as_deref()),
        proxy_trust_warning(&options.host, options.trust_proxy_headers),
        options.usage_history_warning.clone(),
    ]
}

/// Print the readiness banner (websocket + resolved web-dist line) and the
/// config-level warnings in the order the CLI has always emitted them:
/// websocket → dist → web config, then the optional warnings in
/// [insecure-bind, metrics-state, proxy-trust, usage-history] order.
fn emit_serve_banner(
    web: &agentj_ipc::web::WebConfig,
    web_dist: &Option<std::path::PathBuf>,
    host: &str,
    port: u16,
    warnings: [Option<String>; 4],
    config_warnings: &[String],
) {
    println!("websocket on ws://{}:{}/ws", bracket_ipv6(host), port);
    match web_dist {
        Some(dir) => println!(
            "web ui on http://{}:{}/ (dist: {})",
            bracket_ipv6(host),
            port,
            dir.display()
        ),
        None => eprintln!(
            "warning: web dist not found; build it via `npm --prefix agentj-ui run build \
             --workspace=@agentj/web` or pass --web-dist <PATH>"
        ),
    }
    for warning in web.startup_warnings() {
        eprintln!("{warning}");
    }
    for warning in warnings.into_iter().flatten() {
        eprintln!("{warning}");
    }
    // Last, so an operator reading the banner tail sees every silently adjusted knob.
    for warning in config_warnings {
        eprintln!("{warning}");
    }
}

/// Tear the resident server down unconditionally: persist the limiter state
/// before aborting the debounce task, release the MCP connections, then surface
/// any error the serve loop returned.
async fn shutdown_tcp_server(
    tcp: &agentj_ipc::tcp::TcpIpcServer,
    signal_task: tokio::task::JoinHandle<()>,
    mcp_manager: &McpConnectionManager,
    run_result: Result<(), agentj_core::error::IpcError>,
) -> anyhow::Result<()> {
    tcp.web_config().flush_metrics_state();
    tcp.web_config().stop_metrics_flush();
    signal_task.abort();
    mcp_manager.close_all().await;
    run_result?;
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

/// One decoded JSON-RPC frame from a `run --attach` stream. Keeping the decision in a
/// single value instead of inline `if`s in the read loop is what stops a reply frame from
/// being mistaken for a fresh request (and denied again, forever).
#[derive(Debug, Clone, PartialEq, Eq)]
enum AttachAction {
    DenyApproval {
        request_id: String,
        tool: String,
    },
    /// The kernel is waiting on an `ask` tool; attach is not an interactive client, so it
    /// settles the question as skipped instead of letting the 600s timeout run.
    SkipQuestion {
        question_id: String,
    },
    Reply(String),
    Fail(String),
    Ignore,
}

fn decode_attach_frame(v: &serde_json::Value) -> AttachAction {
    if v.get("method").and_then(|m| m.as_str()) == Some("$/permissionRequest") {
        let params = &v["params"];
        return match params.get("request_id").and_then(|r| r.as_str()) {
            Some(request_id) => AttachAction::DenyApproval {
                request_id: request_id.to_string(),
                tool: params
                    .get("tool")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
            },
            None => AttachAction::Ignore,
        };
    }
    if v.get("method").and_then(|m| m.as_str()) == Some("$/askQuestion") {
        return match v["params"].get("question_id").and_then(|q| q.as_str()) {
            Some(question_id) => AttachAction::SkipQuestion {
                question_id: question_id.to_string(),
            },
            None => AttachAction::Ignore,
        };
    }
    if let Some(failure) = run_error_line(v) {
        return AttachAction::Fail(failure);
    }
    match v["result"]["content"].as_str() {
        Some(text) => AttachAction::Reply(text.to_string()),
        None => AttachAction::Ignore,
    }
}

/// True when an error frame answers one of the denies attach itself wrote (a settled or
/// rejected `permission/respond`). Those must not abort the attached run.
fn is_attach_deny_reply(frame: &serde_json::Value, deny_ids: &[u64]) -> bool {
    frame
        .get("id")
        .and_then(|id| id.as_u64())
        .is_some_and(|id| deny_ids.contains(&id))
}

/// The line `run --attach` prints when the remote kernel asks for an approval: attach is a
/// non-interactive client, so it can only refuse and say how to approve for real.
fn attach_deny_warning(tool: &str, request_id: &str) -> String {
    format!(
        "warning: run --attach cannot answer approvals; denying {tool} ({request_id}) — \
         attach again with an interactive serve client, or run `agentj serve` and approve there"
    )
}

/// A `permission/respond` request denying `request_id`, ready to write as one line.
fn attach_deny_request(request_id: &str, id: u64) -> String {
    let params = agentj_protocol::permission::PermissionRespondParams {
        request_id: request_id.to_string(),
        decision: agentj_protocol::permission::PermissionDecision::Deny,
    };
    let body = serde_json::to_string(&params).expect("a deny decision always serializes");
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"permission/respond\",\"params\":{body}}}\n"
    )
}

/// `question/respond` body attach writes: the question is settled as skipped, which the
/// kernel reports to the model honestly instead of blocking until the ask timeout.
fn attach_skip_question_request(question_id: &str, id: u64) -> String {
    let params = agentj_protocol::question::QuestionRespondParams {
        question_id: question_id.to_string(),
        answers: Vec::new(),
        skipped: true,
    };
    let body = serde_json::to_string(&params).expect("a skipped answer always serializes");
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"question/respond\",\"params\":{body}}}\n"
    )
}

/// The line `run --attach` prints when the remote kernel asks a question: attach answers it
/// as skipped, and says how to answer for real.
fn attach_skip_question_warning(question_id: &str) -> String {
    format!(
        "warning: run --attach cannot answer questions; skipping ({question_id}) — \
         answer it in an interactive serve client, or run `agentj run` locally"
    )
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
    read_attach_stream(&mut reader, &mut w).await
}

/// Drain the run's frame stream until it concludes. Returns the failure text when the
/// run failed (the caller prints it and exits non-zero), or `None` on a delivered reply.
/// Read one decodable frame, skipping lines that are not JSON (banners, partial writes).
async fn read_attach_frame<R>(reader: &mut R) -> anyhow::Result<Option<serde_json::Value>>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        if let Ok(frame) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            return Ok(Some(frame));
        }
    }
}

/// Write one request attach owns (a deny or a skip) and remember its id, so a refusal of
/// *that* write is not mistaken for the run failing.
async fn send_own_request<W, F>(
    w: &mut W,
    next_id: &mut u64,
    own_ids: &mut Vec<u64>,
    build: F,
    what: &str,
) where
    W: tokio::io::AsyncWrite + Unpin,
    F: FnOnce(u64) -> String,
{
    use tokio::io::AsyncWriteExt;
    let id = *next_id;
    *next_id += 1;
    own_ids.push(id);
    if let Err(err) = w.write_all(build(id).as_bytes()).await {
        tracing::warn!(%err, "attach: failed to {what}");
    }
}

async fn read_attach_stream<R, W>(reader: &mut R, w: &mut W) -> anyhow::Result<Option<String>>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut next_id = 2u64;
    // Ids of the requests attach itself wrote (a deny or a skip), so their error replies
    // are not mistaken for the run failing.
    let mut own_ids: Vec<u64> = Vec::new();
    loop {
        let Some(frame) = read_attach_frame(reader).await? else {
            return Ok(Some(
                "attach: connection closed before the run finished".into(),
            ));
        };
        match decode_attach_frame(&frame) {
            AttachAction::DenyApproval { request_id, tool } => {
                eprintln!("{}", attach_deny_warning(&tool, &request_id));
                let build = |id| attach_deny_request(&request_id, id);
                send_own_request(
                    w,
                    &mut next_id,
                    &mut own_ids,
                    build,
                    "send the deny decision",
                )
                .await;
            }
            AttachAction::SkipQuestion { question_id } => {
                eprintln!("{}", attach_skip_question_warning(&question_id));
                let build = |id| attach_skip_question_request(&question_id, id);
                send_own_request(w, &mut next_id, &mut own_ids, build, "settle the question").await;
            }
            AttachAction::Fail(failure) => {
                // A refused deny answers *our* write, not the run: the remote turn is still
                // going, so report the bookkeeping failure and keep reading instead of
                // aborting with an error that looks like the run's own.
                if is_attach_deny_reply(&frame, &own_ids) {
                    eprintln!("warning: attach deny was refused: {failure}");
                    continue;
                }
                return Ok(Some(failure));
            }
            AttachAction::Reply(text) => {
                println!("{text}");
                return Ok(None);
            }
            AttachAction::Ignore => continue,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let cli = Cli::parse();
    run_command(cli).await
}

/// Load `--config` (the load error is surfaced verbatim, as the CLI always has).
fn load_config(cli: &Cli) -> anyhow::Result<agentj_config::Config> {
    agentj_config::Config::load(Some(cli.config.clone().into())).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Entitlement gate for the parsed subcommand; `None` means "ungated".
fn license_gate(command: &Option<Commands>) -> Option<(&'static str, &'static str)> {
    match command {
        Some(Commands::Run { attach: None, .. }) => Some(("run", "run")),
        Some(Commands::Run {
            attach: Some(_), ..
        }) => None,
        Some(Commands::Serve { .. }) => Some(("serve", "serve")),
        Some(Commands::Dashboard { .. }) => Some(("dashboard", "dashboard")),
        Some(Commands::Auth { .. }) | None => None,
    }
}

/// What the chosen subcommand needs from the startup assembly.
#[derive(Clone, Copy)]
struct CommandMode {
    /// `run --attach` talks to a remote kernel, so local providers are not required.
    attached: bool,
    /// design D4: only the IPC-serving commands expose the subscription methods.
    serves_clients: bool,
}

impl CommandMode {
    fn classify(command: &Option<Commands>) -> Self {
        Self {
            attached: matches!(
                command,
                Some(Commands::Run {
                    attach: Some(_),
                    ..
                })
            ),
            serves_clients: matches!(
                command,
                Some(Commands::Serve { .. }) | Some(Commands::Dashboard { .. })
            ),
        }
    }
}

/// Everything `main` used to do after `Cli::parse()`: assemble the kernel, then
/// dispatch to the per-subcommand entry point.
async fn run_command(cli: Cli) -> anyhow::Result<()> {
    if let Some(Commands::Auth { action }) = &cli.command {
        return auth::run_auth(action, &cli.config).await;
    }
    let config = load_config(&cli)?;
    if let Some((entitlement, command_name)) = license_gate(&cli.command) {
        license::check_license_gate(
            &config,
            cli.license_key.as_deref(),
            entitlement,
            command_name,
        )?;
    }
    let wiring = wire_kernel(&cli.command, config).await?;
    dispatch(cli.command, wiring).await?;
    // The serve loop reads stdin through tokio's blocking pool; dropping the
    // runtime would wait for that thread while a client keeps stdin open.
    // Graceful shutdown already ran above — exit directly.
    std::process::exit(0);
}

/// Startup state handed from the assembly phase to the dispatch phase.
struct Wiring {
    kernel: Kernel,
    session: Arc<dyn SessionStore>,
    permission_module: Arc<agentj_permission::PermissionModule>,
    question_module: Arc<QuestionModule>,
    trajectory_store: Arc<agentj_ipc::trajectory::TrajectoryStore>,
    provider_service: Option<Arc<dyn agentj_ipc::ProviderService>>,
    mcp_manager: Arc<McpConnectionManager>,
    web: WebSettings,
}

/// Assemble the kernel in the exact order the CLI has always performed it: every
/// side effect (stores, licence-warned logs, provider assembly, MCP connects) keeps
/// its original position so `run`/`serve`/`dashboard` behave identically.
async fn wire_kernel(
    command: &Option<Commands>,
    config: agentj_config::Config,
) -> anyhow::Result<Wiring> {
    let mode = CommandMode::classify(command);
    let question_module = Arc::new(QuestionModule::new());
    apply_question_timeout(&question_module, &config);
    let tools = build_tool_registry(&question_module);
    let providers = Arc::new(ProviderRegistry::new());
    let sessions = build_sessions(&config)?;
    assemble_providers(&config, mode, &providers).await?;
    let provider_service = build_provider_service(&config, mode, &providers)?;
    let permission = resolve_permission_config(&config);
    let mcp_servers = config.inner().mcp.clone();
    let web = WebSettings::resolve(&config);
    let ctx = KernelContext {
        session: sessions.session.clone(),
        tools,
        providers,
        config: Arc::new(config),
    };
    let mut kernel = Kernel::new();
    let permission_module = register_modules(&mut kernel, &ctx, permission)?;
    let events = kernel.events();
    let mcp_manager = start_mcp_manager(&ctx.tools, events.clone(), mcp_servers.as_ref()).await;
    sessions
        .store
        .spawn_expiry_loop(events, Duration::from_secs(3600), Duration::from_secs(60));
    log_loaded_skills();
    Ok(Wiring {
        kernel,
        session: sessions.session,
        permission_module,
        question_module,
        trajectory_store: sessions.trajectory_store,
        provider_service,
        mcp_manager,
        web,
    })
}

/// The tool registry: builtins plus the `ask` tool backed by the question module.
fn build_tool_registry(question_module: &Arc<QuestionModule>) -> Arc<ToolRegistry> {
    let tools = Arc::new(ToolRegistry::new());
    register_builtins(&tools);
    tools.register(Arc::new(AskTool::new(Arc::clone(question_module))));
    tools
}

/// The session stack. The concrete `FileBackedStore` handle is kept because
/// `spawn_expiry_loop` is NOT object-safe (`self: Arc<Self>`).
struct Sessions {
    store: Arc<FileBackedStore>,
    session: Arc<dyn SessionStore>,
    trajectory_store: Arc<agentj_ipc::trajectory::TrajectoryStore>,
}

fn build_sessions(config: &agentj_config::Config) -> anyhow::Result<Sessions> {
    let session_dir = resolve_session_dir(config);
    let session_cfg = config.inner().session.as_ref();
    let trajectory_store = Arc::new(agentj_ipc::trajectory::TrajectoryStore::new(
        session_dir.clone(),
        session_cfg.and_then(|s| s.trajectory_max_events),
        session_cfg.and_then(|s| s.trajectory_max_bytes),
    ));
    let store = Arc::new(FileBackedStore::new(&session_dir)?);
    let session: Arc<dyn SessionStore> = store.clone() as Arc<dyn SessionStore>;
    Ok(Sessions {
        store,
        session,
        trajectory_store,
    })
}

/// `[model] console_base_url` with the opencode console default.
fn console_base_url(config: &agentj_config::Config) -> String {
    config
        .get_str("model.console_base_url")
        .unwrap_or_else(|| "https://opencode.ai/console".into())
}

/// design 4.3 + D7: assemble providers with credential resolution; keyless real
/// providers are skipped (never an empty Bearer). An empty registry is fatal only
/// where no IPC client can link a credential later (`run`); `serve`/`dashboard`
/// boot on and let Settings · Subscription activate a provider at runtime.
/// `run --attach` talks to a remote kernel, so local providers are not required.
async fn assemble_providers(
    config: &agentj_config::Config,
    mode: CommandMode,
    registry: &Arc<ProviderRegistry>,
) -> anyhow::Result<()> {
    if mode.attached {
        return Ok(());
    }
    let auth_store =
        agentj_auth::AuthStore::with_default_path().map_err(|e| anyhow::anyhow!("{e}"))?;
    let live_base = agentj_model::catalog::ModelCatalog::builtin().base_url;
    let console_base = console_base_url(config);
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
    // An empty provider set is fatal only where no client can link a credential later:
    // `serve`/`dashboard` boot on and let Settings · Subscription activate a provider at
    // runtime (design D2/D3/D7), while `run` has no IPC client and must fail fast.
    if assembly.providers.is_empty() && !mode.serves_clients {
        anyhow::bail!("{}", providers::NO_USABLE_PROVIDER);
    }
    for warning in &assembly.warnings {
        tracing::warn!("{warning}");
    }
    for (name, provider) in assembly.providers {
        registry.register(name, provider);
    }
    Ok(())
}

/// The IPC-facing provider service, present only on the commands that serve clients.
fn build_provider_service(
    config: &agentj_config::Config,
    mode: CommandMode,
    providers: &Arc<ProviderRegistry>,
) -> anyhow::Result<Option<Arc<dyn agentj_ipc::ProviderService>>> {
    if !mode.serves_clients {
        return Ok(None);
    }
    let store = agentj_auth::AuthStore::with_default_path().map_err(|e| anyhow::anyhow!("{e}"))?;
    let default_model = config.get_str("model.default_model").unwrap_or_default();
    let (history_days, history_max_samples) = usage_history_limits(config);
    let svc = provider_service::CliProviderService::new(
        store,
        provider_cfgs(config),
        config.get_str("model.provider").unwrap_or_default(),
        console_base_url(config),
        config.get_int("model.quota_warn_percent").unwrap_or(80) as u8,
        Arc::clone(providers),
    )
    .with_default_model(default_model)
    .with_history_dir(resolve_usage_history_dir(config))
    .with_history_limits(history_days, history_max_samples);
    Ok(Some(Arc::new(svc)))
}

/// The validated `[permission]` config plus its policy map, extracted before
/// `config` is boxed into the KernelContext (whose `config` field erases the
/// concrete `Config` type).
struct PermissionSetup {
    cfg: agentj_permission::PermissionConfig,
    policies: std::collections::HashMap<String, agentj_permission::ToolPolicy>,
}

fn resolve_permission_config(config: &agentj_config::Config) -> PermissionSetup {
    let cfg = config.inner().permission.clone().unwrap_or_default();
    cfg.validate().expect("invalid [permission] config");
    let policies = permission_policies_from_config(config);
    PermissionSetup { cfg, policies }
}

/// `[web]` settings the serve paths need, resolved before the config is moved into
/// the kernel context (`TcpServeOptions` is built later).
struct WebSettings {
    trust_proxy_headers: bool,
    secure_cookies: bool,
    metrics_state_dir: Option<std::path::PathBuf>,
    usage_history_warning: Option<String>,
    integrity: agentj_ipc::web::IntegrityOptions,
    config_warnings: Vec<String>,
}

impl WebSettings {
    fn resolve(config: &agentj_config::Config) -> Self {
        let web = config.inner().web.as_ref();
        Self {
            trust_proxy_headers: web.is_some_and(|web| web.trust_proxy_headers),
            secure_cookies: web.is_some_and(|web| web.secure_cookies),
            metrics_state_dir: resolve_metrics_state_dir(config),
            usage_history_warning: usage_history_warning(
                config,
                resolve_usage_history_dir(config).as_deref(),
            ),
            integrity: resolve_integrity_options(config),
            config_warnings: config.validate(),
        }
    }
}

/// Register every kernel module in the historical order (tool executor, agent loop,
/// subagent service, then permission) and return the permission module, which both
/// the IPC server and the approval notifier need.
fn register_modules(
    kernel: &mut Kernel,
    ctx: &KernelContext,
    permission: PermissionSetup,
) -> anyhow::Result<Arc<agentj_permission::PermissionModule>> {
    let mut register = ModuleRegister::with_services(
        Arc::clone(&ctx.tools),
        Arc::clone(&ctx.providers),
        kernel.services(),
    );
    let usage = Arc::new(agentj_core::UsageCollector::new());
    kernel.register_module(
        Box::new(ToolExecutorModule::new(ctx.clone())),
        ctx,
        &mut register,
    )?;
    let runner = build_turn_runner(ctx, kernel.sender(), &usage);
    kernel.register_module(
        Box::new(agentj_core::agent_loop_module::AgentLoopModule::new(
            runner.clone(),
        )),
        ctx,
        &mut register,
    )?;
    kernel
        .services()
        .register_kernel_async_service("subagent".into(), Arc::new(SubAgentService::new(runner)));
    // Permission wiring (design #17): construct from [permission] config with
    // fail-fast rule validation, register with the kernel so graceful shutdown
    // clears grants + pending approvals, install the checker on the ToolRegistry,
    // and hand the module to the IPC server for permission/respond.
    let module = Arc::new(agentj_permission::PermissionModule::new(
        permission.policies,
        permission_timeout(&permission.cfg),
    ));
    ctx.tools.set_permission_checker(module.clone());
    kernel.register_module(Box::new(module.as_ref().clone()), ctx, &mut register)?;
    Ok(module)
}

/// The turn runner: context pipeline, system prompt and the usage recording hook.
fn build_turn_runner(
    ctx: &KernelContext,
    sender: agentj_core::kernel::KernelSender,
    usage: &Arc<agentj_core::UsageCollector>,
) -> TurnRunner {
    let mut runner = TurnRunner::new(
        ctx.clone(),
        Arc::new(DefaultContextPipeline::new()),
        "你是 AgentJ，一位专业的 AI 编程助手。请始终使用中文回答，代码注释和解释均使用中文，保持专业、简洁。".into(),
        sender,
        "tool_executor".into(),
    );
    let collector_clone = Arc::clone(usage);
    runner.set_on_turn_complete(Arc::new(move |sid, usage| {
        record_completed_turn(&collector_clone, sid.clone(), usage)
    }));
    runner
}

/// Spawn the MCP manager: idle reaper loop, then one connect attempt per configured
/// server (a failed connect is logged, never fatal).
async fn start_mcp_manager(
    tools: &Arc<ToolRegistry>,
    events: Arc<agentj_core::hooks::EventBus>,
    servers: Option<&agentj_config::McpConfig>,
) -> Arc<McpConnectionManager> {
    let mcp_manager = Arc::new(McpConnectionManager::new(
        Arc::clone(tools),
        Duration::from_secs(60),
        Duration::from_millis(500),
        5,
        events,
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
    if let Some(mcp_cfg) = servers {
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
    mcp_manager
}

/// Load the repo-local skills and log them (registration is deferred to the
/// manifest-compat research).
fn log_loaded_skills() {
    let skills = load_skills(Path::new(".")).unwrap_or_default();
    for skill in &skills {
        tracing::info!(skill = %skill.name, priority = ?skill.priority, "loaded");
    }
}

/// One entry point per subcommand; every arm owns the fully wired kernel.
async fn dispatch(command: Option<Commands>, wiring: Wiring) -> anyhow::Result<()> {
    match command {
        Some(Commands::Auth { .. }) => Ok(()),
        Some(Commands::Run {
            prompt,
            attach,
            provider,
            model,
            yes,
        }) => run_agent(wiring, prompt, attach, provider, model, yes).await,
        // `serve` without `--port` is the stdio server; with it, the resident kernel.
        Some(Commands::Serve {
            port,
            host,
            token,
            max_connections,
            web_dist,
        }) => match port {
            None => run_serve_stdio(wiring).await,
            Some(port) => run_serve(wiring, host, port, token, max_connections, web_dist).await,
        },
        Some(Commands::Dashboard {
            port,
            host,
            token,
            web_dist,
        }) => run_serve(wiring, host, port, token, 16, web_dist).await,
        None => run_without_command(wiring).await,
    }
}

/// `agentj run`: attached (remote kernel) or embedded (one in-process turn).
async fn run_agent(
    wiring: Wiring,
    prompt: String,
    attach: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    yes: bool,
) -> anyhow::Result<()> {
    if let Some(url) = attach.as_deref() {
        let failure = run_attached(url, &prompt, provider.as_deref(), model.as_deref()).await?;
        wiring.mcp_manager.close_all().await;
        // The failed path exits the process, exactly as the inline arm did.
        if let Some(line) = failure {
            eprintln!("{line}");
            std::process::exit(1);
        }
        return Ok(());
    }
    warn_if_no_approver(yes);
    attach_approver(&wiring.permission_module, yes);
    let ipc_server = build_ipc_server(
        wiring.kernel,
        wiring.session,
        wiring.permission_module,
        wiring.question_module,
        Some(wiring.trajectory_store),
        wiring.provider_service,
    );
    let signal_task = spawn_signal_task(ipc_server.kernel_sender());
    let sid = ipc_server.create_session()?;
    let overrides = agentj_core::types::RunOverrides { provider, model };
    let mut rx = ipc_server.run_turn_stream_with(&sid, &prompt, overrides);
    let (out, failed) = drain_turn(&mut rx).await;
    // Shut down before reporting: the failed path exits the process, which
    // would otherwise strand the embedded kernel and its signal task.
    ipc_server.signal_shutdown()?;
    drop(signal_task);
    wiring.mcp_manager.close_all().await;
    if let Some(e) = failed {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    println!("{out}");
    Ok(())
}

/// Say up front that nothing can approve tools for this run: the permission
/// module denies each approval-required tool at once (reason `no approver`),
/// and a piped run's reader never sees the tool results.
fn warn_if_no_approver(yes: bool) {
    if yes || std::io::stdin().is_terminal() {
        return;
    }
    eprintln!(
        "warning: no approver attached (non-interactive stdin, no --yes); \
         approval-required tools will be denied"
    );
}

/// Approval path: `--yes` auto-approves; a TTY spawns a thread that owns stdin and
/// answers y/n; without either nothing is attached, and the permission module then
/// denies each approval-required tool at once with reason `no approver` (no stdin
/// read on the request path, no wait for the approval timeout).
fn attach_approver(permission_module: &Arc<agentj_permission::PermissionModule>, yes: bool) {
    if yes {
        let module = Arc::clone(permission_module);
        permission_module.set_notifier(Arc::new(move |req, _session| {
            eprintln!("auto-approved {} (--yes)", req.tool);
            let _ = module.respond(
                &agentj_core::permission::PermissionRequestId(req.request_id.clone()),
                agentj_permission::ApprovalDecision::Allow,
            );
            // Delivered: the ask was answered synchronously above.
            true
        }));
    } else if std::io::stdin().is_terminal() {
        let module = Arc::clone(permission_module);
        permission_module.set_notifier(Arc::new(move |req, _session| {
            // Print the prompt synchronously (the notifier is the only
            // place holding the request details) before the detached
            // reader blocks; reading stdin on this thread would interlock
            // with the approval await.
            eprintln!("{}", approval_prompt(&req.tool, &req.args, &req.reason));
            let module = module.clone();
            let request_id = agentj_core::permission::PermissionRequestId(req.request_id.clone());
            std::thread::spawn(move || {
                let decision = read_decision_from_stdin();
                let _ = module.respond(&request_id, decision);
            });
            // Delivered: a stdin reader owns this request from here on.
            true
        }));
    }
}

/// Drain one turn's event stream: the `Done` text (or the concatenated deltas when no
/// `Done` arrives) plus the failure message, if any.
async fn drain_turn(
    rx: &mut tokio::sync::mpsc::Receiver<agentj_core::types::AgentEvent>,
) -> (String, Option<String>) {
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
    (out, failed)
}

/// `agentj serve` without `--port`: the stdio JSON-RPC server.
async fn run_serve_stdio(wiring: Wiring) -> anyhow::Result<()> {
    let mut ipc_server = build_ipc_server(
        wiring.kernel,
        wiring.session,
        wiring.permission_module,
        wiring.question_module,
        Some(wiring.trajectory_store),
        wiring.provider_service,
    );
    let signal_task = spawn_signal_task(ipc_server.kernel_sender());
    ipc_server.serve_stdio().await?;
    signal_task.abort();
    wiring.mcp_manager.close_all().await;
    Ok(())
}

/// The resident multi-client TCP kernel shared by `serve --port` and `dashboard`
/// (`dashboard` pins `max_connections` to its historical 16).
async fn run_serve(
    wiring: Wiring,
    host: String,
    port: u16,
    token: Option<String>,
    max_connections: usize,
    web_dist: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let web = wiring.web;
    let options = TcpServeOptions {
        host,
        port,
        token,
        max_connections,
        web_dist,
        trajectory_store: Some(wiring.trajectory_store),
        trust_proxy_headers: web.trust_proxy_headers,
        secure_cookies: web.secure_cookies,
        metrics_state_dir: web.metrics_state_dir,
        usage_history_warning: web.usage_history_warning,
        provider_service: wiring.provider_service,
        integrity: web.integrity,
        config_warnings: web.config_warnings,
    };
    serve_tcp(
        wiring.kernel,
        wiring.session,
        wiring.permission_module,
        wiring.question_module,
        options,
        wiring.mcp_manager,
    )
    .await
}

/// No subcommand: print the hint and shut the freshly booted kernel down cleanly.
async fn run_without_command(wiring: Wiring) -> anyhow::Result<()> {
    println!("Use `agentj run \"your prompt\"` or `agentj serve`");
    let ipc_server = build_ipc_server(
        wiring.kernel,
        wiring.session,
        wiring.permission_module,
        wiring.question_module,
        Some(wiring.trajectory_store),
        wiring.provider_service,
    );
    ipc_server.signal_shutdown()?;
    wiring.mcp_manager.close_all().await;
    Ok(())
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
    fn permission_timeout_is_bounded() {
        let mut cfg = agentj_permission::PermissionConfig::default();
        assert_eq!(permission_timeout(&cfg), std::time::Duration::from_secs(60));
        cfg.timeout_ms = Some(800);
        assert_eq!(
            permission_timeout(&cfg),
            std::time::Duration::from_millis(800)
        );
        cfg.timeout_ms = Some(0);
        assert_eq!(
            permission_timeout(&cfg),
            std::time::Duration::from_millis(100)
        );
        cfg.timeout_ms = Some(700_000);
        assert_eq!(
            permission_timeout(&cfg),
            std::time::Duration::from_millis(600_000)
        );
        // A negative value is out of range, not a parse error: it clamps up to the
        // minimum, and the warning echoes the raw value the user wrote (not `0`).
        cfg.timeout_ms = Some(-7);
        assert_eq!(
            permission_timeout(&cfg),
            std::time::Duration::from_millis(100)
        );
        assert_eq!(
            clamp_permission_timeout(Some(-7)).1.as_deref(),
            Some("warning: permission.timeout_ms -7 clamped to 100")
        );
    }

    #[test]
    fn only_out_of_range_permission_timeouts_warn() {
        // In-range and absent values must stay silent — the warning must not become
        // unconditional while fixing the negative case.
        assert_eq!(clamp_permission_timeout(None).1, None);
        assert_eq!(clamp_permission_timeout(Some(800)).1, None);
        assert_eq!(clamp_permission_timeout(Some(100)).1, None);
        assert_eq!(clamp_permission_timeout(Some(600_000)).1, None);
        assert_eq!(
            clamp_permission_timeout(Some(0)).1.as_deref(),
            Some("warning: permission.timeout_ms 0 clamped to 100")
        );
        assert_eq!(
            clamp_permission_timeout(Some(700_000)).1.as_deref(),
            Some("warning: permission.timeout_ms 700000 clamped to 600000")
        );
    }

    /// A negative `[permission] timeout_ms` is an out-of-range value, so it must be
    /// clamped up to the 100 ms minimum like `0` is — never rejected as a parse error.
    #[test]
    fn a_negative_permission_timeout_loads_and_clamps_to_the_minimum() {
        let cfg = load_test_config("[permission]\ntimeout_ms = -7\n");
        let policy = cfg.inner().permission.clone().unwrap_or_default();
        assert_eq!(
            permission_timeout(&policy),
            std::time::Duration::from_millis(100)
        );
    }

    #[test]
    fn stdin_decision_maps_yn_and_eof() {
        assert_eq!(
            decision_from_input(Some("y\n".into())),
            agentj_permission::ApprovalDecision::Allow
        );
        assert_eq!(
            decision_from_input(Some("yes\n".into())),
            agentj_permission::ApprovalDecision::Allow
        );
        assert_eq!(
            decision_from_input(Some("n\n".into())),
            agentj_permission::ApprovalDecision::Deny
        );
        assert_eq!(
            decision_from_input(Some("  \n".into())),
            agentj_permission::ApprovalDecision::Deny
        );
        assert_eq!(
            decision_from_input(None),
            agentj_permission::ApprovalDecision::Deny
        );
    }

    #[test]
    fn approval_prompt_renders_tool_args_and_reason() {
        let prompt = approval_prompt(
            "bash",
            &serde_json::json!({"command": "curl http://example.com"}),
            "tool default mode",
        );
        assert_eq!(
            prompt,
            "approve bash? curl http://example.com\n  reason: tool default mode\n[y/N] "
        );
    }

    #[test]
    fn approval_prompt_shows_the_pattern_for_a_search_tool() {
        let prompt = approval_prompt(
            "glob",
            &serde_json::json!({"pattern": "**/*.rs", "include": "*.toml"}),
            "matched rule: ask",
        );
        // The high-signal `pattern` is shown as a bare string, not a JSON dump.
        assert_eq!(
            prompt,
            "approve glob? **/*.rs\n  reason: matched rule: ask\n[y/N] "
        );
    }

    #[test]
    fn approval_prompt_compacts_long_and_multiline_arguments() {
        let long = format!("echo {} && rm -rf /tmp/x", "a ".repeat(300));
        let prompt = approval_prompt(
            "bash",
            &serde_json::json!({"command": long}),
            "tool default mode",
        );
        let first = prompt.lines().next().unwrap();
        assert!(first.starts_with("approve bash? echo a a a"), "{first}");
        assert!(
            first.ends_with('…'),
            "the long command must be cut: {first}"
        );
        // Whitespace collapses so the prompt stays on one line; no raw JSON braces.
        assert!(!first.contains('{') && !first.contains('\n'), "{first}");
        assert!(
            first.chars().count() <= "approve bash? ".chars().count() + 201,
            "{first}"
        );
        let multi = approval_prompt(
            "bash",
            &serde_json::json!({"command": "printf 'a  b\nc'"}),
            "tool default mode",
        );
        assert_eq!(
            multi,
            "approve bash? printf 'a b c'\n  reason: tool default mode\n[y/N] "
        );
    }

    #[test]
    fn compact_args_falls_back_to_json_for_an_unrecognised_shape() {
        assert_eq!(
            compact_args(&serde_json::json!({"weird": 1})),
            "{\"weird\":1}"
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
        agentj_core::sync::lock_or_recover(ENV_LOCK.get_or_init(|| std::sync::Mutex::new(())))
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
        let c = agentj_core::UsageCollector::new();
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
        let c = agentj_core::UsageCollector::new();
        record_completed_turn(&c, "s1".into(), Some(TokenUsage::default()));
        let t = c.get_session_totals("s1").unwrap();
        assert_eq!((t.prompt_tokens, t.completion_tokens, t.turns), (0, 0, 1));
    }

    #[test]
    fn record_completed_turn_with_none_falls_back_to_turn() {
        let c = agentj_core::UsageCollector::new();
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
                yes,
            }) => {
                assert_eq!(prompt, "hi");
                assert!(attach.is_none());
                assert_eq!(provider.as_deref(), Some("opencode-go"));
                assert_eq!(model.as_deref(), Some("m1"));
                assert!(!yes);
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

    // ---- T5: attach frame decoding ----

    fn frame(v: serde_json::Value) -> AttachAction {
        decode_attach_frame(&v)
    }

    #[test]
    fn attach_deny_replies_are_told_apart_from_run_failures() {
        let refused: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32001,"message":"already settled"}}"#,
        )
        .unwrap();
        assert!(is_attach_deny_reply(&refused, &[3]));
        assert!(
            !is_attach_deny_reply(&refused, &[4]),
            "a foreign id is a run failure"
        );
        assert_eq!(
            frame(refused),
            AttachAction::Fail("error: already settled (code -32001)".into()),
            "the frame still decodes as an error; only the reader decides what to do"
        );
    }

    #[tokio::test]
    async fn attach_survives_a_refused_deny_and_returns_the_run_reply() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (cr, cw) = tokio::io::split(client_side);
        let (mut sr, mut sw) = tokio::io::split(server_side);
        let mut reader = tokio::io::BufReader::new(cr);
        let mut writer = cw;
        let frames = [
            r#"{"jsonrpc":"2.0","method":"$/permissionRequest","params":{"request_id":"r1","tool":"bash","args":{}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32001,"message":"already settled"}}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":"run finished"}}"#,
        ];
        let feeder = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            for f in frames {
                sw.write_all(format!("{f}\n").as_bytes()).await.unwrap();
            }
        });
        let reply = read_attach_stream(&mut reader, &mut writer)
            .await
            .expect("a refused deny must not abort the attached run");
        // `Ok(None)` means the run's answer arrived (it was printed); `Some(_)` would be a
        // failure line, which is what a refused deny used to be turned into.
        assert_eq!(reply, None, "the run's reply must end the stream cleanly");
        feeder.await.unwrap();
        // The deny really went out (the reader wrote it into the half this end reads from).
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 256];
        let n = sr.read(&mut buf).await.expect("the deny must be written");
        let sent = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(sent.contains("\"decision\":\"deny\""), "wrote: {sent}");
    }

    #[test]
    fn attach_frame_denies_a_permission_request() {
        let v = serde_json::json!({"jsonrpc":"2.0","method":"$/permissionRequest",
            "params":{"request_id":"r1","tool":"bash","args":{"cmd":"ls"},"reason":"ask"}});
        assert_eq!(
            frame(v),
            AttachAction::DenyApproval {
                request_id: "r1".into(),
                tool: "bash".into()
            }
        );
    }

    #[test]
    fn attach_frame_ignores_progress_and_respond_replies() {
        for v in [
            serde_json::json!({"jsonrpc":"2.0","method":"$/progress","params":{"token":"t"}}),
            serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}}),
        ] {
            assert_eq!(frame(v), AttachAction::Ignore);
        }
    }

    #[test]
    fn attach_frame_surfaces_errors_and_content() {
        assert!(matches!(
            frame(
                serde_json::json!({"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"boom"}})
            ),
            AttachAction::Fail(_)
        ));
        assert_eq!(
            frame(serde_json::json!({"jsonrpc":"2.0","id":2,"result":{"content":"done"}})),
            AttachAction::Reply("done".into())
        );
    }

    #[test]
    fn attach_deny_warning_names_the_tool_and_request_id() {
        let line = attach_deny_warning("bash", "r1");
        assert!(line.starts_with("warning: run --attach cannot answer approvals"));
        assert!(line.contains("bash") && line.contains("r1"));
    }

    #[test]
    fn attach_settles_a_question_as_skipped() {
        let v = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/askQuestion",
            "params": { "question_id": "q-7", "question": "Which file?", "options": [] }
        });
        assert_eq!(
            frame(v),
            AttachAction::SkipQuestion {
                question_id: "q-7".into()
            }
        );
        let body = attach_skip_question_request("q-7", 9);
        let sent: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(sent["method"], "question/respond");
        assert_eq!(sent["params"]["question_id"], "q-7");
        assert_eq!(sent["params"]["skipped"], true);
        assert_eq!(sent["id"], 9);
        assert!(
            attach_skip_question_warning("q-7").contains("cannot answer questions"),
            "the operator must be told the question was skipped"
        );
    }

    #[test]
    fn attach_ignores_a_malformed_question_notification() {
        let v = serde_json::json!({"jsonrpc": "2.0", "method": "$/askQuestion", "params": {}});
        assert_eq!(frame(v), AttachAction::Ignore);
    }

    #[test]
    fn attach_deny_request_serializes_to_a_deny_decision() {
        let body = attach_deny_request("req-9", 7);
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["method"], "permission/respond");
        assert_eq!(v["params"]["request_id"], "req-9");
        assert_eq!(v["params"]["decision"], "deny");
        assert_eq!(v["id"], 7);
    }

    #[test]
    fn attach_frame_ignores_a_malformed_permission_notification() {
        let v = serde_json::json!({"jsonrpc":"2.0","method":"$/permissionRequest","params":{}});
        assert_eq!(
            frame(v),
            AttachAction::Ignore,
            "no request_id means nothing to deny"
        );
    }

    #[test]
    fn run_error_line_is_none_for_result() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":2,"result":{"content":"x"}}"#).unwrap();
        assert!(run_error_line(&v).is_none());
    }

    /// `agentj_config::Config` has no `Default` impl, so every case goes through the
    /// real loader: a temp file for an explicit `[web]` section, `load(None)` otherwise.
    fn config_with_web(state_dir: Option<&str>) -> agentj_config::Config {
        match state_dir {
            Some(dir) => {
                let path = std::env::temp_dir().join(format!(
                    "agentj-web-test-{}-{:?}.toml",
                    std::process::id(),
                    std::thread::current().id()
                ));
                std::fs::write(&path, format!("[web]\nstate_dir = \"{dir}\"\n")).unwrap();
                let loaded = agentj_config::Config::load(Some(path.clone())).unwrap();
                std::fs::remove_file(&path).ok();
                loaded
            }
            None => agentj_config::Config::load(None).unwrap(),
        }
    }

    /// Mirrors `config_with_web`: a temp file holding the given `[web]` body, loaded
    /// through the real loader.
    fn config_with_web_body(body: &str) -> agentj_config::Config {
        let path = std::env::temp_dir().join(format!(
            "agentj-web-integrity-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, body).unwrap();
        let loaded = agentj_config::Config::load(Some(path.clone())).unwrap();
        std::fs::remove_file(&path).ok();
        loaded
    }

    #[test]
    fn resolve_integrity_options_reads_the_web_section() {
        let cfg = config_with_web_body(
            "[web]\nspot_interval_secs = 45\nspot_sample = 3\n\
             verify_on_request = false\nverify_cache_ttl_secs = 9\n",
        );
        let opts = resolve_integrity_options(&cfg);
        assert_eq!(opts.spot_interval, std::time::Duration::from_secs(45));
        assert_eq!(opts.spot_sample, 3);
        assert!(!opts.verify_on_request);
        assert_eq!(opts.verify_cache_ttl, std::time::Duration::from_secs(9));
    }

    #[test]
    fn resolve_integrity_options_defaults_without_a_web_section() {
        let cfg = config_with_web_body("[permission]\ntimeout_ms = 800\n");
        assert_eq!(
            resolve_integrity_options(&cfg),
            agentj_ipc::web::IntegrityOptions::default()
        );
    }

    #[test]
    fn resolve_integrity_options_clamps_a_zero_spot_interval() {
        let cfg = config_with_web_body("[web]\nspot_interval_secs = 0\n");
        assert_eq!(
            resolve_integrity_options(&cfg).spot_interval,
            std::time::Duration::from_secs(5),
            "Duration::ZERO would panic tokio::time::interval"
        );
    }

    #[test]
    fn explicit_state_dir_wins_over_the_environment() {
        let cfg = config_with_web(Some("/var/lib/agentj"));
        assert_eq!(
            metrics_state_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/var/lib/agentj"))
        );
    }

    #[test]
    fn state_dir_empty_string_is_not_a_path() {
        let cfg = config_with_web(Some(""));
        assert_eq!(
            metrics_state_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/xdg/agentj")),
            "an empty configured state_dir must behave like an absent one"
        );
        assert_eq!(
            usage_history_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/xdg/agentj/usage"))
        );
    }

    #[test]
    fn state_dir_falls_back_to_xdg_then_home() {
        let cfg = config_with_web(None);
        assert_eq!(
            metrics_state_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/xdg/agentj"))
        );
        assert_eq!(
            metrics_state_dir_from(&cfg, None, Some("/home/u")),
            Some(std::path::PathBuf::from("/home/u/.local/share/agentj"))
        );
        assert_eq!(
            metrics_state_dir_from(&cfg, Some(""), Some("/home/u")),
            Some(std::path::PathBuf::from("/home/u/.local/share/agentj")),
            "an empty XDG_DATA_HOME is not a path"
        );
    }

    #[test]
    fn state_dir_empty_string_without_env_warns() {
        let cfg = config_with_web(Some(""));
        assert_eq!(metrics_state_dir_from(&cfg, None, None), None);
        assert!(metrics_state_warning(None).is_some());
    }

    #[test]
    fn state_dir_is_none_without_any_env_input() {
        let cfg = config_with_web(None);
        assert_eq!(metrics_state_dir_from(&cfg, None, None), None);
        assert!(metrics_state_warning(None).is_some());
        assert!(metrics_state_warning(Some(std::path::Path::new("/tmp/x"))).is_none());
    }

    #[test]
    fn usage_history_dir_prefers_state_dir_then_xdg_then_home() {
        let cfg = config_with_web(Some("/tmp/aj"));
        assert_eq!(
            usage_history_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/tmp/aj/usage")),
            "[web] state_dir wins, and the store appends its own subdirectory"
        );
        let cfg = config_with_web(None);
        assert_eq!(
            usage_history_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            Some(std::path::PathBuf::from("/xdg/agentj/usage"))
        );
        assert_eq!(
            usage_history_dir_from(&cfg, None, Some("/home/u")),
            Some(std::path::PathBuf::from(
                "/home/u/.local/share/agentj/usage"
            ))
        );
        assert_eq!(usage_history_dir_from(&cfg, None, None), None);
    }

    #[test]
    fn web_settings_carry_the_config_clamp_warnings() {
        let cfg = load_test_config("[agent]\nmax_tool_iterations = 9000\n");
        let web = WebSettings::resolve(&cfg);
        assert_eq!(
            web.config_warnings,
            vec!["warning: agent.max_tool_iterations = 9000 outside 1..=1000; using 1000"],
            "the serve banner must surface a silently clamped knob"
        );
        assert!(
            WebSettings::resolve(&load_test_config(""))
                .config_warnings
                .is_empty(),
            "an in-range config stays quiet"
        );
    }

    #[test]
    fn usage_history_dir_is_none_when_history_is_disabled() {
        let cfg = config_with_web_body("[usage]\nhistory_enabled = false\n");
        assert_eq!(
            usage_history_dir_from(&cfg, Some("/xdg"), Some("/home/u")),
            None,
            "a disabled store must not get a directory at all"
        );
    }

    #[test]
    fn usage_history_warning_names_every_fallback() {
        let cfg = config_with_web(None);
        let warn = usage_history_warning(&cfg, None).expect("warning expected");
        assert!(warn.contains("XDG_DATA_HOME"), "{warn}");
        assert!(warn.contains("HOME"), "{warn}");
        assert!(warn.contains("state_dir"), "{warn}");
        assert!(warn.contains("usage history is disabled"), "{warn}");
        assert!(usage_history_warning(&cfg, Some(Path::new("/tmp/x"))).is_none());
        let disabled = config_with_web_body("[usage]\nhistory_enabled = false\n");
        assert!(
            usage_history_warning(&disabled, None).is_none(),
            "a disabled store must not warn"
        );
    }

    #[test]
    fn usage_history_limits_fall_back_to_the_store_defaults() {
        let cfg = config_with_web_body("[permission]\ntimeout_ms = 800\n");
        assert_eq!(
            usage_history_limits(&cfg),
            (
                usage_history::DEFAULT_RETENTION_DAYS,
                usage_history::DEFAULT_MAX_SAMPLES
            )
        );
        let cfg = config_with_web_body("[usage]\nhistory_days = 7\nhistory_max_samples = 500\n");
        assert_eq!(usage_history_limits(&cfg), (7, 500));
    }

    #[test]
    fn metrics_state_warning_explains_the_in_memory_fallback() {
        let warning = metrics_state_warning(None).expect("warning expected");
        assert!(warning.contains("XDG_DATA_HOME"), "{warning}");
        assert!(warning.contains("in memory"), "{warning}");
    }

    #[test]
    fn proxy_trust_warning_only_for_non_loopback_hosts() {
        assert!(proxy_trust_warning("127.0.0.1", true).is_none());
        assert!(proxy_trust_warning("localhost", true).is_none());
        assert!(proxy_trust_warning("0.0.0.0", false).is_none());
        let warning = proxy_trust_warning("0.0.0.0", true).expect("warning expected");
        assert!(warning.contains("trust_proxy_headers"), "{warning}");
    }

    /// The graceful-shutdown contract `serve_tcp` relies on: `flush_metrics_state()`
    /// (persist) runs before `stop_metrics_flush()` (abort the debounce task), and the
    /// snapshot outlives the abort. Uses the same public seam the CLI calls, so the
    /// state path is the real `<dir>/metrics-state-<bound port>.json`.
    #[tokio::test]
    async fn graceful_shutdown_persists_metrics_state_before_aborting_the_task() {
        let dir = tempfile::tempdir().unwrap();
        let tcp = agentj_ipc::tcp::TcpIpcServer::bind(
            "127.0.0.1:0",
            agentj_core::kernel::Kernel::new(),
            std::sync::Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            agentj_ipc::tcp::TcpBindOptions {
                metrics_state_dir: Some(dir.path().to_path_buf()),
                ..Default::default()
            },
        )
        .await
        .expect("bind");
        let port = tcp.local_addr().expect("local addr").port();
        let path = dir.path().join(format!("metrics-state-{port}.json"));
        assert!(
            !path.exists(),
            "the debounce task must not write on its own"
        );
        // Exactly what `serve_tcp` does on shutdown, in order.
        tcp.web_config().flush_metrics_state();
        tcp.web_config().stop_metrics_flush();
        assert!(path.is_file(), "flush must persist {}", path.display());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"version\":1"), "{body}");
        assert!(body.contains("\"counters\""), "{body}");
    }

    /// A bound TCP server whose metrics state persists under `dir`: the same
    /// `TcpIpcServer::bind` seam `serve_tcp` uses, so the real state path is
    /// `<dir>/metrics-state-<bound port>.json`.
    async fn tcp_server_with_metrics_dir(dir: &std::path::Path) -> agentj_ipc::tcp::TcpIpcServer {
        agentj_ipc::tcp::TcpIpcServer::bind(
            "127.0.0.1:0",
            Kernel::new(),
            Arc::new(agentj_session::InMemoryStore::new()),
            None,
            None,
            agentj_ipc::tcp::TcpBindOptions {
                metrics_state_dir: Some(dir.to_path_buf()),
                ..Default::default()
            },
        )
        .await
        .expect("bind")
    }

    /// An idle MCP manager for the teardown path: nothing is connected, so
    /// `close_all` is a no-op and the manager needs no real tool registry.
    fn test_mcp_manager() -> McpConnectionManager {
        McpConnectionManager::new(
            Arc::new(ToolRegistry::new()),
            Duration::from_secs(60),
            Duration::from_millis(500),
            5,
            Arc::new(agentj_core::hooks::EventBus::new()),
        )
    }

    /// Serve-path regression guard: a notifier is attached (never answers), and
    /// the configured `[agent] question_timeout_s` default must be applied so an
    /// `ask` that omits `timeout_s` settles as `Skipped` instead of hanging.
    #[test]
    fn served_path_question_omitting_timeout_settles_skipped() {
        // 1s default keeps the wait short and proves the injection, not the
        // value; the serve path attaches a notifier that ACKNOWLEDGES delivery
        // but never answers, so only the configured default can settle it.
        let cfg = load_test_config("[agent]\nquestion_timeout_s = 1\n");
        let module = QuestionModule::new();
        // THE SAME injection helper the construction site calls (design D4) —
        // the test cannot drift from production's single injection point.
        apply_question_timeout(&module, &cfg);
        module.set_notifier(Arc::new(|_params, _session| true));

        let module = Arc::new(module);
        let (tx, rx) = std::sync::mpsc::channel();
        let asker = {
            let module = Arc::clone(&module);
            std::thread::spawn(move || {
                let params = agentj_protocol::question::AskQuestionParams {
                    question_id: String::new(),
                    session_id: "s1".into(),
                    question: "Pick a color".into(),
                    header: None,
                    options: vec![agentj_protocol::question::AskOption {
                        id: "red".into(),
                        label: "Red".into(),
                        hint: None,
                    }],
                    allow_multiple: false,
                    allow_other: true,
                    timeout_s: None, // omitted -> must fall back to the configured default
                };
                let (_, outcome) = module.ask_blocking("s1", &params);
                let _ = tx.send(outcome);
            })
        };

        let outcome = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| {
            panic!(
                "question did not settle within 5s: the module default timeout was not applied, \
                 so an `ask` omitting `timeout_s` hangs forever on the serve path"
            )
        });
        assert_eq!(outcome, agentj_question::QuestionOutcome::Skipped);
        asker.join().unwrap();
    }

    /// `shutdown_tcp_server` must run the full teardown (persist metrics, abort
    /// the debounce task) even when the serve loop returned `Err`, then still
    /// surface the error so the CLI exits non-zero. Regression guard: the old
    /// `tcp.run().await?` skipped the flush on the error path.
    #[tokio::test]
    async fn shutdown_flushes_metrics_state_even_when_the_serve_loop_failed() {
        let dir = tempfile::tempdir().unwrap();
        let tcp = tcp_server_with_metrics_dir(dir.path()).await;
        let port = tcp.local_addr().expect("local addr").port();
        let path = dir.path().join(format!("metrics-state-{port}.json"));
        let signal_task = tokio::spawn(std::future::pending::<()>());
        let result = shutdown_tcp_server(
            &tcp,
            signal_task,
            &test_mcp_manager(),
            Err(agentj_core::error::IpcError::InvalidRequest("boom".into())),
        )
        .await;
        assert!(
            result.is_err(),
            "a serve-loop error must still propagate after the teardown"
        );
        assert!(
            path.is_file(),
            "flush must persist {} on the error path",
            path.display()
        );
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"version\":1"), "{body}");
        assert!(body.contains("\"counters\""), "{body}");
    }
}
