# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.4 opencode-go 订阅真实 key 端到端调测适配 + `e2e_permission` 挂起修复（后端部分）**
> ——来源：用户给出测试用订阅 key，要求完成 opencode-go 订阅的端到端调测适配
> （`space-bunny-free` 不存在，实为 `space-bunny` + 轮换的 `*-free`）。
> 后端范围：`agentj-model`（`opencode_go` / `catalog_source` / `responses` / `openai_compat` /
> `anthropic` / `gateway_error` / 新增 `retry`）、`agentj-permission`（`[permission] timeout_ms`）、
> `agentj-cli`（`permission_timeout` + e2e fixture）、`docs/changelog/v0.29.md`、
> `docs/guide/configuration.md`、版本元数据。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.3 → **0.29.4**（纯修复，无新增方法 / schema）。

## v0.29.4（真实 key 端到端调测：4 个上游缺陷 + 1 个测试挂起）✅

### 缺陷与修复

| # | 缺陷 | 根因 | 修复 |
|---|---|---|---|
| 1 | 命名空间 model id 泄漏到上游 wire（三条路由全中） | `OpenCodeGoProvider::route()` 只返回 wire format，`options.model` 原样透传 → 网关收到 `opencode-go/space-bunny` 并回 `unknown model` | `catalog_source::bare_id()`（`pub(crate)`）去掉 `opencode-go/` 前缀后再发送 |
| 2 | `/v1/responses` 输出恒为空 | 解析 `json["delta"]["text"]`，真实 wire 是 `{"type":"response.output_text.delta","delta":"<text>"}`（`delta` 为**字符串**） | 改为「字符串优先、旧对象形状容忍」 |
| 3 | `/v1/responses` 工具参数恒为空 | 解析 `json["delta"]["arguments"]`，真实 wire 是 `"delta":"{\"pattern\":\"*.rs\"}"` | 同上 |
| 4 | `temperature` 被部分模型拒绝（400） | `claude-haiku-5-5`（messages）/ `gpt-6-luna`（responses）不支持该参数 | 新增 `retry::send()`：仅「温度类 400」→ 去字段**重试一次** |
| 5 | `e2e_permission` 无限挂起 → `cargo test --workspace` **永不终止** | 用例把 `session_id` 硬编码为 `"s1"`，而 `session/new` 返回真实 UUID → `session/run` 回 `session error: session 's1' is inactive`，随后裸 `read_line` 在无匹配回复时**永久阻塞** | 从 `session/new` 回复取真实 id + 所有读取改带 20s 截止的通道接收 |

- 附带收敛：三个 transport 各自私有的 `api_error` → `gateway_error::api_error`（`pub(crate)`）；
  同一入口补齐 `429` → `ModelError::RateLimited`（含 `Retry-After`）。
- 新增 `agentj_model::retry`：`pub(crate) async fn send<F>(body, post)` /
  `temperature_unsupported(status, body)` / `without_temperature(body)`。
- 行为变化：`chat()` 的 `429` 现映射为 `RateLimited`（原为 `Api { status: 429, .. }`）；无测试依赖旧行为。
- `responses` 解析器新增容错断言：`responses_tolerates_object_delta_shape` /
  `responses_ignores_reasoning_deltas`；`anthropic_retries_without_temperature_when_rejected`
  用 2 个 mock（400 `.up_to_n_times(1)` → 200）并比对 `received_requests()` 断言重试体无 `temperature`。

### 真实 key 端到端实测（2026-10-16，9 模型全绿）

| 路由 | 模型 | 结果 |
|---|---|---|
| `/v1/chat/completions` | `space-bunny` / `beam-free` / `longcat-2.5-preview-free` | ✅ 文本往返 |
| `/v1/messages` | `minimax-m3` / `qwen3.8-flash` | ✅ 200 往返（**闭环 0.29.3 遗留的「真实 key 200 往返未验证」**） |
| `/v1/messages` | `claude-haiku-5-5` | ✅（走温度降级重试） |
| `/v1/responses` | `grok-4.7` / `gpt-6-luna` / `muse-spark-1.3-contributor` | ✅ 文本往返 |
| 工具调用（三路由） | `glob`；`grok-4.7` 以 `*.txt` 命中唯一标记文件 `./UNIQUE_MARKER_7f3a.txt` | ✅ 参数非空（缺陷 3 的端到端证据） |

- 环境：`/tmp/aj-e2e/config.toml`（`[model] provider = "opencode-go"`、
  `default_model = "opencode-go/space-bunny"`）+ `XDG_DATA_HOME=/tmp/aj-e2e/data`；
  真实 key 经本地 auth store（`$XDG_DATA_HOME/agentj/auth.json`，`type = "api_key"`）注入。
- 事实澄清：`space-bunny-free` **不存在**；免费模型为**轮换制**，`/v1/models` 的 id 会变，
  旧 id 报 `unknown model` 属正常轮换而非缺陷（本期免费位为 `longcat-2.5-preview-free` / `beam-free`）。
- `agentj run` 触发**需审批**工具（`bash`）在非 TTY 下会一直等待审批（无 `--yes` 开关）；
  只读工具（`read`/`glob`/`grep`/`todo`/`ask`）按 `crates/agentj-permission/src/policy.rs:68`
  的 `builtin_default_mode` 自动放行，故工具调用 E2E 用 `glob` 完成。

### 测试速度（用户提出的优化项）

- 根因即缺陷 5：挂起使 `cargo test --workspace` 永不返回，而非单纯「慢」。
- 修复后 `e2e_permission` **0.85s** 完成；全量 `cargo test --workspace` 约 **31s**（含增量编译）。
- 新增 `[permission] timeout_ms`（见下）使审批超时可在 fixture 中降到 800ms，避免再花 60s。

## 已完成（0.29.0 – 0.29.3，详见 `docs/changelog/v0.29.md`）

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。

## 验证结果（0.29.4，2026-10-16）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo test --workspace` | 全绿；`e2e_permission` **0.85s**（修复前无限挂起） |
| `cargo test -p agentj-protocol -- drift` | 通过（无协议漂移） |
| `cargo test -p agentj-model --lib` | 70 passed |
| `./scripts/check-version.sh` | version-ok（0.29.4 / 0.18.0） |
| 真实 key 端到端 | 9 模型 × 3 路由全绿（上表） |

## 遗留问题 / 已知限制（后端）

- **`agentj run` 非 TTY 下遇需审批工具会一直等待**：无自动拒绝、无 `--yes` 开关，也没有上限
  （本次以只读工具绕过，未改该行为）。跟进项：加 `run --yes` 或非 TTY 自动拒绝。
- **`[permission] timeout_ms` 未做下界校验**：`0` 语义为「立即自动拒绝」，当前不视为非法配置。
- **2 条 legacy 路由**：`qwen3.6-plus` / `qwen3.7-max` 按 models.dev 判为 `openai-compat`
  （旧手写表标 `anthropic`），二者均不在官方在售列表内；如实测不符，加回一条种子即可。
- **温度降级只重试一次**：`retry::send` 仅处理「仅 temperature 不支持」的 400；若模型同时拒绝
  其他参数，仍按原错误上抛（不盲目剥参）。
- **凭据时序**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间进行中的那一轮 run
  可能仍使用旧凭据，新凭据从下一轮生效。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 即远端主机）；
  非回环 `serve --host` 必须配 `--token`。
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

- v0.29.4 已落地，全门禁绿（`version-ok` 0.29.4 / 0.18.0）；提交待推送 / PR（用户负责合并）。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝），让 CI / 自动化能跑需审批工具路径。
