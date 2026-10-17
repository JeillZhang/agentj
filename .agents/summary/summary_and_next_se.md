# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.7 上下文预算（修复）**
> ——来源：用户报告 `-32603: agent error: API error (400): invalid_request_error: Upstream
> request failed: [context_length_exceeded] This model's maximum context length is 1048576
> tokens. However, your messages resulted in 3904559 tokens.`
> 后端范围：`crates/agentj-core/src/{budget.rs,module.rs,turn_runner.rs}`、
> `crates/agentj-model/src/gateway_error.rs`、`crates/agentj-config/src/{lib.rs,merge.rs}`。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.6 → **0.29.7**（纯修复，无新增方法 / schema）。

## v0.29.7（请求 token 预算：裁剪 + 超限收紧重试）🚧

### 缺陷与根因

内核此前**没有任何上下文管理**，长会话把整段历史与每轮工具结果原样发给上游，直到网关用
400 拒绝。用户看到的是一条无法自助解决的 `-32603`。

| 位置 | 缺陷 |
|---|---|
| `crates/agentj-context/src/lib.rs` | `ContextPipeline::assemble` 是纯函数，返回**全量**历史（system + history + tool_results），无任何预算/裁剪 |
| `crates/agentj-core/src/turn_runner.rs` | `assemble` 后 `for _iter in 0..max_iter`（`agent.max_tool_iterations`，最多 50 轮）每轮 `pipeline.reflux(...)` 追加工具结果，**只增不减**，然后整段 `provider.chat_stream(messages.clone(), …)`（`turn_runner.rs:238` 是全仓**唯一**的生产发送点，已核对：其余 `.chat_stream(` 命中都是装饰器透传或测试） |
| `crates/agentj-core/src/error.rs:59` | `ModelError::TokenLimitExceeded { used, max }` 自 v0.1 起**从未被构造**（死代码）——本缺陷本应走的类型化错误 |
| `crates/agentj-core/src/module.rs:534` / `crates/agentj-session/src/{lib.rs:99,file_store.rs:218}` | `SessionStore::estimate_tokens` 已实现（`chars/4`）但**无生产调用方**（仅自身测试），同样是死代码 |
| `crates/agentj-model/src/gateway_error.rs` | 共用 `api_error` 一律返回 `Api { status, message }`（0.29.4 起的温度重试依赖该分支），上下文超限只作为文案上抛 |
| `crates/agentj-config` | `[model]` 无预算键；`ModelSection` 字面量有 4 处（`lib.rs` ×2、`merge.rs` 测试 ×2），新增字段需同步 |

### 修复

- 新增 `crates/agentj-core/src/budget.rs`（`mod budget;` 已注册）：
  `DEFAULT_CONTEXT_TOKENS = 128_000` / `MIN_CONTEXT_TOKENS = 1_000` /
  `MAX_CONTEXT_TOKENS = 10_000_000` / `MAX_OVERFLOW_RETRIES = 2`；
  `estimate_tokens`（ASCII 约 4 字符/token，非 ASCII **按 1 字符/token**，中文宁可高估）、
  `message_tokens`（含 `tool_calls` 名称与参数 JSON）、`request_tokens`、`clamp_budget`、
  `retry_budget(budget, upstream_max) -> Option<usize>`（`0 < upstream_max × 0.9 < budget` 时取
  上游窗口 × 0.9，否则取本预算 × 0.9，再 `clamp_budget`；结果**不小于**当前预算时返回 `None`，
  即「无法再减就不重试」）、`fit_messages(messages, budget)`。
- `fit_messages` 语义：保留 `Role::System` 与**最新**消息；逐条正文上限 `budget/4`，超长正文
  保留头尾并插入 `…[已截断 N 字符]…`（system 永不截断）；正文为空但工具调用参数超大的消息，
  整段参数替换为 `{"_truncated": …}`（否则单条仍可远超预算）；前缀裁剪**不会留下缺少对应
  assistant 调用的 `Role::Tool` 结果**（上游会拒绝不成对的 tool 消息）。
- `ContextPipeline`（`crates/agentj-core/src/module.rs`）新增**默认方法**
  `fn fit(&self, messages: Vec<ChatMessage>, budget_tokens: usize) -> Vec<ChatMessage>`，
  默认委托 `budget::fit_messages` → 所有实现（含 `agentj-ipc` / e2e 的 stub 管线）默认受约束，
  无需逐个改动。
- `turn_runner`：从 `ctx.config.get_int("model.max_context_tokens")` 取预算（缺省
  `DEFAULT_CONTEXT_TOKENS`，`clamp_budget` 钳制）；每次发送前 `pipeline.fit(messages.clone(),
  context_budget)`；`provider.chat_stream` 返回 `Err(ModelError::TokenLimitExceeded { max, .. })`
  时用 `budget::retry_budget(context_budget, max)` 求下一预算：`Some(next)` 且
  `overflow_retries < MAX_OVERFLOW_RETRIES` → 递增计数、`context_budget = next`、`continue 'outer`
  重发；`None`（无法再减）或次数用尽 → 照旧 `route(err.to_string())` 上抛。
