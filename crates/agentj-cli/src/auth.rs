//! `agentj auth login|status|logout` (design D1/D2, G1/G2).

use std::path::Path;

use agentj_auth::{AuthStore, Credential, ResolvedKey, redact, resolve_credential};
use clap::Subcommand;

pub const OPENCODE_GO_BASE: &str = "https://opencode.ai/zen/go";

#[derive(Subcommand)]
pub enum AuthAction {
    /// Store a workspace API key (0600) and optionally point config.toml at a model.
    Login {
        #[arg(long, default_value = "opencode-go")]
        provider: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        default_model: Option<String>,
        /// Skip the live catalog display (no network); the credential is still written.
        #[arg(long)]
        offline: bool,
    },
    /// Show the resolved key source and the last 4 digits (never the full key).
    Status {
        #[arg(long, default_value = "opencode-go")]
        provider: String,
        #[arg(long)]
        offline: bool,
    },
    /// Remove a stored credential; the file is deleted when it becomes empty.
    Logout {
        #[arg(long, default_value = "opencode-go")]
        provider: String,
    },
}

#[derive(Debug, Clone)]
pub struct LoginArgs {
    pub provider: String,
    pub api_key: Option<String>,
    pub default_model: Option<String>,
    pub offline: bool,
}

/// Pick a non-empty key from the flag or piped stdin; empty input is rejected.
pub fn parse_login_key(flag: Option<&str>, stdin: Option<&str>) -> Result<String, String> {
    if let Some(k) = flag.map(str::trim).filter(|k| !k.is_empty()) {
        return Ok(k.to_string());
    }
    if let Some(k) = stdin.map(str::trim).filter(|k| !k.is_empty()) {
        return Ok(k.to_string());
    }
    Err("密钥为空：请通过 --api-key 或 stdin 管道提供".into())
}

/// One status line: provider, key source, redacted key. Never the full key.
pub fn format_status_line(provider: &str, key: &ResolvedKey) -> String {
    format!(
        "{provider} · 密钥来源 {:?} · {}",
        key.source,
        redact(&key.key)
    )
}

fn now_epoch() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Login with an injectable live-catalog fetcher so `--offline` and tests can prove
/// zero HTTP attempts.
pub async fn run_login_with<F>(
    store: &AuthStore,
    args: &LoginArgs,
    fetch: &F,
    config_path: &Path,
) -> Result<(), String>
where
    F: Fn(&str) -> Result<Option<Vec<String>>, String>,
{
    let key = parse_login_key(args.api_key.as_deref(), None)?;
    store
        .set(
            &args.provider,
            Credential::ApiKey {
                key: key.clone(),
                label: None,
                created_at: Some(now_epoch()),
            },
        )
        .map_err(|e| e.to_string())?;

    let live_ids = if args.offline {
        None
    } else {
        match fetch(&key) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("live 目录不可用（{e}），已按提交件 overlay 展示；凭据已保存");
                None
            }
        }
    };

    if let Some(model) = &args.default_model {
        let catalog = agentj_model::catalog::ModelCatalog::from_overlay_and_live(
            agentj_model::catalog::ModelCatalog::builtin(),
            live_ids,
        );
        let ids = catalog.ids();
        if !ids.iter().any(|id| id == model) {
            return Err(format!("未知模型 {model}；可用：{}", ids.join(", ")));
        }
        agentj_config::write_model_defaults(config_path, &args.provider, model)?;
    }
    Ok(())
}

/// Login using the real live-catalog fetcher (5s timeout, failure only warns).
pub async fn run_login(
    store: &AuthStore,
    args: &LoginArgs,
    config_path: &Path,
) -> Result<(), String> {
    if args.offline {
        return run_login_with(store, args, &|_k: &str| Ok(None), config_path).await;
    }
    let key = parse_login_key(args.api_key.as_deref(), None)?;
    let result: Result<Option<Vec<String>>, String> =
        match agentj_model::catalog_source::fetch_live_models(OPENCODE_GO_BASE, Some(&key)).await {
            Ok(ids) => Ok(Some(ids)),
            Err(e) => Err(e.to_string()),
        };
    run_login_with(store, args, &|_k: &str| result.clone(), config_path).await
}

