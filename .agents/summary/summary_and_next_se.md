# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.0 Web/TUI 链接 opencode-go API Key（后端部分）**
> ——来源：设计 `docs/superpowers/specs/2026-10-08-agentj-ui-opencode-go-api-key-link-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-08-agentj-ui-opencode-go-api-key-link-plan.md`
> （T0–T12 / T21–T22 / T23 + Phase-5 评审修复）。
> 后端范围：`agentj-protocol`（6 个 wire 类型 + 2 方法 + 6 schemas）、`agentj-core`
> （`ProviderRegistry::remove`）、`agentj-ipc`（trait 7 方法 + 2 dispatch 臂）、`agentj-cli`
> （`set_api_key` / `clear_credential` + 运行期激活 helper + 内置条目 + 设备码激活）、
> `docs/specs/ipc-protocol.md`（§8.28 / §8.29 等）、`docs/changelog/v0.29.md`、版本元数据。
> `PROTOCOL_VERSION` 0.17.0 → **0.18.0**（26 → **28** 方法 / 72 → **78** schemas）；
> 工作区 0.28.0 → **0.29.0**。前端部分见 `summary_and_next_fe.md`。

## v0.29.0（Web/TUI 链接 opencode-go API Key，后端部分）✅（T0–T12 / T21–T22 / T23 + 评审修复）

- ✅ **`agentj-protocol`（T0/T1/T2）**：新增 6 个 wire 类型（`AuthSetApiKeyParams` /
  `AuthSetApiKeyResult` / `ApiKeyOutcome`，含 `#[serde(other)]` 的 `Unknown` 向前兼容 /
  `ApiKeyLinkError` / `AuthClearCredentialParams` / `AuthClearCredentialResult`）；
  `PROTOCOL_VERSION` → **0.18.0**，`schemas/*.json` 72 → **78**，`protocol/methods.json`
  26 → **28** 方法（`auth/setApiKey` / `auth/clearCredential`），registry / contract / drift
  密封测试同步。
- ✅ **`agentj-core`（T3）**：加性方法 `ProviderRegistry::remove(name) -> bool`（移除后
  `list()` / `get()` 不含该项，默认 provider 指向被移除项时不 panic）；既有排序 / models
  投影用例零回归。
- ✅ **`agentj-ipc`（T4）**：`ProviderService` trait 扩到 **7** 个 async 方法 + 2 个 dispatch 臂；
  非法 params 结构 → `-32602`，handle 未注入 → 7 个方法统一
  `-32603 provider_service_unavailable`；ipc 不新增 auth / model 依赖。
- ✅ **`agentj-cli`（T5/T6）**：有效 provider 列表 = `config.model.providers` ∪ 内置
  `opencode-go` 默认条目（`base_url = https://opencode.ai/zen/go`、无 `api_key_env`；
  显式条目优先、不写用户 TOML），`build_providers` 与 `CliProviderService` 共用同一函数；
  运行期激活 helper = `build_opencode_go`（api_key → `RefreshingProvider::static`，
  OAuth → `RefreshingProvider::oauth`）+ 替换 `registry.register` / `remove` + 失效该 provider
  的模型与用量缓存。
- ✅ **`agentj-cli`（T7/T8/T9/T10）**：`auth/setApiKey` 一次 `GET {console}/api/v2/config`
  完成校验 + 目录 + 缓存播种，三态 `verified`（落盘 + 激活 + `source:"account"`）/
  `rejected`（401/403：**不落盘、不注册**，`error.kind` ∈ `unauthorized` / `entitlement`，
  文案内核自产） / `unverified`（超时 / 网络 / 5xx / 畸形 / 200 但无 go 模型：**仍落盘**，
  目录退 live ∩ overlay）；`effective_source` 如实回告 env 遮蔽；非法参数走 `-32602` +
  `error.data.reason`（`invalid_params` / `unknown_provider` / `unsupported_provider`）。
  `auth/clearCredential` 移除凭据并按剩余凭据收敛（仍有 env key → 重建注册；无 key → 注销，
  `auth/status.authenticated=false`），无条目 → `cleared:false` 成功响应。
  `auth/devicePoll` 的 `authorized` 分支复用同一激活路径（免重启）。
