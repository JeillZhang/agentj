//! agentj-cli — the `agentj` binary: run a single prompt, or serve the JSON-RPC API
//! over stdio. Production wiring: AgentLoop registers into the Kernel as a module so
//! init/shutdown run in the real process; stdin EOF and signals share the same
//! graceful-shutdown chain (design #13).

use std::sync::Arc;

use clap::{Parser, Subcommand};

use agentj_context::ContextPipeline;
use agentj_core::agent_loop::AgentLoop;
use agentj_core::error::ModuleError;
use agentj_core::kernel::Kernel;
use agentj_core::module::{
    KernelContext, KernelMessage, KernelModule, KernelResponse, Module, ModuleMetadata,
    ModuleRegister, ProviderRegistry, ToolRegistry,
};
use agentj_ipc::IpcServer;
use agentj_model::factory::ProviderFactory;
use agentj_session::InMemoryStore;
use agentj_tools::builtins::register_builtins;

/// Wraps the AgentLoop as a kernel Module so init/shutdown run in the production process.
struct AgentLoopModule {
    // Held so the kernel owns the loop's lifetime; orchestration stays direct.
    #[allow(dead_code)]
    agent_loop: AgentLoop,
}

impl Module for AgentLoopModule {
    fn id(&self) -> agentj_core::types::ModuleId {
        "agent_loop".into()
    }
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "agent_loop".into(),
            name: "AgentLoop".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Agent orchestration loop".into(),
            author: "AgentJ".into(),
        }
    }
    fn init(
        &mut self,
        _ctx: &KernelContext,
        _register: &mut ModuleRegister,
    ) -> Result<(), ModuleError> {
        tracing::info!("agent_loop module initialized");
        Ok(())
    }
    fn shutdown(&mut self) -> Result<(), ModuleError> {
        tracing::info!("agent_loop module shutting down");
        Ok(())
    }
}

impl KernelModule for AgentLoopModule {
    fn handle(&mut self, _msg: &KernelMessage) -> Result<KernelResponse, ModuleError> {
        // Orchestration stays direct (architecture note): the kernel owns lifecycle only.
        Ok(KernelResponse::Handled)
    }
}

#[derive(Parser)]
#[command(name = "agentj", version, about = "AgentJ — AI coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single prompt and print the final response.
    Run { prompt: String },
    /// Serve the JSON-RPC API over stdio.
    Serve,
}

/// Build the per-tool policy map from the [permission] config section
/// (empty map when absent — the engine falls back to factory defaults).
fn permission_policies_from_config(
    cfg: &agentj_config::Config,
) -> std::collections::HashMap<String, agentj_permission::ToolPolicy> {
    cfg.inner().permission.clone().unwrap_or_default().tools
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let config =
        agentj_config::Config::load(Some(cli.config.into())).map_err(|e| anyhow::anyhow!("{e}"))?;
    let tools = Arc::new(ToolRegistry::new());
    register_builtins(&tools);
    let providers = Arc::new(ProviderRegistry::new());
    let session = Arc::new(InMemoryStore::new());

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
    permission_cfg.validate().expect("invalid [permission] config");
    let permission_policies = permission_policies_from_config(&config);

    let ctx = KernelContext {
        session,
        tools,
        providers,
        config: Arc::new(config),
    };

    // Lifecycle wiring: AgentLoop registers into the Kernel, so init/shutdown run in
    // the real process. session/run still calls AgentLoop directly (orchestration stays
    // direct per architecture note); the Kernel owns module lifecycle + graceful shutdown.
    let mut kernel = Kernel::new();
    let mut register = ModuleRegister::new(Arc::clone(&ctx.tools), Arc::clone(&ctx.providers));
    let agent_loop = AgentLoop::new(
        ctx.clone(),
        Arc::new(ContextPipeline::new()),
        "You are AgentJ, an AI coding assistant.".into(),
    );
    kernel.register_module(
        Box::new(AgentLoopModule {
            agent_loop: agent_loop.clone(),
        }),
        &ctx,
        &mut register,
    )?;

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

    let mut ipc_server = IpcServer::new(agent_loop, kernel);
    ipc_server.permission_module = Some(permission_module);

    // design #13: SIGINT/SIGTERM → same graceful shutdown chain as stdin EOF.
    // The sender is cloned so the signal task doesn't borrow ipc_server; serve_stdio
    // exits when the kernel finishes (read-side interruption in T13).
    let sender = ipc_server.kernel_sender();
    let signal_task = tokio::spawn(async move {
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
    });

    match cli.command {
        Some(Commands::Run { prompt }) => {
            let sid = ipc_server.create_session()?;
            let response = ipc_server.run_turn(&sid, &prompt).await?;
            println!("{response}");
            // graceful shutdown: signal the kernel and wait for modules' shutdown()
            ipc_server.signal_shutdown()?;
            ipc_server.await_shutdown().await?;
        }
        Some(Commands::Serve) => {
            // serve_stdio exits on EOF, shutdown request, or signal-triggered kernel
            // completion; kernel graceful shutdown runs after
            ipc_server.serve_stdio().await?;
        }
        None => {
            println!("Use `agentj run \"your prompt\"` or `agentj serve`");
            ipc_server.signal_shutdown()?;
            ipc_server.await_shutdown().await?;
        }
    }
    signal_task.abort();
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
            cfg.inner().permission.clone().unwrap_or_default().validate().is_err()
        );
    }
}
