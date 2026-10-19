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

fn default_max_tool_result_chars() -> i64 {
    100_000
}

fn default_question_timeout_s() -> i64 {
    600
}

/// Bounds for the agent loop (`[agent]`). @since 0.29.9
#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    /// Tool-calling rounds a single turn may run before the loop stops offering tools and
    /// asks the model for a conclusion from what it already gathered. Clamped to
    /// `MIN_TOOL_ITERATIONS..=MAX_TOOL_ITERATIONS` by the accessor. @since 0.29.9
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: i64,
    /// Largest single tool result the kernel hands back to the model. Clamped to
    /// `MIN_TOOL_RESULT_CHARS..=MAX_TOOL_RESULT_CHARS` by the accessor. @since 0.29.9
    #[serde(default = "default_max_tool_result_chars")]
    pub max_tool_result_chars: i64,
    /// Seconds a `question` may wait for an answer before the kernel settles it as
    /// unanswered. Clamped to `MIN_QUESTION_TIMEOUT_S..=MAX_QUESTION_TIMEOUT_S` by the
    /// accessor. @since 0.29.9
    #[serde(default = "default_question_timeout_s")]
    pub question_timeout_s: i64,
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_tool_iterations(),
            max_tool_result_chars: default_max_tool_result_chars(),
            question_timeout_s: default_question_timeout_s(),
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

/// Lower bound of `agent.max_tool_result_chars` (a zero budget would truncate everything).
/// @since 0.29.9
pub const MIN_TOOL_RESULT_CHARS: i64 = 1_000;
/// Upper bound of `agent.max_tool_result_chars`. @since 0.29.9
pub const MAX_TOOL_RESULT_CHARS: i64 = 1_000_000;

/// Clamp a configured tool-result budget into the supported range. @since 0.29.9
pub fn clamp_tool_result_chars(value: i64) -> i64 {
    value.clamp(MIN_TOOL_RESULT_CHARS, MAX_TOOL_RESULT_CHARS)
}

/// Lower bound of `agent.question_timeout_s` (a zero timeout would settle every question
/// instantly). @since 0.29.9
pub const MIN_QUESTION_TIMEOUT_S: i64 = 1;
/// Upper bound of `agent.question_timeout_s` (one day). @since 0.29.9
pub const MAX_QUESTION_TIMEOUT_S: i64 = 86_400;

/// Clamp a configured question timeout into the supported range. @since 0.29.9
pub fn clamp_question_timeout_s(value: i64) -> i64 {
    value.clamp(MIN_QUESTION_TIMEOUT_S, MAX_QUESTION_TIMEOUT_S)
}

/// Default `[web] spot_interval_secs` — matches the pre-existing hard-coded 60s sweep.
pub fn default_spot_interval_secs() -> u64 {
    60
}

/// Default `[web] spot_sample` — matches the pre-existing hard-coded 8 files per tick.
pub fn default_spot_sample() -> usize {
    8
}

/// Default `[web] verify_on_request` — request-path verification is on by default.
pub fn default_verify_on_request() -> bool {
    true
}

/// Default `[web] verify_cache_ttl_secs` — the request-path memo window.
pub fn default_verify_cache_ttl_secs() -> u64 {
    60
}

/// Lower bound of `web.spot_interval_secs` (`tokio::time::interval(0)` would panic).
pub const MIN_SPOT_INTERVAL_SECS: u64 = 5;
/// Upper bound of `web.spot_interval_secs`.
pub const MAX_SPOT_INTERVAL_SECS: u64 = 3600;
/// Lower bound of `web.spot_sample` (a zero budget would never sample anything).
pub const MIN_SPOT_SAMPLE: usize = 1;
/// Upper bound of `web.spot_sample`.
pub const MAX_SPOT_SAMPLE: usize = 1024;
/// Lower bound of `web.verify_cache_ttl_secs` (a zero TTL would hash on every request).
pub const MIN_VERIFY_CACHE_TTL_SECS: u64 = 1;
/// Upper bound of `web.verify_cache_ttl_secs`.
pub const MAX_VERIFY_CACHE_TTL_SECS: u64 = 3600;

/// Clamp a configured runtime spot-check interval into the supported range.
pub fn clamp_spot_interval_secs(value: u64) -> u64 {
    value.clamp(MIN_SPOT_INTERVAL_SECS, MAX_SPOT_INTERVAL_SECS)
}

/// Clamp a configured runtime spot-check sample size into the supported range.
pub fn clamp_spot_sample(value: usize) -> usize {
    value.clamp(MIN_SPOT_SAMPLE, MAX_SPOT_SAMPLE)
}

