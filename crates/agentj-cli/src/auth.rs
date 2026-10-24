//! `agentj auth login|status|logout` (design D1/D2, G1/G2).

use std::path::Path;

use agentj_auth::oauth::{
    DeviceCode, PollOutcome, Tokens, device_poll, device_start, now_epoch as oauth_now_epoch,
};
use agentj_auth::{AuthStore, Credential, ResolvedKey, redact, resolve_credential};
#[cfg(test)]
use agentj_core::sync::lock_or_recover;
use clap::Subcommand;

pub const OPENCODE_GO_BASE: &str = agentj_model::catalog::OPENCODE_GO_BASE;

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
        /// Log in with the browser device-code flow instead of pasting an API key.
        /// @since 0.27.0
        #[arg(long, conflicts_with = "api_key")]
        device: bool,
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
    /// Use the RFC 8628 device-code flow (mutually exclusive with `api_key`). @since 0.27.0
    pub device: bool,
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
///
/// An OAuth credential prints its type/org/expiry instead of a key hint — the access and
/// refresh tokens are never rendered (`design D9`).
pub fn format_status_line(
    provider: &str,
    key: &ResolvedKey,
    credential: Option<&Credential>,
) -> String {
    match credential {
        Some(Credential::OAuth {
            expires, org_id, ..
        }) => format!(
            "{provider} · 凭据类型 oauth · 来源 {:?} · org {} · 有效期至 {expires}",
            key.source,
            org_id.as_deref().unwrap_or("(无)")
        ),
        _ => format!(
            "{provider} · 密钥来源 {:?} · {}",
            key.source,
            redact(&key.key)
        ),
    }
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

/// Drives the RFC 8628 device flow; `sleep` is injected so tests never wait on the real
/// clock. The deadline comes from the upstream `expires_in` (never an unbounded poll).
pub async fn run_login_device_with<F, Fut>(
    store: &AuthStore,
    args: &LoginArgs,
    console_base: &str,
    sleep: F,
) -> Result<(), String>
where
    F: Fn(u64) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let code = start_device(console_base).await?;
    let deadline = oauth_now_epoch() + code.expires_in as i64;
    let tokens = poll_until_authorized(console_base, &code, deadline, &sleep).await?;
    persist_credential(store, args, tokens)
}

/// Ask the console for a device code and show the user what to do with it.
async fn start_device(console_base: &str) -> Result<DeviceCode, String> {
    let code = device_start(console_base)
        .await
        .map_err(|e| e.to_string())?;
    println!("在浏览器中打开：{}", code.verification_uri);
    println!("设备码：{}", code.user_code);
    Ok(code)
}

/// Poll until the user authorizes; the interval only ever grows (`SlowDown`) and the
/// upstream deadline is the sole exit for a code nobody confirms.
async fn poll_until_authorized<F, Fut>(
    console_base: &str,
    code: &DeviceCode,
    deadline: i64,
    sleep: &F,
) -> Result<Tokens, String>
where
    F: Fn(u64) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut interval = code.interval;
    loop {
        if oauth_now_epoch() > deadline {
            return Err("设备码已过期，请重新运行 agentj auth login --device".into());
        }
        sleep(interval).await;
        let outcome = device_poll(console_base, &code.device_code, interval)
            .await
            .map_err(|e| e.to_string())?;
        match outcome {
            PollOutcome::Pending => continue,
            PollOutcome::SlowDown { interval: next } => interval = next,
            PollOutcome::Denied => return Err("授权被拒绝".into()),
            PollOutcome::Expired => return Err("设备码已过期".into()),
            PollOutcome::Error(msg) => return Err(format!("设备授权失败：{msg}")),
            PollOutcome::Authorized(t) => return Ok(t),
        }
    }
}