- ✅ **E2E（T11/T12）**：`crates/agentj-cli/tests/e2e_serve.rs` 真实 socket 覆盖
  「链接 → `model/list` 返回 `account` → 解除 → 未认证」、rejected key、未注入
  `provider_service` 的内核、以及链接 / 解除全程 `config.toml` 未被修改。
- ✅ **`docs/specs/ipc-protocol.md` + changelog（T23）**：§8.1 方法表 28 行；新增 §8.28 / §8.29
  契约（params / result / 三态 / `error.data.reasons` / 领域失败即结果数据 / `effective_source` /
  no-secret 不变式 / `ApiKeyLinkError.kind` 为 `String` 且当前只产出 `unauthorized` /
  `entitlement` / 换入瞬间进行中轮次可能用旧凭据的风险说明）；§3.4 标注复用既有错误码 +
  `-32603 reason:"activation_failed"`；§10 / §11.3 记 `@since 0.18.0`（0.17.0 → 0.18.0）；
  新建 `docs/changelog/v0.29.md` + 索引行。
- ✅ **版本元数据（T21/T22）**：`Cargo.toml` / `Cargo.lock` 与 `scripts/check-version.sh` 同步
  **0.29.0**（`PROTOCOL_VERSION` **0.18.0**）；desktop 三清单与前端 lockfile 亦同步
  **0.29.0**（详见 `summary_and_next_fe.md`），`check-version.sh` = `version-ok`。
- ✅ **评审修复（Phase 5，`fix:` 提交）**：整体分支持续只读评审得 1 HIGH / 1 MEDIUM / 4 LOW，
  全部闭合：**H1** `clear_credential` 增加 `require_kind("api_key")` 守卫——已知非 opencode-go
  provider 返回 `-32602 unsupported_provider` 且**凭据与注册表均不被触碰**（此前会把无关 provider
  注销或以 opencode-go 工厂误重建）；**L1** `deactivate_opencode_go` 无密钥分支补启动同款
  `tracing::warn!`（D7）；**L2/L4** `authorized_login` 与 `save_link` 失效对应 provider 的
  `model_cache` / `usage_cache`（协议 §8.28「两处缓存」）；`docs/specs/ipc-protocol.md`§8.29
  能力声明由「任意已注册 provider」收窄为「仅 opencode-go」。复审 **VERDICT: PASS**。

## 验证结果（当前树实测，2026-10-08）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo test --workspace -- --skip permission_request_notified_and_timeout_denies` | **851** passed / 0 failed（48 suites） |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `schemas/*.json` | **78** |
| `protocol/methods.json` | **28** 方法 / `protocol_version` **0.18.0** |
| `crates/agentj-protocol` `PROTOCOL_VERSION` | **0.18.0** |
| `./scripts/check-version.sh` | **version-ok（0.29.0 / 0.18.0）** |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |
| `git diff main...HEAD -- schemas/KernelStatusResult.json` | 空（`kernel/status` 逐字不变） |

> 本表于 Phase 8 收尾在最终树重跑；前端侧（SDK 55 · Web 652 · TUI 177 · 构建 clean）见
> `summary_and_next_fe.md`。

## 遗留问题 / 已知限制（后端）

- **时序（设计已记录）**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间
  **进行中的那一轮 run 可能仍使用旧凭据，新凭据从下一轮生效**。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve`
  即远端主机）；非回环 `serve --host` 必须配 `--token`（既有要求，指南重述）。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案；
  两个方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。
- **预存在的 e2e 挂起（非本阶段回归）**：`crates/agentj-cli/tests/e2e_permission.rs` 的
  `permission_request_notified_and_timeout_denies` 在无外部交互时会挂起，全量测试统一以
  `-- --skip permission_request_notified_and_timeout_denies` 运行。
- **真实 Key 人工 E2E 为跟踪项（非 CI）**：步骤见 `summary_and_next_fe.md`「人工验收」小节。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |
| ⑥ | 用量历史（服务端持久化 + 趋势） | `provider/usage` 由按需 TTL 300s 扩为服务端持久化采样 + 历史/趋势查询（协议加性）、Web/TUI 趋势视图与保留策略 | 无（基于 v0.27.0 用量面板；设计需先定采样归属/保留/加性 wire） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- v0.29.0 后端 + 评审修复已落地，全门禁绿（`version-ok`）；自动验收（`jskill-test-acceptance`）
  通过后由验收流程在 `origin` 创建 PR，用户负责合并。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