/// Print provider, key source, last 4 digits, credential path and model count.
pub async fn run_status(store: &AuthStore, provider: &str, offline: bool) -> Result<(), String> {
    match resolve_credential(provider, None, None, store) {
        Ok(Some(r)) => {
            println!("{}", format_status_line(provider, &r));
            println!("凭据文件：{}", store.path().display());
            let live = if offline {
                None
            } else {
                agentj_model::catalog_source::fetch_live_models(OPENCODE_GO_BASE, Some(&r.key))
                    .await
                    .ok()
            };
            let catalog = agentj_model::catalog::ModelCatalog::from_overlay_and_live(
                agentj_model::catalog::ModelCatalog::builtin(),
                live,
            );
            println!("模型数：{}", catalog.ids().len());
            println!("提示：密钥有效性将在首次真实请求时暴露");
        }
        Ok(None) => println!("{provider}：未找到凭据（{}）", store.path().display()),
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

/// Remove a stored credential; the file is deleted when it becomes empty.
pub fn run_logout(store: &AuthStore, provider: &str) -> Result<(), String> {
    let removed = store.remove(provider).map_err(|e| e.to_string())?;
    if removed {
        println!("已移除 {provider} 的凭据");
    } else {
        println!("{provider}：没有可移除的凭据");
    }
    Ok(())
}

fn read_stdin_key() -> Result<Option<String>, String> {
    use std::io::{BufRead, IsTerminal};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(Some(line))
}

/// Dispatch `agentj auth <action>`; never touches the kernel or the license gate.
pub async fn run_auth(action: &AuthAction, config_path: &str) -> anyhow::Result<()> {
    let store = AuthStore::with_default_path().map_err(|e| anyhow::anyhow!("{e}"))?;
    match action {
        AuthAction::Login {
            provider,
            api_key,
            default_model,
            offline,
        } => {
            let stdin_key = if api_key.is_none() {
                read_stdin_key().map_err(|e| anyhow::anyhow!("{e}"))?
            } else {
                None
            };
            let args = LoginArgs {
                provider: provider.clone(),
                api_key: api_key.clone().or(stdin_key),
                default_model: default_model.clone(),
                offline: *offline,
            };
            run_login(&store, &args, Path::new(config_path))
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("已保存 {provider} 的凭据（{}）", store.path().display());
            println!("提示：密钥有效性将在首次真实请求时暴露");
        }
        AuthAction::Status { provider, offline } => run_status(&store, provider, *offline)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        AuthAction::Logout { provider } => {
            run_logout(&store, provider).map_err(|e| anyhow::anyhow!("{e}"))?
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentj_auth::KeySource;

    #[test]
    fn login_requires_a_non_empty_key() {
        assert!(parse_login_key(Some("  "), None).is_err());
        assert_eq!(parse_login_key(Some("sk-1"), None).unwrap(), "sk-1");
    }

    #[test]
    fn status_line_never_contains_the_full_key() {
        let line = format_status_line(
            "opencode-go",
            &ResolvedKey {
                key: "sk-test-1234".into(),
                source: KeySource::File,
            },
        );
        assert!(line.contains("…1234"));
        assert!(!line.contains("sk-test-1234"));
    }

    #[tokio::test]
    async fn offline_login_never_calls_the_catalog_fetcher() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let called = std::cell::Cell::new(false);
        run_login_with(
            &store,
            &LoginArgs {
                provider: "opencode-go".into(),
                api_key: Some("sk-1".into()),
                default_model: None,
                offline: true,
            },
            &|_key: &str| {
                called.set(true);
                Ok(None)
            },
            &dir.path().join("config.toml"),
        )
        .await
        .unwrap();
        assert!(
            !called.get(),
            "--offline must short-circuit before the catalog fetch"
        );
        assert!(store.get("opencode-go").unwrap().is_some());
    }

    #[tokio::test]
    async fn unknown_default_model_is_rejected_and_lists_available_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "[model]\nprovider = \"deepseek\"\n").unwrap();
        let err = run_login_with(
            &store,
            &LoginArgs {
                provider: "opencode-go".into(),
                api_key: Some("sk-1".into()),
                default_model: Some("opencode-go/not-a-model".into()),
                offline: true,
            },
            &|_k: &str| Ok(None),
            &cfg,
        )
        .await
        .unwrap_err();
        assert!(err.contains("opencode-go/not-a-model"), "{err}");
        assert!(
            err.contains("opencode-go/"),
            "must list available ids: {err}"
        );
        assert!(
            std::fs::read_to_string(&cfg).unwrap().contains("deepseek"),
            "config must be untouched"
        );
    }
}
