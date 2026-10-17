pub mod merge;
pub mod write;
use agentj_core::module::ConfigProvider;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
pub use write::{set_model_defaults, write_model_defaults};

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    /// Wire-format discriminator (design 4.3): "openai-compat" | "responses" |
    /// "anthropic" | "opencode-go". Default "openai-compat" keeps legacy configs working.
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "openai-compat".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSection {
    pub provider: String,
    #[serde(default)]
    pub providers: Vec<ModelProviderConfig>,
    /// Default model id for AgentLoop (design 4.3). Injected into ModelOptions via
    /// ConfigProvider::get_str("model.default_model"); default "deepseek-chat".
    #[serde(default = "default_model")]
    pub default_model: String,
    /// Usage warning threshold in percent, clamped to 1..=100 by the accessor.
    /// @since 0.27.0
    #[serde(default = "default_quota_warn_percent")]
    pub quota_warn_percent: i64,
    /// opencode-go Console base URL (device flow + account catalog). @since 0.27.0
    #[serde(default = "default_console_base_url")]
    pub console_base_url: String,
    /// Upper bound (approximate tokens) for an assembled request; the kernel trims the
    /// session history and tool results to fit before calling a provider.
    /// @since 0.29.7
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: i64,
}

fn default_model() -> String {
    "deepseek-chat".into()
}

/// Default usage warning threshold (percent). @since 0.27.0
pub fn default_quota_warn_percent() -> i64 {
    80
}

/// Default opencode-go Console base URL. @since 0.27.0
pub fn default_console_base_url() -> String {
    "https://opencode.ai/console".into()
}

/// Default per-request context budget (approximate tokens), kept well below every window
/// in the catalog so a request is almost always accepted. Single source of truth lives in
/// `agentj_core::budget`. @since 0.29.7
pub fn default_max_context_tokens() -> i64 {
    agentj_core::budget::DEFAULT_CONTEXT_TOKENS as i64
}

impl Default for ModelSection {
    fn default() -> Self {
        Self {
            provider: "deepseek".into(),
            providers: vec![],
            default_model: default_model(),
            quota_warn_percent: default_quota_warn_percent(),
            console_base_url: default_console_base_url(),
            max_context_tokens: default_max_context_tokens(),
        }
    }
}

fn default_max_tool_iterations() -> i64 {
    50
}

/// Bounds for the agent loop (`[agent]`). @since 0.29.9
#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    /// Tool-calling rounds a single turn may run before the loop stops offering tools and
    /// asks the model for a conclusion from what it already gathered. Clamped to
    /// `MIN_TOOL_ITERATIONS..=MAX_TOOL_ITERATIONS` by the accessor. @since 0.29.9
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: i64,
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_tool_iterations(),
        }
    }
}

/// Lower bound of `agent.max_tool_iterations` (0 cannot be expressed by the loop, which always
/// runs at least one round). @since 0.29.9
pub const MIN_TOOL_ITERATIONS: i64 = 1;
/// Upper bound of `agent.max_tool_iterations`. @since 0.29.9
pub const MAX_TOOL_ITERATIONS: i64 = 1_000;

