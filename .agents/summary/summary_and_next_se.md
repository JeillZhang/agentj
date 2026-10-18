# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**指标导出扩展（代理头解析 + 计数持久化）** —— 分支 `feature/metrics-export-extension`。
> 改 `crates/agentj-ipc`（新增 `client_ip.rs` / `metrics_state.rs`，扩展 `login_limiter.rs` /
> `web.rs` / `tcp.rs`）+ `crates/agentj-config`（新增 `[web]`）+ `crates/agentj-cli`（serve 装配与
> 优雅落盘）；协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 0.18.0 / 28 方法 / 78 schemas；工作区仍
> **0.1.0**）。下方 0.29.x / BUG-017 等前序章节为**上一阶段记录**（归档）。

## 指标导出扩展：代理头解析 + 计数持久化 ✅

分支 `feature/metrics-export-extension`。无 wire 变更（无新方法 / 无 schema / 无 `PROTOCOL_VERSION` 变更）。

### 交付

- **客户端 IP 派生**（`crates/agentj-ipc/src/client_ip.rs`，新增，纯函数、无 IO）：
  `resolve_client_ip(peer, headers, trust) -> ClientIp { ip, source: IpSource }`——仅在**信任生效**
  时读 `X-Forwarded-For` / `X-Real-IP`，而信任生效的唯一前提是**直接 TCP peer 为回环**；
  `X-Forwarded-For` 从右向左扫描、跳过回环跳，取第一个合法非回环项（最右 = 代理实际观测值，
  客户端伪造的左前缀无效）；`X-Real-IP` 为回退；否则回退原始 peer。
  `IpSource::{Peer, XForwardedFor, XRealIp}::label()` → `peer` / `forwarded` / `real_ip`。
- **计数持久化**（`crates/agentj-ipc/src/metrics_state.rs`，新增）：`version: 1` 的 JSON 快照 +
  `state_path_in(dir, port)` = `metrics-state-<port>.json`；原子写（目录 `0700` / 文件 `0600` /
  同目录 tmp + `rename`，**每条失败路径都清理 tmp**）；`read` 缺文件 → `Ok(None)`，损坏 / 超
  1 MiB / `version` 不符 → `Err`；`restore_seeds()` 做钳制换算（锁 ≤ 900 s、idle 年龄 ≤ 1800 s、
  超 TTL 丢弃、按 `max_tracked` 截断；锁过期仍保留失败数）。
- **限流器**（`crates/agentj-ipc/src/login_limiter.rs`）：新增 `to_state` / `restore_from` /
  `dirty` / `mark_clean`；**既有公开签名一律未动**。
- **Web 面**（`crates/agentj-ipc/src/web.rs`）：`WebConfig` 增信任开关 / 状态路径 / 5 s debounce
  flush 任务（**仅 dirty 才写**）+ `restore_metrics_state()` + `flush_metrics_state()` /
  `stop_metrics_flush()`（供优雅关闭）；`prometheus_text` 增 6 个**加性**指标——
  `agentj_web_client_ip_source_total{source=peer|forwarded|real_ip}` /
  `agentj_web_proxy_header_invalid_total` / `agentj_web_metrics_state_restored` /
  `agentj_web_metrics_state_entries_restored` / `agentj_web_metrics_state_write_failures_total` /
  `agentj_web_metrics_state_last_flush_unix`；既有指标名与 `PER_IP_SERIES_LIMIT = 50` 不变。
- **TCP 绑定**（`crates/agentj-ipc/src/tcp.rs`）：`TcpBindOptions` 增 `trust_proxy_headers` +
  `metrics_state_dir`；状态路径按**实际绑定端口**（`local_addr()`）拼出，故 `--port 0` 亦正确。
