# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.27.0 opencode-go 订阅二期（后端部分）**
> ——来源：设计 `docs/superpowers/specs/2026-10-07-agentj-opencode-go-phase2-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-07-agentj-opencode-go-phase2-plan.md`（T0–T21/T28–T36）。
> 后端范围：`agentj-protocol`（5 方法 + 20 schema）+ `agentj-auth`（oauth/refresh）+
> `agentj-model`（usage/console_catalog）+ `agentj-config`（2 键）+ `agentj-ipc`
> （`ProviderService` 注入）+ `agentj-cli`（设备码登录 / `CliProviderService` / `RefreshingProvider`）。
> `PROTOCOL_VERSION` 0.16.0 → **0.17.0**（21 → **26 方法**，52 → **72 schemas**）；
> 工作区 0.26.0 → **0.27.0**。前端部分见 `summary_and_next_fe.md`。

## v0.27.0（opencode-go 订阅二期，后端部分）✅（本次完成）

- ✅ **协议（`agentj-protocol`）**：新增 `provider` 模块 20 个 wire 类型
  （`ModelListParams/Result`、`ModelEntry`、`ModelSource`、`AuthStatusParams/Result`、
  `CredentialInfo/Kind/Source`、`ProviderUsageParams/Result`、`UsageWindow(+Kind/Status)`、
  `ProviderUsageError`、`AuthDeviceStartParams/Result`、`AuthDevicePollParams/Result`、
  `DeviceAuthState`），全部 snake_case、值枚举带 `#[serde(other)]` + `#[schemars(skip)] Unknown`；
  `PROTOCOL_VERSION` 0.17.0，`protocol/methods.json` 26 方法（5 个新增均为
  `request` + `client_to_server`），schemas 72（新增 20，既有 schema 逐字节未改）。
  **无新错误码**：未知/不支持 provider → `-32602` + `error.data.reason`；
  未注入 handle → `-32603` + `reason=provider_service_unavailable`；上游/凭据失败为结果数据。
- ✅ **`agentj-auth`**：`Credential::key()` 接受 OAuth（access token）；修正一期潜在缺陷
  （`rename_all=snake_case` 会把 `OAuth` 序列化成 `o_auth`，导致 `{"type":"oauth"}` 无法解析）；
  新增 `oauth` 模块（RFC 8628 `device_start`/`device_poll`/`device_refresh`、共享 token 解析、
  `PollOutcome`）与 `refresh::ensure_access`（`auth.json.lock` 单飞、重读后再轮换、
  refresh token 单次使用不重放、60s 安全余量、失败不落盘）；`AuthStore` 可 Clone；
  新增 `reqwest`/`tokio` 依赖（无 `oauth2`/`keyring`/`dirs`/`rpassword`）。
- ✅ **`agentj-model`**：`usage`（防御式 `GET /v1/usage`：只认三窗口、未知键忽略、
  非整数 percent 降级 `None`、`percent==0` 丢弃占位 `resetsAt`、403 不折叠为 401、
  无 key 不发请求）与 `console_catalog`（`GET /api/v2/config` 账号级目录，失败即 Err 以便回退）。
  依赖不变式保持：`agentj-model` 不依赖 `agentj-auth`。
- ✅ **`agentj-config`**：`[model].quota_warn_percent`（默认 80、`get_int` clamp 1..=100）
  与 `[model].console_base_url`（默认 `https://opencode.ai/console`），含 merge 规则。
- ✅ **`agentj-ipc`**：`DispatchError` 公开（pub 字段 + pub 构造器）；新增对象安全
  `ProviderService` trait；`IpcServer.provider_service` 注入字段（默认 `None`）+
  `TcpBindOptions` 贯通；5 个方法 dispatch（未注入统一 `-32603`，参数非法 `-32602`
  且不调用服务，未知方法仍 `-32601`）。**ipc 未新增 auth/model 依赖**。