/// Clamp a configured request-path verification cache TTL into the supported range.
pub fn clamp_verify_cache_ttl_secs(value: u64) -> u64 {
    value.clamp(MIN_VERIFY_CACHE_TTL_SECS, MAX_VERIFY_CACHE_TTL_SECS)
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

/// `[web]` HTTP surface options: proxy-header trust for client-IP derivation,
/// the directory holding the persisted login-limiter state, and the integrity
/// verification knobs (runtime spot-check cadence + request-path verification).
///
/// `trust_proxy_headers` defaults to `false`; enabling it only takes effect when
/// the direct TCP peer is loopback (design D1), so it is safe for same-host proxies.
/// The four integrity knobs default to the pre-existing behaviour (60s sweep,
/// 8 files per tick, request-path verification on, 60s memo) and are clamped by
/// the `clamped_*` accessors before use.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct WebSection {
    pub trust_proxy_headers: bool,
    /// Overrides `$XDG_DATA_HOME/agentj`; `None` means "resolve from the environment".
    pub state_dir: Option<PathBuf>,
    /// Runtime spot-check cadence in seconds (clamped to 5..=3600).
    pub spot_interval_secs: u64,
    /// Files hashed per spot-check tick (clamped to 1..=1024).
    pub spot_sample: usize,
    /// Whether the static surface verifies served assets on request.
    pub verify_on_request: bool,
    /// How long a request-path verification verdict is memoized (clamped to 1..=3600).
    pub verify_cache_ttl_secs: u64,
}

impl Default for WebSection {
    fn default() -> Self {
        Self {
            trust_proxy_headers: false,
            state_dir: None,
            spot_interval_secs: default_spot_interval_secs(),
            spot_sample: default_spot_sample(),
            verify_on_request: default_verify_on_request(),
            verify_cache_ttl_secs: default_verify_cache_ttl_secs(),
        }
    }
}

impl WebSection {
    /// `spot_interval_secs` clamped into the supported range.
    pub fn clamped_spot_interval_secs(&self) -> u64 {
        clamp_spot_interval_secs(self.spot_interval_secs)
    }

    /// `spot_sample` clamped into the supported range.
    pub fn clamped_spot_sample(&self) -> usize {
        clamp_spot_sample(self.spot_sample)
    }

    /// `verify_cache_ttl_secs` clamped into the supported range.
    pub fn clamped_verify_cache_ttl_secs(&self) -> u64 {
        clamp_verify_cache_ttl_secs(self.verify_cache_ttl_secs)
    }
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
    #[serde(default)]
    pub web: Option<WebSection>,
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
            web: None,
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

    /// Largest single tool result (in characters) the kernel may hand back to the model,
    /// clamped into the supported range. @since 0.29.9
    pub fn max_tool_result_chars(&self) -> usize {
        clamp_tool_result_chars(self.inner.agent.max_tool_result_chars) as usize
    }