- **配置 / CLI**：`crates/agentj-config` 新增 `[web]`（`trust_proxy_headers` 默认 `false` /
  `state_dir`）→ `AgentJConfig.web`；`crates/agentj-cli/src/main.rs` 装配进 `serve`——状态目录
  依次取 `[web] state_dir` → `$XDG_DATA_HOME/agentj` → `$HOME/.local/share/agentj`，全空则仅内存
  并启动警告；非回环绑定 + 信任开启另发启动警告；**优雅关闭先落盘再停 flush 任务，且 serve 循环
  返回错误时也不跳过**。
- **安全红线（永久不变量）**：派生 IP **只**用于登录限流键、per-IP 指标序列与日志/指标字段；
  `auth_required` 始终按**原始 TCP peer** 判定，故转发头**永远不能**换取 `/metrics` 的回环豁免
  （回归测试 `forwarded_headers_cannot_buy_loopback_exemption` 钉住）。
- **口径**：持久化为**每进程、每端口一个文件**；同机多实例共享聚合为**非目标**（Prometheus 侧
  `sum by`）。

### 验证

| 门禁 | 结果 |
|------|------|
| `cargo test --workspace` | **980 passed** / 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `cargo clippy -p agentj-ipc -- -D warnings` | 0 warning |
| `cargo fmt --all --check` | FMT-OK |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `./scripts/check-version.sh` | `version-ok`（0.1.0 / 0.18.0，本阶段不 bump） |
| `cargo test -p agentj-ipc --lib` | 239 passed |
| `cargo test -p agentj-cli --bin agentj` | 110 passed |
| `cargo test -p agentj-cli --test e2e_serve` | 9 passed |

### 真实提交（`9f2bc15..HEAD`，最旧 → 最新）

- `1ec9e5b` feat(config): add [web] section with trust_proxy_headers and state_dir
- `8986885` feat(ipc): derive the client IP from loopback-gated forwarded headers
- `55e6d29` feat(ipc): add atomic metrics state snapshot with clamped clock conversion
- `55d63de` feat(ipc): snapshot and restore login limiter state with dirty tracking
- `5b6f16a` fix(ipc): clean up the metrics state tmp file on every write failure
- `3ae4f87` feat(ipc): restore and debounce-flush login metrics state behind WebConfig
- `b4c77eb` feat(ipc): throttle on the derived client IP while auth keeps the raw peer
- `853a760` feat(ipc): resolve the metrics state path from the bound port
- `54db01c` refactor(ipc): make the metrics state flush and stop API public for graceful shutdown
- `496a0af` feat(cli): wire [web] proxy trust and metrics state dir into serve
- `ca52ff5` test(cli): prove login counters survive a serve restart
- `e36db88` docs: document proxy-header trust and persisted login metrics
- `ac44b66` refactor(ipc): drop the staging dead_code allowances now that the metrics wiring is live
- `c6fbf16` refactor(cli): always tear down the serve loop and trim serve_tcp
- `f5955a1` test(cli): assert exact metric lines in the restart e2e
- `2eaf0ec` refactor(ipc): render client IP source labels from IpSource::label
- `d888b24` docs: record the per-IP last-seen metric and snapshot-only history
- `b8b5493` test(cli): translate the last Chinese comment fragment in the serve e2e
- `d45e30c` test(cli): cover the error-path teardown of the serve loop
- `965dcf4` test(ipc): assert exact metric lines in the web unit tests

## 已完成（0.29.0 – 0.29.12 + BUG-017/018，详见 `docs/changelog/v0.1.md`）

- ✅ 0.29.8：工具往返落库（每轮 `assistant{tool_calls}` + `tool` 结果写入会话，「继续」不再失忆）
  + 截断流（无 `Done` 的纯文本轮）重放 ≤2 次后如实报错；前端输入框 chip 跟随实际选择。
- ✅ 0.29.9：工具轮次用尽后追加一趟「无工具收尾」（`conclude_without_tools`），
  `[agent] max_tool_iterations` 从死配置变为可配（钳制 `1..=1000`）。