/// Clamp a configured tool-iteration budget into the supported range. @since 0.29.9
pub fn clamp_tool_iterations(value: i64) -> i64 {
    value.clamp(MIN_TOOL_ITERATIONS, MAX_TOOL_ITERATIONS)
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SessionConfig {
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Trajectory rolling-trim caps (defaults 10_000 / 5 MiB, see ipc::trajectory).
    #[serde(default)]
    pub trajectory_max_events: Option<u64>,
    #[serde(default)]
    pub trajectory_max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LicenseConfig {
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentJConfig {
    #[serde(default)]
    pub model: ModelSection,
    #[serde(default)]
    pub agent: AgentSection,
    #[serde(default)]
    pub permission: Option<agentj_permission::PermissionConfig>,
    #[serde(default)]
    pub session: Option<SessionConfig>,
    #[serde(default)]
    pub license: Option<LicenseConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
}

impl Default for AgentJConfig {
    fn default() -> Self {
        Self {
            model: ModelSection {
                provider: "deepseek".into(),
                providers: vec![ModelProviderConfig {
                    name: "deepseek-chat".into(),
                    base_url: "https://api.deepseek.com".into(),
                    api_key_env: Some("DEEPSEEK_API_KEY".into()),
                    kind: default_kind(),
                }],
                default_model: default_model(),
                quota_warn_percent: default_quota_warn_percent(),
                console_base_url: default_console_base_url(),
                max_context_tokens: default_max_context_tokens(),
            },
            agent: AgentSection::default(),
            permission: None,
            session: None,
            license: None,
            mcp: None,
        }
    }
}

pub struct Config {
    inner: AgentJConfig,
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = AgentJConfig::default();
        if let Some(p) = path
            && p.exists()
        {
            let content = std::fs::read_to_string(&p)?;
            let file_config: AgentJConfig = toml::from_str(&content)?;
            config = merge::merge_config(config, file_config);
        }
        config = merge::apply_env(config);
        Ok(Self { inner: config })
    }

    pub fn inner(&self) -> &AgentJConfig {
        &self.inner
    }
}

impl ConfigProvider for Config {
    fn get_str(&self, key: &str) -> Option<String> {
        match key {
            "model.provider" => Some(self.inner.model.provider.clone()),
            "model.default_model" => Some(self.inner.model.default_model.clone()),
            "model.console_base_url" => Some(self.inner.model.console_base_url.clone()),
            "session.dir" => self
                .inner
                .session
                .as_ref()
                .and_then(|s| s.dir.clone())
                .map(|p| p.to_string_lossy().to_string()),
            "license.key" => self.inner.license.as_ref().and_then(|l| l.key.clone()),
            _ => None,
        }
    }
    fn get_int(&self, key: &str) -> Option<i64> {
        match key {
            "model.quota_warn_percent" => Some(self.inner.model.quota_warn_percent.clamp(1, 100)),
            "agent.max_tool_iterations" => {
                Some(clamp_tool_iterations(self.inner.agent.max_tool_iterations))
            }
            "model.max_context_tokens" => Some(self.inner.model.max_context_tokens.clamp(
                agentj_core::budget::MIN_CONTEXT_TOKENS as i64,
                agentj_core::budget::MAX_CONTEXT_TOKENS as i64,
            )),
            _ => None,
        }
    }
    fn providers(&self) -> Vec<(String, String)> {
        self.inner
            .model
            .providers
            .iter()
            .map(|p| (p.name.clone(), p.base_url.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn loads_default_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        let providers = config.providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].0, "deepseek-chat");
    }

    #[test]
    fn env_overrides_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "[model]\nprovider = \"deepseek\"\n").unwrap();

        unsafe { std::env::set_var("AGENTJ_MODEL_PROVIDER", "opencode-go") };
        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.provider, "opencode-go");
        unsafe { std::env::remove_var("AGENTJ_MODEL_PROVIDER") };
    }

    #[test]
    fn kind_defaults_to_openai_compat_for_legacy_config() {
        // Legacy config (design 4.3): no `kind` field → "openai-compat" (zero break).
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "deepseek"
        [[model.providers]]
        name = "deepseek-chat"
        base_url = "https://api.deepseek.com"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.inner().model.providers[0].kind, "openai-compat");
        assert_eq!(config.inner().model.default_model, "deepseek-chat");
    }

    #[test]
    fn opencode_go_kind_and_default_model_parse() {
        let dir = tempfile::TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[model]
        provider = "opencode-go"
        default_model = "opencode-go/kimi-k3"
        [[model.providers]]
        name = "opencode-go"
        kind = "opencode-go"
        base_url = "https://opencode.ai/zen/go"
        api_key_env = "OPENCODE_GO_API_KEY"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        let p = &config.inner().model.providers[0];
        assert_eq!(p.kind, "opencode-go");
        assert_eq!(p.base_url, "https://opencode.ai/zen/go");
        assert_eq!(config.inner().model.default_model, "opencode-go/kimi-k3");
        assert_eq!(
            config.get_str("model.default_model").as_deref(),
            Some("opencode-go/kimi-k3")
        );
    }

    #[test]
    fn permission_section_parses() {
        // AgentJConfig is the serde struct (Config is a non-deserializable wrapper).
        let cfg: AgentJConfig = toml::from_str(
            r#"
            [permission.tools.bash]
            mode = "ask"
            [[permission.tools.bash.command_rules]]
            effect = "deny"
            prefix = "rm -rf /*"
        "#,
        )
        .unwrap();
        let pc = cfg.permission.unwrap_or_default();
        let bash = pc.tools.get("bash").unwrap();
        assert_eq!(bash.mode, agentj_permission::PermissionMode::Ask);
        assert_eq!(bash.command_rules.len(), 1);
        assert_eq!(
            bash.command_rules[0].effect,
            agentj_permission::PermissionEffect::Deny
        );
        assert_eq!(bash.command_rules[0].prefix, "rm -rf /*");
    }

    #[test]
    fn permission_section_defaults_when_absent() {
        let cfg: AgentJConfig = toml::from_str("").unwrap();
        assert!(cfg.permission.is_none());
        // Factory defaults kick in at the engine level, not config level:
        assert_eq!(
            agentj_permission::builtin_default_mode("read"),
            agentj_permission::PermissionMode::Allow
        );
        assert_eq!(
            agentj_permission::builtin_default_mode("bash"),
            agentj_permission::PermissionMode::Ask
        );
    }

    /// The agent-loop budget is configurable now: it used to be a hardcoded 50 the config
    /// layer could not answer at all, so a long task died at round 50 with an internal error
    /// the user never saw. @since 0.29.9
    /// Parse `text` as a config file through the same path production uses.
    fn parse_config(text: &str) -> Config {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        Config::load(Some(path)).unwrap()
    }

    #[test]
    fn agent_section_sets_the_tool_iteration_budget() {
        let cfg = parse_config("[agent]\nmax_tool_iterations = 120\n");
        assert_eq!(cfg.get_int("agent.max_tool_iterations"), Some(120));
        assert_eq!(cfg.inner().agent.max_tool_iterations, 120);
    }

    #[test]
    fn agent_section_defaults_and_clamps() {
        let cfg = parse_config("[agent]\n");
        assert_eq!(cfg.get_int("agent.max_tool_iterations"), Some(50));
        let low = parse_config("[agent]\nmax_tool_iterations = 0\n");
        assert_eq!(
            low.get_int("agent.max_tool_iterations"),
            Some(MIN_TOOL_ITERATIONS)
        );
        let high = parse_config("[agent]\nmax_tool_iterations = 99999\n");
        assert_eq!(
            high.get_int("agent.max_tool_iterations"),
            Some(MAX_TOOL_ITERATIONS)
        );
    }

    #[test]
    fn permission_rule_invalid_glob_fails_fast() {
        // "[**" is an unterminated glob — globset rejects it.
        let cfg: AgentJConfig = toml::from_str(
            r#"
            [permission.tools.write]
            mode = "ask"
            [[permission.tools.write.path_rules]]
            effect = "allow"
            pattern = "[**"
        "#,
        )
        .unwrap();
        let pc = cfg.permission.unwrap_or_default();
        let err = pc.validate().unwrap_err();
        assert!(matches!(
            err,
            agentj_permission::PermissionError::InvalidRule { .. }
        ));
    }

    #[test]
    fn permission_reachable_through_config_wrapper() {
        // The permission field lives on AgentJConfig; Config::load/merge preserves it.
        let dir = std::env::temp_dir().join(format!("agentj-cfg-perm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.toml");
        std::fs::write(&path, "[permission.tools.bash]\nmode = \"ask\"\n").unwrap();
        let config = Config::load(Some(path)).unwrap();
        assert!(config.inner().permission.is_some());
        assert!(
            config
                .inner()
                .permission
                .as_ref()
                .unwrap()
                .tools
                .contains_key("bash")
        );
    }

    #[test]
    fn session_dir_section_parses() {
        let cfg: AgentJConfig = toml::from_str("[session]\ndir = \"/tmp/sess\"\n").unwrap();
        assert_eq!(
            cfg.session.as_ref().unwrap().dir.as_deref(),
            Some(std::path::Path::new("/tmp/sess"))
        );
    }

    #[test]
    fn session_section_absent_is_none() {
        let cfg: AgentJConfig = toml::from_str("").unwrap();
        assert!(cfg.session.is_none());
    }

    #[test]
    fn session_dir_merges_from_file() {
        let base = AgentJConfig::default();
        let file = AgentJConfig {
            session: Some(SessionConfig {
                dir: Some("/tmp/x".into()),
                ..Default::default()
            }),
            ..AgentJConfig::default()
        };
        let merged = crate::merge::merge_config(base, file);
        assert_eq!(
            merged.session.as_ref().unwrap().dir.as_deref(),
            Some(std::path::Path::new("/tmp/x"))
        );
    }

    #[test]
    fn license_section_parses() {
        let cfg: AgentJConfig = toml::from_str("[license]\nkey = \"abc.def\"\n").unwrap();
        assert_eq!(
            cfg.license.as_ref().unwrap().key.as_deref(),
            Some("abc.def")
        );
    }

    #[test]
    fn license_section_absent_is_none() {
        let cfg: AgentJConfig = toml::from_str("").unwrap();
        assert!(cfg.license.is_none());
    }

    #[test]
    fn license_key_env_overrides() {
        let _g = env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "[license]\nkey = \"abc.def\"\n").unwrap();
        unsafe { std::env::set_var("AGENTJ_LICENSE_KEY", "xyz") };
        let config = Config::load(Some(p)).unwrap();
        unsafe { std::env::remove_var("AGENTJ_LICENSE_KEY") };
        assert_eq!(
            config.inner().license.as_ref().unwrap().key.as_deref(),
            Some("xyz")
        );
    }

    #[test]
    fn empty_license_key_env_keeps_config_key() {
        let _g = env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "[license]\nkey = \"abc.def\"\n").unwrap();
        unsafe { std::env::set_var("AGENTJ_LICENSE_KEY", "") };
        let config = Config::load(Some(p)).unwrap();
        unsafe { std::env::remove_var("AGENTJ_LICENSE_KEY") };
        assert_eq!(
            config.inner().license.as_ref().unwrap().key.as_deref(),
            Some("abc.def")
        );
    }

    #[test]
    fn mcp_section_parses() {
        let cfg: AgentJConfig = toml::from_str(
            r#"
        [[mcp.servers]]
        name = "fs"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-filesystem"]
        env = { FOO = "bar" }
        "#,
        )
        .unwrap();
        let mcp = cfg.mcp.unwrap();
        assert_eq!(mcp.servers.len(), 1);
        let s = &mcp.servers[0];
        assert_eq!(s.name, "fs");
        assert_eq!(s.command, "npx");
        assert_eq!(
            s.args,
            vec!["-y", "@modelcontextprotocol/server-filesystem"]
        );
        assert_eq!(s.env.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    fn mcp_server_defaults_args_and_env() {
        let cfg: AgentJConfig = toml::from_str(
            r#"
        [[mcp.servers]]
        name = "fs"
        command = "npx"
        "#,
        )
        .unwrap();
        let s = &cfg.mcp.unwrap().servers[0];
        assert!(s.args.is_empty());
        assert!(s.env.is_empty());
    }

    #[test]
    fn mcp_section_absent_is_none() {
        let cfg: AgentJConfig = toml::from_str("").unwrap();
        assert!(cfg.mcp.is_none());
    }

    #[test]
    fn quota_warn_percent_defaults_and_clamps() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[model]\nprovider = \"deepseek\"\n").unwrap();
        assert_eq!(
            Config::load(Some(p.clone()))
                .unwrap()
                .get_int("model.quota_warn_percent"),
            Some(80)
        );
        std::fs::write(
            &p,
            "[model]\nprovider = \"deepseek\"\nquota_warn_percent = 0\n",
        )
        .unwrap();
        assert_eq!(
            Config::load(Some(p.clone()))
                .unwrap()
                .get_int("model.quota_warn_percent"),
            Some(1)
        );
        std::fs::write(
            &p,
            "[model]\nprovider = \"deepseek\"\nquota_warn_percent = 250\n",
        )
        .unwrap();
        assert_eq!(
            Config::load(Some(p))
                .unwrap()
                .get_int("model.quota_warn_percent"),
            Some(100)
        );
    }

    #[test]
    fn max_context_tokens_defaults_overrides_and_clamps() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[model]\nprovider = \"deepseek\"\n").unwrap();
        assert_eq!(
            Config::load(Some(p.clone()))
                .unwrap()
                .get_int("model.max_context_tokens"),
            Some(128_000)
        );
        std::fs::write(
            &p,
            "[model]\nprovider = \"deepseek\"\nmax_context_tokens = 32000\n",
        )
        .unwrap();
        assert_eq!(
            Config::load(Some(p.clone()))
                .unwrap()
                .get_int("model.max_context_tokens"),
            Some(32_000)
        );
        std::fs::write(
            &p,
            "[model]\nprovider = \"deepseek\"\nmax_context_tokens = 5\n",
        )
        .unwrap();
        assert_eq!(
            Config::load(Some(p))
                .unwrap()
                .get_int("model.max_context_tokens"),
            Some(1_000)
        );
    }

    #[test]
    fn console_base_url_defaults_and_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[model]\nprovider = \"deepseek\"\n").unwrap();
        assert_eq!(
            Config::load(Some(p.clone()))
                .unwrap()
                .get_str("model.console_base_url")
                .as_deref(),
            Some("https://opencode.ai/console")
        );
        std::fs::write(
            &p,
            "[model]\nprovider = \"deepseek\"\nconsole_base_url = \"https://console.example\"\n",
        )
        .unwrap();
        assert_eq!(
            Config::load(Some(p))
                .unwrap()
                .get_str("model.console_base_url")
                .as_deref(),
            Some("https://console.example")
        );
    }
}
