# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.5 `serve` / `dashboard` 不再强制预先配置密钥（修复）**
> ——来源：用户报告「启动 `agentj serve` 时不要强制配置 key，该 key 应可以从 Web/TUI 等
> 客户端通过设置配置」。
> 后端范围：`crates/agentj-cli`（`providers.rs` / `main.rs` / `tests/e2e_auth.rs` /
> `tests/e2e_serve.rs`）、`docs/changelog/v0.29.md`、`docs/guide/subscription.md`、
> 版本元数据。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.4 → **0.29.5**（纯修复，无新增方法 / schema）。

## v0.29.5（`serve` 无密钥可启动，密钥由客户端录入）🚧

### 缺陷与根因

`serve` / `dashboard` 的承诺（0.29.0 设计 D2/D3/D7）是「客户端链接 Key 即生效、免重启」，
但启动装配一旦解析不到任何密钥就**直接报错退出**，首次部署必须先跑一次
`agentj auth login` 才能起服务——与「在 Web/TUI 设置里链接」自相矛盾。

| 位置 | 缺陷 |
|---|---|
| `crates/agentj-cli/src/providers.rs` | `build_providers` 在 provider 集合为空时 `return Err("no usable model provider：未解析到任何密钥，见 agentj auth status")` |
| `crates/agentj-cli/src/main.rs` | 该 `Err` 对**除 `run --attach` 外**的所有命令直接 `bail!`，`serve` / `dashboard` 因此进不了「客户端录入密钥」路径 |

### 修复（收窄 fatal 边界）

- `build_providers` **不再**把空集合当错误：改为 `warnings.push(NO_USABLE_PROVIDER.to_string())`
  并返回 `Ok(Assembly { providers: vec![], warnings })`；新增
  `pub const NO_USABLE_PROVIDER: &str` 作为该文案的**唯一来源**（`main` 复用）。
- `main.rs`：把 `inject_subscription` 的 `matches!(Serve | Dashboard)` 判定**上提**为
  `serves_clients`（先于装配），装配后新增：
  `if assembly.providers.is_empty() && !serves_clients { anyhow::bail!("{}", providers::NO_USABLE_PROVIDER); }`
  随后 `let inject_subscription = serves_clients;`。
- 结果语义：
  - `serve` / `dashboard`：启动继续，逐个 provider 打印「无可用密钥…已跳过」+ 一条汇总警告；
    客户端「设置 · 订阅」链接 Key 后当场重建并替换 provider（免重启）。
  - `run`（含无子命令的裸 `agentj`）：**与修复前完全一致**，无密钥仍启动即报错退出非 0
    （其消息仍含 `no usable model provider`），避免无凭据的上游请求。
  - `run --attach`：不变（`attached` 短路，不要求本地 provider）。
  - `agentj auth login`：不受影响（`main.rs:470` 在装配前即 `return`）。

### 测试

| 文件 | 变更 | RED 证据 |
|---|---|---|
| `crates/agentj-cli/src/providers.rs` | `no_usable_provider_is_a_hard_error` → `keyless_provider_set_is_reported_not_fatal`（断言 `Ok` + 空集合 + 警告含 `no usable model provider`） | `called Result::unwrap() on an Err value: "no usable model provider：…"` @ `providers.rs:249` |
| `crates/agentj-cli/tests/e2e_serve.rs` | 新增 `keyless_provider_set_still_boots_serve`：隔离 `XDG_DATA_HOME`/`HOME`、`api_key_env = "AGENTJ_E2E_KEYLESS"` + `env_remove`、`base_url = "https://example.invalid"`（绝不触网）、`serve --port 0` 期望 `read_listening_addr` 成功且进程常驻 | `panicked at crates/agentj-cli/tests/e2e_serve.rs:96:5: server did not print a listening address` |
| `crates/agentj-cli/tests/e2e_auth.rs` | `keyless_real_provider_fails_startup` → `keyless_real_provider_fails_run`（改用 `run "hi"` 保留断言：失败 + 文案 + 未触网 `example.invalid`） | 旧用例断言的正是缺陷行为，改后即绿 |