- ✅ 0.29.10：`text_tool_call` 把模型写成纯文本方言（`<rfl:parameter>` / `<tool_call>`）的
  工具调用还原成真正的调用；无法还原时纠偏 2 次后如实报错，而不是当结论。
- ✅ 0.29.11：Web 模型选择器不再提供虚构 `deepseek-chat` 占位（前端，详见 `_fe.md`）。

- ✅ 0.29.7：请求 token 预算（`budget.rs` + `ContextPipeline::fit` + 超限收紧重试）——长会话
  不再把整段历史原样发给上游后才被 400 拒绝；`[model] max_context_tokens` 可配（默认 128k）。

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

- **远端命名修正（本工作副本实测）**：`github` = `git@github.com:JeillZhang/agentj.git`（**权威**）、
  `origin` = `git@gitcode.com:jeill/agentj.git`（GitCode **镜像**）——与 `AGENTS.md`「Remote & Merge
  Policy」的命名**相反**；feature 分支须推 `github` + `origin` 两个远端，PR 开在 `github`（待实施 ⑳
  的主题，本阶段不改文档，仅记录实测事实）。
- **文档滞后（本阶段有意未动 `AGENTS.md`）**：`AGENTS.md` 的 `agentj-config` 条目尚未列新增 `[web]`
  配置节，`agentj-ipc` 条目尚未提 `client_ip` / `metrics_state` 两个新模块（本阶段文档落在
  `docs/specs/ipc-protocol.md` + `docs/guide/http-and-auth.md` + `docs/changelog/v0.1.md`）；下次
  结构性触碰时补齐。
- **状态目录默认值文档不完整（实测）**：`docs/specs/ipc-protocol.md:58` 与
  `docs/guide/http-and-auth.md:63,82` 只写默认 `$XDG_DATA_HOME/agentj`，而
  `crates/agentj-cli/src/main.rs:146 fn metrics_state_dir_from` 的实装回退链为 `[web] state_dir`
  → `$XDG_DATA_HOME/agentj` → `$HOME/.local/share/agentj`（代码自身的启动警告亦写「set
  XDG_DATA_HOME, HOME or [web] state_dir」）；下次触碰该文档时补 HOME 回退。
- **错误路径收尾的测试只覆盖到 helper 内部**（终审 MEDIUM）：`crates/agentj-cli/src/main.rs` 的
  新测试 `shutdown_flushes_metrics_state_even_when_the_serve_loop_failed` 直接调用私有
  `shutdown_tcp_server`，因此**能**钉住「`run_result?` 不得早于 `flush_metrics_state()`」这一
  顺序回归（把 `?` 前移即 RED）；但它**不**驱动 `serve_tcp`，故若回归重新在调用点写回
  `tcp.run().await?`（`main.rs:447` 一类），该测试仍会通过。要覆盖这一层需让 `serve_tcp` 以
  可失败的 `run()` 端到端跑通（当前无该夹具），故仅记录、不再加测。
- **`[web] state_dir = ""` 未做配置校验**（验收实测）：`metrics_state_dir_from`
  （`crates/agentj-cli/src/main.rs:146`）对空 `$XDG_DATA_HOME` / `$HOME` 有过滤，但**显式空**
  `state_dir` 会返回 `Some(PathBuf::from(""))`；后果为每次 debounce flush 均失败（
  `warning: cannot persist login metrics state to metrics-state-<port>.json (io error: No such
  file or directory)` + `agentj_web_metrics_state_write_failures_total` 增长），不写文件、
  不改 cwd 权限、不崩溃。字面满足验收 AC（`<dir>` 即 `state_dir`），故未计 Bug；跟进项：
  加载配置时把空串归为 `None` 并复用「无状态目录」启动警告。