    /// Seconds a `question` may wait for an answer before it is settled as unanswered,
    /// clamped into the supported range. @since 0.29.9
    pub fn question_timeout_s(&self) -> u64 {
        clamp_question_timeout_s(self.inner.agent.question_timeout_s) as u64
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
            "agent.max_tool_result_chars" => Some(clamp_tool_result_chars(
                self.inner.agent.max_tool_result_chars,
            )),
            "agent.question_timeout_s" => Some(clamp_question_timeout_s(
                self.inner.agent.question_timeout_s,
            )),
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
    fn web_section_defaults_trust_proxy_headers_to_false() {
        let cfg: AgentJConfig = toml::from_str("").unwrap();
        assert!(cfg.web.is_none(), "an absent [web] section stays None");
        let web = WebSection::default();
        assert!(!web.trust_proxy_headers, "forwarded headers are opt-in");
        assert!(web.state_dir.is_none());
    }

    #[test]
    fn web_section_reads_trust_and_state_dir() {
        let cfg: AgentJConfig =
            toml::from_str("[web]\ntrust_proxy_headers = true\nstate_dir = \"/var/lib/agentj\"\n")
                .unwrap();
        let web = cfg.web.expect("[web] section present");
        assert!(web.trust_proxy_headers);
        assert_eq!(
            web.state_dir,
            Some(std::path::PathBuf::from("/var/lib/agentj"))
        );
    }

    #[test]
    fn web_section_tolerates_unknown_keys() {
        // The future `trusted_proxies` CIDR whitelist must not break older binaries.
        let cfg: AgentJConfig = toml::from_str(
            "[web]\ntrusted_proxies = [\"10.0.0.0/8\"]\ntrust_proxy_headers = true\n",
        )
        .unwrap();
        assert!(cfg.web.expect("[web] section present").trust_proxy_headers);
    }

    #[test]
    fn web_section_integrity_knobs_default_to_today_behaviour() {
        let section = WebSection::default();
        assert_eq!(section.spot_interval_secs, default_spot_interval_secs());
        assert_eq!(section.spot_sample, default_spot_sample());
        assert!(section.verify_on_request);
        assert_eq!(
            section.verify_cache_ttl_secs,
            default_verify_cache_ttl_secs()
        );
        assert_eq!(section.spot_interval_secs, 60);
        assert_eq!(section.spot_sample, 8);
        assert_eq!(section.verify_cache_ttl_secs, 60);
    }

    #[test]
    fn web_section_integrity_knobs_are_clamped() {
        let section = WebSection {
            spot_interval_secs: 1,
            spot_sample: 0,
            verify_cache_ttl_secs: 99_999,
            ..WebSection::default()
        };
        assert_eq!(section.clamped_spot_interval_secs(), MIN_SPOT_INTERVAL_SECS);
        assert_eq!(section.clamped_spot_sample(), MIN_SPOT_SAMPLE);
        assert_eq!(
            section.clamped_verify_cache_ttl_secs(),
            MAX_VERIFY_CACHE_TTL_SECS
        );
        assert_eq!(clamp_spot_interval_secs(0), 5);
        assert_eq!(clamp_spot_interval_secs(u64::MAX), 3600);
        assert_eq!(clamp_spot_sample(4096), 1024);
    }

    #[test]
    fn web_section_reads_the_integrity_knobs_from_toml() {
        let cfg = parse_config(
            "[web]\nspot_interval_secs = 30\nspot_sample = 2\nverify_on_request = false\nverify_cache_ttl_secs = 15\n",
        );
        let web = cfg.inner().web.as_ref().expect("web section");
        assert_eq!(web.spot_interval_secs, 30);
        assert_eq!(web.spot_sample, 2);
        assert!(!web.verify_on_request);
        assert_eq!(web.verify_cache_ttl_secs, 15);
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
    fn max_tool_result_chars_defaults_and_clamps() {
        let cfg = parse_config("");
        assert_eq!(cfg.max_tool_result_chars(), 100_000);
        assert_eq!(
            parse_config("[agent]\nmax_tool_result_chars = 2000\n").max_tool_result_chars(),
            2000
        );
        // Bounds are pinned against the literals, not the constants under test, so
        // mutating a constant cannot keep the test green.
        assert_eq!(
            parse_config("[agent]\nmax_tool_result_chars = 0\n").max_tool_result_chars(),
            1_000
        );
        assert_eq!(
            parse_config("[agent]\nmax_tool_result_chars = 9999999\n").max_tool_result_chars(),
            1_000_000
        );
    }

    #[test]
    fn max_tool_result_chars_is_readable_through_get_int() {
        let cfg = parse_config("[agent]\nmax_tool_result_chars = 2000\n");
        assert_eq!(cfg.get_int("agent.max_tool_result_chars"), Some(2000));
        // The clamp bounds and the default drive `get_int` too, mirroring
        // `agent_section_defaults_and_clamps`.
        let low = parse_config("[agent]\nmax_tool_result_chars = 0\n");
        assert_eq!(low.get_int("agent.max_tool_result_chars"), Some(1_000));
        let high = parse_config("[agent]\nmax_tool_result_chars = 9999999\n");
        assert_eq!(high.get_int("agent.max_tool_result_chars"), Some(1_000_000));
        assert_eq!(
            parse_config("").get_int("agent.max_tool_result_chars"),
            Some(100_000)
        );
    }

    #[test]
    fn question_timeout_defaults_to_ten_minutes_and_clamps() {
        assert_eq!(parse_config("").question_timeout_s(), 600);
        assert_eq!(
            parse_config("[agent]\nquestion_timeout_s = 300\n").question_timeout_s(),
            300
        );
        assert_eq!(
            parse_config("[agent]\nquestion_timeout_s = 0\n").question_timeout_s(),
            1
        );
        assert_eq!(
            parse_config("[agent]\nquestion_timeout_s = 999999\n").question_timeout_s(),
            86_400
        );
        // A negative value must clamp before the i64 → u64 cast, never wrap.
        assert_eq!(
            parse_config("[agent]\nquestion_timeout_s = -5\n").question_timeout_s(),
            1
        );
    }

    #[test]
    fn question_timeout_s_is_readable_through_get_int() {
        let cfg = parse_config("[agent]\nquestion_timeout_s = 300\n");
        assert_eq!(cfg.get_int("agent.question_timeout_s"), Some(300));
        let low = parse_config("[agent]\nquestion_timeout_s = 0\n");
        assert_eq!(low.get_int("agent.question_timeout_s"), Some(1));
        let high = parse_config("[agent]\nquestion_timeout_s = 999999\n");
        assert_eq!(high.get_int("agent.question_timeout_s"), Some(86_400));
        assert_eq!(
            parse_config("").get_int("agent.question_timeout_s"),
            Some(600)
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