> 命令提示：`agentj-cli` **没有 lib target**（`cargo test -p agentj-cli --lib` 报
> `no library targets found`）；单测用 `cargo test -p agentj-cli --bin agentj <filter>`。

### 验证结果（0.29.5，2026-10-16）

| 门禁 | 结果 |
|---|---|
| `cargo test -p agentj-cli --bin agentj keyless` | 3 passed |
| `cargo test -p agentj-cli --test e2e_serve` | 8 passed（含新增 1 条），1.81s |
| `cargo test -p agentj-cli --test e2e_auth` | 2 passed |
| `cargo test --workspace` | 全绿（各 suite `0 failed`） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo fmt --all` | clean |
| 真实二进制手测（无密钥，`XDG_DATA_HOME=/tmp/aj-repro/data HOME=/tmp/aj-repro`） | `serve --port 0` 打印 `listening on 127.0.0.1:46377`、进程常驻、`GET /version` → **200**（修复前 `Error: no usable model provider…` / EXIT=1） |
| `./scripts/check-version.sh` | 待重跑（版本已 bump 至 0.29.5） |

## 已完成（0.29.0 – 0.29.4，详见 `docs/changelog/v0.29.md`）

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。
- ✅ 0.29.4：真实 key 端到端调测适配（4 个上游缺陷 + 1 个测试挂起）——合成 agentj-model
  部分：`catalog_source::bare_id()` 去 `opencode-go/` 前缀（三条路由的 model id 泄漏）；
  `responses` 文本/工具 delta 改「字符串优先、旧对象形状容忍」；新增 `retry::send()` 温度类
  400 去参数重试一次；`gateway_error::api_error` 收敛三份私有副本 + `429` → `RateLimited`；
  `[permission] timeout_ms` 使 e2e fixture 审批超时降到 800ms，
  `crates/agentj-cli/tests/e2e_permission.rs` 改用真实 `session_id` + 20s 截止通道
  （修复前该用例**无限挂起**导致 `cargo test --workspace` 永不终止，现 0.85s）。
  真实 key 实测 9 模型 × 3 路由全绿（含 `glob` 工具调用命中唯一标记文件
  `./UNIQUE_MARKER_7f3a.txt`）。

## 遗留问题 / 已知限制（后端）

- **`agentj run` 非 TTY 下遇需审批工具会一直等待**：无自动拒绝、无 `--yes` 开关，也没有上限
  （0.29.4 以只读工具绕过，未改该行为）。跟进项：加 `run --yes` 或非 TTY 自动拒绝。
- **`[permission] timeout_ms` 未做下界校验**：`0` 语义为「立即自动拒绝」，当前不视为非法配置。
- **2 条 legacy 路由**：`qwen3.6-plus` / `qwen3.7-max` 按 models.dev 判为 `openai-compat`
  （旧手写表标 `anthropic`），二者均不在官方在售列表内；如实测不符，加回一条种子即可。
- **温度降级只重试一次**：`retry::send` 仅处理「仅 temperature 不支持」的 400；若模型同时拒绝
  其他参数，仍按原错误上抛（不盲目剥参）。
- **凭据时序**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间进行中的那一轮 run
  可能仍使用旧凭据，新凭据从下一轮生效。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 即远端主机）；
  非回环 `serve --host` 必须配 `--token`。
- **`e2e_cli` / `e2e_permission` 未隔离 `XDG_DATA_HOME`**（仅 `e2e_auth.rs` / `e2e_serve.rs` 隔离）：
  实测在「注入假凭据 + `AGENTJ_OPENCODE_GO_API_KEY`」的环境下仍全绿，非必须但可硬化。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中；两个 auth 方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。

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

- v0.29.5 代码 + 文档 + 版本已落地；剩余：重跑 `./scripts/check-version.sh`
  （0.29.5 / 0.18.0）→ 提交 → 推送**两个远端**（`origin` = GitHub 权威 + `gitcode` 镜像）
  → 在 GitHub 开 PR（用户负责合并）。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝），让 CI / 自动化能跑需审批工具路径；
  以及把 `serve` 的「无 provider」从 WARN 提升为 `/version` 可见的启动警告（目前仅日志）。