- **flush 与变更之间存在理论竞态（未复现）**：`flush_metrics_state`
  （`crates/agentj-ipc/src/web.rs:412-424`）先快照、后**无条件** `mark_clean()`；若某次限流变更
  恰好落在两者之间，该变更会被当作「已持久化」。可由下一次变更或关机时的无条件 flush 补回，
  仅「在该窗口内 SIGKILL」才真正丢一次增量；因无法确定性复现，故仅记录。
- **预算为近似估算**：`estimate_tokens` 用「ASCII / 4 + 非 ASCII × 1」的字符启发式，非真实
  tokenizer；目录里**没有 per-model 上下文窗口元数据**（`schemas/ModelEntry.json` 仅
  format/id/verified），因此默认 128k 是保守假设，真窗口更小的模型仍可能被上游拒绝一次
  （此时由 `retry_budget` 按上游窗口收紧重试，最多 2 次）。
- **裁剪即丢失**：超预算时最老的会话历史与（同轮内）较早的工具结果会被丢弃，正文超长时中段
  被截断；仅在日志/行为上可观测，UI 无「已裁剪」提示。
- **文本方言还原只按参数名推断**（0.29.10）：泄漏模型已确认为 **`beam-free`**（用户
  2026-10-17），但泄漏随机、无法稳定复现（前缀回放 / 完整 harness run 都返回原生
  `tool_calls`），因此还原能力靠**参数名**推断工具（`command`→bash 等）；多参数调用（如
  write 的 path+content）与未知参数名仍走「纠偏重发 → 如实报错」。
- **grep 回退不读 `.gitignore`**（0.29.12）：无 `rg` 时的进程内检索只跳过固定目录
  （`.git`/`target`/`node_modules` 等），不解析 `.gitignore`；命中结果仍受 2000 条上限约束。
- **`glob` 只按 basename 匹配**（0.29.12 实测发现）：`find . -name "crates/agentj-core/src/*.rs"`
  这类**路径型 pattern 恒空**（`-name` 只匹配文件名），模型因此拿到空的 `glob` 结果后改用
  `bash` 绕路；属真实缺陷，未在本轮修改（见待实施 ⑨）。
- **`ask` 问题无超时上限**（0.29.9 实测发现）：长任务里模型反问方向时挂起
  `question/respond`；探测客户端未应答就一直等到被手动杀掉（服务端无默认 `timeout_s`）。
  UI 有 `AskQuestionCard` 所以正常路径可见，但「没人回答 = 本轮永久挂起」值得跟进
  （默认超时或 idle 提示）。
- **快照体积随工具往返增长**（0.29.8 起）：工具结果现在会落库，实测一次只读探测里的 `glob`
  结果就有 **7,963,644 字符**并原样进快照；发送侧虽有 `fit_messages` 预算，**存储侧没有上限**。
  跟进项：落库前按工具结果设上限（或压缩为摘要 + 提示「已截断」）。
- **生效模型未回显**：`session/run` 结果不返回实际使用的 `provider`/`model`，trajectory 里也
  不记录，因此排障时无法从快照/轨迹反查「这句残句到底是哪个模型吐的」。已靠前端在
  `session/run` 里下发 `provider`/`model` 覆盖 + 标签跟随选择缓解，后端回显仍是加性 wire 候选。
- **`agentj run` 非 TTY 下遇需审批工具会一直等待**：无自动拒绝、无 `--yes` 开关，也没有上限
  （0.29.4 以只读工具绕过，未改该行为；0.29.8 真实 key 验证时仍观察到
  `permission approval timed out … timeout_ms=60000`）。跟进项：加 `run --yes` 或非 TTY 自动拒绝。
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
- **进程型工具报错里的 `name` 变成程序名**（本轮引入）：`NotFound` / spawn 失败时 executor 把
  `ProgramMissing.name` 填成**程序**（`crates/agentj-tools/src/executor.rs:27-30`：bash → `sh`、
  glob → `find`、grep → `rg`），而改动前填的是工具名（`bash` / `glob` / `grep`）；管道读失败则
  是 `"child"`（`executor.rs:71,78`）。错误码与变体未变（无 wire 影响），但与设计 G4「错误面
  不变」有偏差，属可选跟进。