/// Store the OAuth credential (replacing any previous one) and report the login.
fn persist_credential(store: &AuthStore, args: &LoginArgs, t: Tokens) -> Result<(), String> {
    store
        .set(
            &args.provider,
            Credential::OAuth {
                access: t.access,
                refresh: t.refresh,
                expires: t.expires_at,
                account_id: None,
                org_id: t.org_id.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
    println!(
        "已登录 {}（已替换原有凭据；{}）",
        args.provider,
        t.org_id.unwrap_or_else(|| "无 org 信息".into())
    );
    Ok(())
}

/// Print the live catalog size after a successful non-offline login (never the credential).
async fn print_catalog_summary(store: &AuthStore, provider: &str, offline: bool) {
    if offline {
        return;
    }
    let key = store
        .get(provider)
        .ok()
        .flatten()
        .and_then(|c| c.key().ok().map(str::to_string));
    let live = match key {
        Some(k) => agentj_model::catalog_source::fetch_live_models(OPENCODE_GO_BASE, Some(&k))
            .await
            .ok(),
        None => None,
    };
    let catalog = agentj_model::catalog::ModelCatalog::from_overlay_and_live(
        agentj_model::catalog::ModelCatalog::builtin(),
        live,
    );
    println!("模型数：{}", catalog.ids().len());
}

/// Print provider, key source, last 4 digits, credential path and model count.
pub async fn run_status(store: &AuthStore, provider: &str, offline: bool) -> Result<(), String> {
    match resolve_credential(provider, None, None, store) {
        Ok(Some(r)) => {
            let stored = store.get(provider).ok().flatten();
            println!("{}", format_status_line(provider, &r, stored.as_ref()));
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
            device,
        } => {
            let stdin_key = if api_key.is_none() && !*device {
                read_stdin_key().map_err(|e| anyhow::anyhow!("{e}"))?
            } else {
                None
            };
            let args = LoginArgs {
                provider: provider.clone(),
                api_key: api_key.clone().or(stdin_key),
                default_model: default_model.clone(),
                offline: *offline,
                device: *device,
            };
            if args.device {
                run_login_device_with(
                    &store,
                    &args,
                    agentj_auth::oauth::OPENCODE_CONSOLE_BASE,
                    |secs| tokio::time::sleep(std::time::Duration::from_secs(secs)),
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
                println!("凭据文件：{}", store.path().display());
                print_catalog_summary(&store, provider, *offline).await;
                println!("提示：access token 将在到期前自动刷新");
            } else {
                run_login(&store, &args, Path::new(config_path))
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                println!("已保存 {provider} 的凭据（{}）", store.path().display());
                println!("提示：密钥有效性将在首次真实请求时暴露");
            }
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
            None,
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
                device: false,
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
    async fn login_with_default_model_creates_missing_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let cfg = dir.path().join("config.toml");
        run_login_with(
            &store,
            &LoginArgs {
                provider: "opencode-go".into(),
                api_key: Some("sk-1".into()),
                default_model: Some("opencode-go/kimi-k3".into()),
                offline: true,
                device: false,
            },
            &|_k: &str| Ok(None),
            &cfg,
        )
        .await
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("provider = \"opencode-go\""), "{text}");
        assert!(
            text.contains("default_model = \"opencode-go/kimi-k3\""),
            "{text}"
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
                device: false,
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

    async fn mount_device_code(server: &wiremock::MockServer, user_code: &str) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/code"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code":"dc","user_code":user_code,
                "verification_uri_complete":"/console/device?user_code=x",
                "expires_in":600,"interval":5 })),
            )
            .mount(server)
            .await;
    }

    async fn mount_device_token_once(
        server: &wiremock::MockServer,
        status: u16,
        body: serde_json::Value,
    ) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .up_to_n_times(1)
            .mount(server)
            .await;
    }

    async fn mount_device_token(
        server: &wiremock::MockServer,
        status: u16,
        body: serde_json::Value,
    ) {
        wiremock::Mock::given(wiremock::matchers::path("/auth/device/token"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    fn device_args() -> LoginArgs {
        LoginArgs {
            provider: "opencode-go".into(),
            api_key: None,
            default_model: None,
            offline: true,
            device: true,
        }
    }

    #[tokio::test]
    async fn device_login_polls_with_the_upstream_interval_and_writes_oauth() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let server = wiremock::MockServer::start().await;
        mount_device_code(&server, "ABCD-1234").await;
        mount_device_token_once(&server, 400, serde_json::json!({"error":"slow_down"})).await;
        mount_device_token(
            &server,
            200,
            serde_json::json!({
                "access_token":"a","refresh_token":"r","expires_in":600,"org_id":"org_1"}),
        )
        .await;

        let sleeps: std::sync::Arc<std::sync::Mutex<Vec<u64>>> =
            std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let args = device_args();
        run_login_device_with(&store, &args, &server.uri(), |secs| {
            let sleeps = sleeps.clone();
            async move {
                lock_or_recover(&sleeps).push(secs);
            }
        })
        .await
        .unwrap();

        assert_eq!(
            lock_or_recover(&sleeps).as_slice(),
            [5, 10],
            "slow_down bumps the interval"
        );
        assert!(matches!(
            store.get("opencode-go").unwrap().unwrap(),
            Credential::OAuth { ref org_id, .. } if org_id.as_deref() == Some("org_1")
        ));
    }

    #[tokio::test]
    async fn device_login_failure_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = AuthStore::with_path(dir.path().join("auth.json"));
        let server = wiremock::MockServer::start().await;
        mount_device_code(&server, "ABCD-1234").await;
        mount_device_token(&server, 400, serde_json::json!({"error":"expired_token"})).await;

        let args = device_args();
        let err = run_login_device_with(&store, &args, &server.uri(), |_| async {})
            .await
            .unwrap_err();
        assert!(err.contains("过期"), "got {err}");
        assert!(
            !store.path().exists(),
            "no credential file may be created on failure"
        );
    }

    #[test]
    fn device_and_api_key_are_mutually_exclusive() {
        use clap::Parser;
        let r =
            crate::Cli::try_parse_from(["agentj", "auth", "login", "--device", "--api-key", "k"]);
        assert!(r.is_err(), "--device must conflict with --api-key");
    }

    #[test]
    fn status_line_for_oauth_never_prints_tokens() {
        let line = format_status_line(
            "opencode-go",
            &ResolvedKey {
                key: "a".into(),
                source: KeySource::File,
            },
            Some(&Credential::OAuth {
                access: "access-secret".into(),
                refresh: "refresh-secret".into(),
                expires: 4_000_000_000,
                account_id: None,
                org_id: Some("org_1".into()),
            }),
        );
        assert!(line.contains("oauth") && line.contains("org_1"));
        assert!(!line.contains("access-secret") && !line.contains("refresh-secret"));
    }
}
