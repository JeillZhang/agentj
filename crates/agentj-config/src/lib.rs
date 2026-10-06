pub mod merge;
pub mod write;
use agentj_core::module::ConfigProvider;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
pub use write::{set_model_defaults, write_model_defaults};

#[derive(Debug, Clone, Deserialize)]
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
}

fn default_model() -> String {
    "deepseek-chat".into()
}

impl Default for ModelSection {
    fn default() -> Self {
        Self {
            provider: "deepseek".into(),
            providers: vec![],
            default_model: default_model(),
        }
    }
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
            },
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
    fn get_int(&self, _key: &str) -> Option<i64> {
        None
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
}