- `crates/agentj-model/src/gateway_error.rs`：新增
  `pub fn context_overflow(status: u16, body: &str) -> Option<(usize, usize)>`（仅 400/413；
  措辞命中 `context_length_exceeded` / `maximum context length` /
  `reduce the length of the messages`——**刻意不含**宽泛的 `context window`，否则 `max_tokens`
  超窗的参数校验错误会被误判重试；`number_after` 解析 `maximum context length is 1048576` 与
  `resulted in 3904559`，容忍 `1,048,576` 千位分隔，解析不到记 0）与私有 `number_after`；`api_error` 命中即返回
  `ModelError::TokenLimitExceeded { used, max }`，用户可读文案变为
  `context length exceeded: 3904559/1048576 tokens`。三种 wire 共用同一映射。
- `crates/agentj-config`：`ModelSection.max_context_tokens: i64`
  （`#[serde(default = "default_max_context_tokens")]`，`default_max_context_tokens() = 128_000`）、
  `get_int("model.max_context_tokens")` 钳制 `1_000..=10_000_000`、`merge_config` 新增
  「文件值 ≠ 默认值即覆盖 base」规则（与 `quota_warn_percent` 同形），并同步 `lib.rs` ×2 与
  `merge.rs` 测试 ×2 的 `ModelSection` 字面量。
- 代码评审（`del_mvb9gank_sjdk`）后回填：`retry_budget` 改 `Option` 并**保证严格变小**（否则
  128k 预算遇上 1M 窗口时两次重试发的是同一份请求）、超长工具参数替换、`fit` 文档不再承诺
  「绝不超过窗口」、`gateway_error` 去掉 `context window` 措辞 + 千位分隔解析、测试改为断言
  实际请求的 **token 估算**并新增重试耗尽用例。
- **`e2e_ws` 端口竞态修复**（`crates/agentj-cli/tests/e2e_ws.rs`，非本缺陷引入）：改为
  `serve --port 0` + 从 stdout 读回绑定地址（同 `e2e_serve.rs`），并行复现 1/3 → 3/3 通过。
- 文档：`docs/guide/configuration.md`（`[model] max_context_tokens`）、`AGENTS.md`
  （agentj-core `budget` / agentj-config 键 / agentj-model `gateway_error` + `retry` /
  agentj-context 说明）、`docs/changelog/v0.29.md` 新增 0.29.7 小节、索引行。

### 测试（RED → GREEN）

| 测试 | 文件 | RED 证据 |
|---|---|---|
| `provider_request_stays_within_the_configured_context_budget` | `crates/agentj-core/src/turn_runner.rs` | `request carried 72475 chars, budget is 2000 tokens`（provider 实际收到的字符数远超预算） |
| `context_overflow_retries_with_a_smaller_request` | 同上 | `the overflow must trigger exactly one retry: left 1, right 2`（上游 400 从不重试） |
| `context_overflow_maps_to_the_typed_error` | `crates/agentj-model/src/gateway_error.rs` | 修前只得到 `API error (400): …`（断言期望 `context length exceeded: 3904559/1048576 tokens`） |
| `unparsable_overflow_reports_unknown_sizes` | 同上 | 同上（期望 `TokenLimitExceeded { used: 0, max: 0 }`） |
| `unrelated_400_stays_a_plain_api_error` | 同上 | 始终通过（守护 `retry.rs` 的温度重试分支不被抢走） |
| `context_overflow_gives_up_after_the_retry_budget` | `crates/agentj-core/src/turn_runner.rs` | 持续超限 → 共 3 次请求且逐次严格变小，最终上抛错误（修前只 1 次请求、无重试） |
| `retry_budget_always_shrinks_or_gives_up` | `crates/agentj-core/src/budget.rs` | 修前 `retry_budget(128_000, 1_048_576) == 128_000`（两次重试发同一份请求） |
| `truncate_shrinks_a_tool_call_payload` | 同上 | 修前正文为空的超大 tool-call 消息原样保留 |
| `digit_grouped_sizes_are_parsed` | `crates/agentj-model/src/gateway_error.rs` | 修前 `1,048,576` 只解析出 `1` |
| `context_window_validation_error_stays_a_plain_api_error` | 同上 | 修前 `max_tokens` 超窗文案被误判为可裁剪重试 |
| `budget.rs` 5 条 | `crates/agentj-core/src/budget.rs` | 与实现同批新增：`estimate_tokens_charges_cjk_per_character`、`fit_messages_keeps_system_and_newest_messages`、`fit_messages_truncates_a_single_oversized_body`、`fit_messages_never_orphans_a_tool_result`、`retry_budget_uses_the_upstream_limit_with_headroom` |
| `max_context_tokens_defaults_overrides_and_clamps` | `crates/agentj-config/src/lib.rs` | 首版**失败**（`Some(128000) != Some(32000)`）→ 暴露 `merge_config` 缺规则，补规则后通过（测试驱动出的真实集成缺口） |