- **`ToolExecutor::run_blocking` 只能在阻塞线程里调**：它自建 runtime 后 `block_on`
  （`crates/agentj-tools/src/executor.rs:220-237`），当前唯一生产调用方是 `spawn_blocking`
  （`crates/agentj-core/src/module.rs:451`，线程未设 `EnterRuntime::Entered` 故安全）；若将来在
  异步上下文（多线程 worker / `block_in_place`）直接调 `Tool::execute` 会 panic。
- **MCP 子进程链路仍未接线**：`crates/agentj-mcp/src/connection_manager.rs:222` 仍是裸
  `tokio::spawn` 管理子进程，未走 `ToolExecutor` 的进程组 / 超时 / drop 回收；MCP server 挂起
  同样不会被杀整组（后续跟进）。
- **bash 默认 120s 对冷编译偏紧**：Rust 全量 `cargo build` / 大测试套件可能超过 120s；模型可用
  入参 `timeout_ms`（上限 600s）覆盖，默认值是否上调待观察。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中；两个 auth 方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |
| ⑥ | 用量历史（服务端持久化 + 趋势） | `provider/usage` 由按需 TTL 300s 扩为服务端持久化采样 + 历史/趋势查询（协议加性）、Web/TUI 趋势视图与保留策略 | 无（基于 v0.27.0 用量面板；设计需先定采样归属/保留/加性 wire） | 待实施 |
| ⑦ | 工具结果落库上限 | 落库前对单个工具结果设上限（或摘要 + 「已截断」标记），避免 7.9MB 级 `glob` 结果撑爆快照 | 0.29.8 已落库工具往返（存储侧尚无上限） | 待实施 |
| ⑧ | `session/run` 回显生效模型 | 结果/trajectory 记录实际使用的 `provider`/`model`（加性 wire），便于排障与前端一致性校验 | 0.29.8 前端已按选择下发并显示 | 待实施 |
| ⑨ | `glob` 路径语义 | 让 `glob` 支持路径型 pattern（`crates/*/src/*.rs`）与 `**`，而不是只按 basename 匹配（当前路径型 pattern 恒空） | 0.29.12 已引入 `globset`（grep 回退依赖） | 待实施 |
| ⑩ | 【债】IPC 协议层拆分 | `crates/agentj-ipc/src/lib.rs`（实测 **7,339 行 / 295,749 B**）按 dispatch 域拆为 `dispatch_session` / `dispatch_permission` / `dispatch_question` / `dispatch_file` / `dispatch_provider` 等子模块；`impl IpcServer`（行 486–1931，约 **1,445 行**、30+ 方法）切分为多个 impl 块或独立 handler | 无（纯重构，wire 面不变） | 待实施 |
| ⑪ | 【债】超长函数治理 | 实测 lib 代码 831 个函数中 **26 个 >50 行**（违反 `AGENTS.md` 「function ≤50 lines」）：`crates/agentj-cli/src/main.rs:466 fn main`（**379 行**）、`crates/agentj-core/src/turn_runner.rs:189 fn run_turn`（**362 行**）、`crates/agentj-ipc/src/lib.rs:861 fn dispatch_session_run`（**252 行**）、`crates/agentj-ipc/src/lib.rs:1420 fn dispatch_permission_respond`（80 行）——按域抽子函数 | 与 ⑩ 同批执行 | 待实施 |
| ⑫ | 【债】锁中毒与 panic 隔离 | 非测试代码 **170 处 `.lock().unwrap()`**：内核是常驻多客户端守护进程，任一线程持锁 panic → mutex 中毒 → 后续每次取锁全部 panic（单会话 bug 可升级为整内核不可用）。改 `unwrap_or_else(|e| e.into_inner())` 或统一封装；请求入口（`agentj-ipc` dispatch / `turn_runner`）补 `catch_unwind` + 错误响应（现全仓仅 `agentj-core/src/hooks.rs:117`、`agentj-question/src/module.rs:356` 两处隔离；另 `crates/agentj-ipc/src/web.rs:257` 的 `std::sync::RwLock` 用 `web.rs:453/457/460` 的 `.read()/.write().expect("sessions")`，是同类中毒路径） | 无 | 待实施 |
| ⑬ | 【债】permission / question 待决机制收敛 | `crates/agentj-permission/src/module.rs` 与 `crates/agentj-question/src/module.rs` 是同一套 pending / owner / settle 的双份镜像（`new` / `set_notifier` / `respond` / `session_of` / `cancel_session` / `clear_all` + `PENDING_TTL` 一一对应）；抽出共用 `PendingRegistry`，消除「修一处漏一处」（BUG-017 已证明代价） | 无 | 待实施 |
| ⑭ | 【修复】e2e 测试环境隔离 | `e2e_cli` / `e2e_permission` **未设** `XDG_DATA_HOME` / `XDG_CONFIG_HOME`（实测仅 `e2e_auth.rs` / `e2e_serve.rs` 设了）→ 本地跑 `cargo test --workspace` 会读写真实 `~/.local/share/agentj/`（会话快照 + `auth.json`），与 0600/0700 凭据纪律相悖 | 无 | 待实施 |
| ⑮ | 【债】Rust 门禁硬化 | ① `[workspace.lints]` 落仓（现 lint 策略只存在于 `ci.yml` 命令行，本地 `cargo build` 不继承；仓内 13 处 `#[allow]`，其中 5 处 `type_complexity` 说明长签名已开始堆积）；② 覆盖率棘轮（`ci.yml:15` `COVERAGE_THRESHOLD: 50` 只有下界、无基线，覆盖率可一路退化到 50% 仍绿） | 无 | 待实施 |
| ⑯ | 【债】crate 粒度收敛 | `agentj-context` 87 行 / `agentj-stats` 133 / `agentj-skills` 132 / `agentj-license` 149 均为 <150 行的独立 crate，而 `agentj-ipc` 单 crate 12,514 行；评估合并或降级为模块（触及 `agentj-cli` 装配与 CI） | 无 | 待实施 |
| ⑰ | 模型元数据与 token 预算 | `schemas/ModelEntry.json` 补 `context_window`（加性 wire），把 `estimate_tokens` 的「ASCII/4 + 非 ASCII×1」字符启发式换成真 tokenizer，或至少按模型真实窗口裁剪（现默认 128k 是保守假设） | 取决于上游目录是否提供窗口字段 | 待实施 |
| ⑱ | `ask` 问题默认超时 | `question` 通道无默认 `timeout_s`：无人应答时本轮**永久挂起**（实测探测客户端须手动杀）；加默认超时 + idle 提示（可镜像 permission 的 60s） | 无 | 待实施 |
| ⑲ | CLI / 配置健壮性 | ① `run --yes` 或非 TTY 自动拒绝（现非 TTY 遇需审批工具一直等到超时）；② `[permission] timeout_ms` 下界校验（`0` 现静默 = 立即拒绝，非法值不报错） | 无 | 待实施 |
| ⑳ | 【修复】远端命名与 `AGENTS.md` 对齐 | 实测 `git remote -v`：`origin` = `git@gitcode.com:jeill/agentj.git`（镜像）、`github` = `git@github.com:JeillZhang/agentj.git`（权威），与 `AGENTS.md`「Remote & Merge Policy」**完全相反**——照文档执行 `git pull --ff-only origin main` / `git push -u origin <branch>` 会打到镜像。二者必须一致（改文档或改 remote 名，重命名方向需用户定夺） | 无（需用户决策） | 待实施 |
| ㉗ | 【债】错误处理约定与生产 `expect` | lib 代码 `let _ =` **58 处**（已剔除 `#[cfg(test)]`；报告所称 222 含测试），多数是合理「尽力而为」却无统一约定与注释，审计成本高；生产路径 `crates/agentj-ipc/src/web.rs:595/642 .expect("valid cookie header")`、`web.rs:627 .expect("retry-after")` 本可 `if let` 处理 | 无 | 待实施 |
| ㉘ | 【修复】Web 登录门与 cookie 安全细节 | ① `crates/agentj-ipc/src/web.rs:587 if !expected.is_empty() && form.token == expected` 非常量时间比较（低成本可换 `subtle::ConstantTimeEq`，限流已缓解但非等价）；② `web.rs:593/640` 会话 cookie 仅 `HttpOnly; SameSite=Lax; Path=/`，**缺 `Secure`**（https 部署下明文回放） | 无 | 待实施 |
| ㉙ | 【债】工具侧路径校验弱于 IPC 侧 | `crates/agentj-ipc/src/file_list.rs:71/126/331` 用 `canonicalize` + 前缀比对（真沙箱），而 `crates/agentj-permission/src/engine.rs:76 escapes_root` = `lexical_normalize` + `starts_with`、`crates/agentj-core/src/tool_paths.rs:16 rebase_tool_args` 亦纯词法归一化 → **符号链接可绕过**；bash 只设 `current_dir`，模型可 `cd /` 后任意操作（无 seccomp/namespace 硬隔离，安全性完全押在权限系统 + 用户审批上）→ 统一到 canonicalize 或明确文档化该边界 | 无 | 待实施 |
| ㉚ | 【债】SSE 事件分派三份重复 | `crates/agentj-model/src/responses.rs:141`、`anthropic.rs:140`、`openai_compat.rs:188` 各一份 ~96–101 行 `push_line`（字节流→行的组装已抽到 `sse.rs`，**事件分派层未抽**）→ 新增 provider 需再抄一遍 | 无 | 待实施 |
| ㉛ | 【债】构建与依赖治理配置 | ① 无 `[profile.release]`（未开 LTO / `codegen-units`）；② **无 `[workspace.dependencies]`**——实测 18 个 crate 共 108 处显式 `version`，`workspace = true` 的 54 处是 `edition/version/rust-version` 继承字段（非依赖统一），三方依赖版本分散易漂移；③ 无 `clippy.toml` / `deny.toml` | 无 | 待实施 |
| ㉜ | 【合规】开源与安全治理文件缺失 | 仓库**无 `LICENSE`**（却有 Ed25519 license 校验的商业功能）、无 `CONTRIBUTING.md`、无 `SECURITY.md`、无 `.github/dependabot.yml`、无 cargo-audit / cargo-deny 依赖漏洞门禁（Cargo.lock 数百依赖无扫描）、无 `.cargo/config.toml` | 许可证选型需用户决策 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- 指标导出扩展（代理头解析 + 计数持久化）代码 + 文档 + summary(se) 已收尾；剩余：提交 →
  推送**两个远端**（本工作副本 `github` = GitHub 权威、`origin` = GitCode 镜像；与 `AGENTS.md`
  远端命名相反）→ 开 GitHub PR（用户负责合并）→ 自动进入测试验收。
- 下一特性：待实施 ②「抽检扩展」（逐请求校验 / 手动全量触发端点，基于 v0.22.0 运行期抽检）。
- 后端候选：上表 ②④⑥⑦⑧⑨（特性）+ ⑩–⑳、㉗–㉜（工程债 / 修复 / 合规）；或前端跟进见
  `summary_and_next_fe.md`（含 ㉑–㉕ 前端债 / 修复）。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝）；`ask` 问题默认超时；把「会话已裁剪
  N 条消息」作为可观测事件；`ModelEvent` 增加 `reasoning_content` 承载。