- ✅ **`agentj-cli`**：`CliProviderService`（usage 300s 缓存 + `refresh` 绕过 + 域错误即结果数据；
  `auth/status` 纯读（不轮换、不联网）；`model/list` 三层 account → live → overlay（60s 复用窗口），
  账号级未命中 overlay 的 id 合成为 `openai-compat` + `verified:false`；设备流按
  `Authorized` 才落盘）；`RefreshingProvider` 装饰器（OAuth 每次 chat 前保鲜，仅密钥变化才重建；
  api_key 走透明静态包装）；`build_providers` 增加 `console_base` 参数；
  `agentj auth login --device`（与 `--api-key` 互斥，deadline 由 `expires_in` 推导，
  `slow_down` 递增间隔，失败不落盘）；`serve`/`dashboard` 注入 handle（`run` 不注入）。

## 验证结果（全量门禁清扫，2026-10-07）

> 本表由编排器在本阶段最终树上执行；后端门禁如下，前端门禁见 `summary_and_next_fe.md`。
> `cargo test --workspace` 以 `-- --skip permission_request_notified_and_timeout_denies`
> 运行（见「遗留问题」）。

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo check --workspace --locked --quiet` | OK（lockfile fresh） |
| `cargo test --workspace -- --skip permission_request_notified_and_timeout_denies` | **790 passed / 0 failed** |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `./scripts/check-version.sh` | version-ok（**0.27.0** / `PROTOCOL_VERSION` **0.17.0**） |
| `cargo run -p agentj-protocol --bin gen_schemas` | 72 schemas |

## 代码评审（Phase 5，`ajteam-task-reviewer` 分组只读审查）

- **后端（T0–T21/T28–T32）：VERDICT PASS**。确认：单次 refresh token 纪律
  （锁内重读、失败不落盘、空 refresh 先拒绝）、依赖不变式、加性纪律
  （既有 21 方法逐字节未改）、usage 防御式解析、三层目录与 `openai-compat` 合成、
  `auth/status` 不联网、结果不含任何完整密钥/token。
- 评审提出 3 项偏差，其中 2 项为计划口径差异（`ModelSource::Unknown` 规范化为
  `"unknown"` 而非回填原串；T19 计划注释提到的目录摘要），1 项为纵深防御建议
  （空 access token 可能形成空 Bearer）。
- **已修复**（commit `bc19b2b`）：`CliProviderService::key()` 过滤空 key（杜绝空 Bearer）；
  非 `--offline` 的设备码登录补打模型目录摘要。
- **前端审查 VERDICT FAIL → 已修复**（commit `99385c6`，见 `_fe.md`）。

## 遗留问题 / 已知限制（后端）

- **预存在的 e2e 挂起（非本阶段回归）**：`crates/agentj-cli/tests/e2e_permission.rs`
  的 `permission_request_notified_and_timeout_denies` 在无外部交互时会挂起，
  全量测试统一以 `-- --skip permission_request_notified_and_timeout_denies` 运行。
- **锁超时窗口（设计已记录）**：`auth.json.lock` 5s 超时后仍可能由用户手动并发触发刷新；
  锁文件非进程崩溃自清理，崩溃残留会在 5s 后报错而非自动接管。
- **usage 上游形状不稳定（设计已记录）**：`GET /v1/usage` 未官方文档化、曾上线一小时内改形状、
  无 cache 头；面板对不可识别响应用 `error.kind="unavailable"` 而不是编造数据。
- **`auth/devicePoll` 的 interval 常量 5s（设计已记录）**：服务端不维护会话状态，
  重复 `slow_down` 每次只报到 10s，累加由客户端（CLI/Web/TUI）自行记账。
- **`agentj-auth` 由零依赖小 crate 变为带 HTTP 依赖**（设计已记录的架构成本）：
  token 生命周期与凭据同域，代价是与 `agentj-model` 各持一套 HTTP 栈。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **v0.27.0 opencode-go 二期（后端部分）已落地**，全量门禁清扫通过；待 PR 由用户合并 main。
- 后端候选：上表 ①②④；或为 CLI `run` 补 `--provider` / `--model` flag；
  或订阅用量面板的服务端持久化/历史趋势。
- 前端候选见 `summary_and_next_fe.md`。