### 验证结果（0.29.7，2026-10-16）

| 门禁 | 结果 |
|---|---|
| `cargo test -p agentj-core --lib -- budget turn_runner` | 28 passed（budget 7 条单测 + 3 条集成用例） |
| `cargo test -p agentj-model --lib gateway_error` | 7 passed |
| `cargo test -p agentj-config --lib` | 27 passed |
| `cargo test --workspace --no-fail-fast` | 48 套件 / **882 passed, 0 failed**（866 → 882，+16 条新增） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo fmt --all` / `-p agentj-protocol -- drift` | clean / 2 passed |
| Web / SDK / TUI | 53 文件 / 655 passed · 6 文件 / 55 passed · 177 passed |
| `./scripts/check-version.sh` | `version-ok`（0.29.7 / 0.18.0） |
| 真实 key E2E（opencode-go `space-bunny`） | `max_context_tokens = 4000` + 30k 字符提示 → 退出码 0、回复 `pong`（裁剪后可达，头尾保留让尾部长提示仍被读到）；不设该键（默认 128k）→ 退出码 0、回复 `pong` |

## 已完成（0.29.0 – 0.29.6，详见 `docs/changelog/v0.29.md`）

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。
- ✅ 0.29.4：真实 key 端到端调测适配（4 个上游缺陷 + 1 个测试挂起）——`catalog_source::bare_id()`
  去 `opencode-go/` 前缀；`responses` 文本/工具 delta 改「字符串优先、旧对象形状容忍」；
  新增 `retry::send()` 温度类 400 去参数重试一次；`gateway_error::api_error` 收敛三份私有副本
  + `429` → `RateLimited`；`[permission] timeout_ms` 使 e2e 审批超时降到 800ms，
  `e2e_permission.rs` 改用真实 `session_id` + 20s 截止通道（修复前**无限挂起**，现 0.85s）。
  真实 key 实测 9 模型 × 3 路由全绿（含 `glob` 命中唯一标记文件 `./UNIQUE_MARKER_7f3a.txt`）。
- ✅ 0.29.5：`serve` / `dashboard` 不再强制预先配置密钥（`providers::NO_USABLE_PROVIDER`
  只作为警告，`main.rs` 仅对无可录入客户端凭据的命令 fatal；`run` 行为不变）。
- ✅ 0.29.6：断连重连后的 `SessionBusy`——`settle_owner_pending` 在**每条**连接收尾路径
  （EOF / 传输错误 / shutdown）结算该 owner 的 question 与 permission 待决项（reason
  `disconnected`），`ConnectionRegistry::clear_owner` 在排空**前**撤销归属（排空期间新审批
  立即被拒），`attach_permission_notifier` 无 owner 可达即刻拒绝；Web 重连即重新
  `session/subscribe` + `SessionBusy` 友好文案；`pump_requests` 借用 `&mut Box<dyn Connection>`
  使连接活过排空（否则写方向被 shutdown，响应丢失）。

## 遗留问题 / 已知限制（后端）

- **预算为近似估算**：`estimate_tokens` 用「ASCII / 4 + 非 ASCII × 1」的字符启发式，非真实
  tokenizer；目录里**没有 per-model 上下文窗口元数据**（`schemas/ModelEntry.json` 仅
  format/id/verified），因此默认 128k 是保守假设，真窗口更小的模型仍可能被上游拒绝一次
  （此时由 `retry_budget` 按上游窗口收紧重试，最多 2 次）。
- **裁剪即丢失**：超预算时最老的会话历史与（同轮内）较早的工具结果会被丢弃，正文超长时中段
  被截断；仅在日志/行为上可观测，UI 无「已裁剪」提示。
- **`agentj run` 非 TTY 下遇需审批工具会一直等待**：无自动拒绝、无 `--yes` 开关，也没有上限
  （0.29.4 以只读工具绕过，未改该行为）。跟进项：加 `run --yes` 或非 TTY 自动拒绝。
- **`[permission] timeout_ms` 未做下界校验**：`0` 语义为「立即自动拒绝」，当前不视为非法配置。
- ~~`e2e_ws` 端口竞态~~：0.29.7 已修复（`serve --port 0` + 读回绑定地址；并行复现 1/3 → 3/3）。
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

- v0.29.7 代码 + 测试已落地，版本元数据与文档已收尾（`Cargo.lock` / desktop `Cargo.lock` 已刷到
  0.29.7，`./scripts/check-version.sh` 已过，代码评审（`del_mvb9gank_sjdk`）结论已回填且
  F1–F6 全部处理）；剩余：提交 → 推送**两个远端**（本机 `github` = GitHub 权威、
  `origin` = GitCode 镜像；注意 AGENTS.md 的远端命名与本机相反）→ 在 GitHub 开 PR（用户负责合并）。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝），让 CI / 自动化能跑需审批工具路径；
  把 `serve` 的「无 provider」从 WARN 提升为 `/version` 可见的启动警告（目前仅日志）；
  把「会话已裁剪 N 条消息」作为可观测事件（`$/progress` 或日志）暴露。
