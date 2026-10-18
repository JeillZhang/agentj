# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**BUG-017 空 thinking 落盘守卫（验收修复）** —— 与前端同分支
> `feature/tool-card-command-summary`（PR #84）交付；前端侧见 `summary_and_next_fe.md`。
> 仅改 `crates/agentj-ipc/src/lib.rs`，协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 0.18.0 /
> 28 方法 / 78 schemas；工作区仍 **0.1.0**）。下方 v0.29.12 及前序章节为**上一阶段记录**（保留备查）。

## BUG-017 空 thinking 落盘守卫（验收修复）✅

### 现象与根因

- 现象（用户 m00229）：对话中栏在 bash 工具卡片之间出现「只有左侧强调线、无任何文字」的空助手气泡。
- 根因：网关在工具调用之间把**孤立的零宽空格（U+200B）**当保活增量下发；U+200B **无 Unicode
  `White_Space` 属性**，`char::is_whitespace()` 不判空 → `flush_thinking` 只守卫
  `!pending.is_empty()`，白空格 / 零宽一律落盘为 `TrajectoryValue::Thinking`（既污染轨迹，
  也让前端每帧渲染一个空气泡）。

### 修复

- `crates/agentj-ipc/src/lib.rs`（`impl IpcServer`）：`flush_thinking` 改为**总是**
  `let text = std::mem::take(pending);`，再由新关联函数
  `fn is_visually_blank(s: &str) -> bool { !s.chars().any(|c| !c.is_whitespace() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}')) }`
  判定「仅空白 / 零宽 / BOM」即 `return`（不落盘）；始终 take 可避免零宽残留污染下一段 thinking。
- **未改** `AgentEvent::Delta` 的 `$/progress` 广播与任何 wire 字段（前端已有
  `agentj-ui/apps/web/src/blankText.ts` 过滤，保持 live 与落盘一致）。

### 测试（RED → GREEN）

新增 3 条（插在 `run_records_one_merged_thinking_node_for_two_deltas` 之后）：
`run_drops_zero_width_only_deltas_instead_of_recording_blank_thinking`（`"\u{200b}"` /
`" \n"` / `"\u{feff}"` + `Done` → `traj_kinds == ["user"]`）、
`run_keeps_thinking_that_contains_visible_glyphs`（`"\u{200b}"` + `"ok"` →
`["user","thinking"]`，content `"\u{200b}ok"`）、
`is_visually_blank_covers_zero_width_and_whitespace_only_text`（同步 test）。

### 验证结果

| 门禁 | 结果 |
|------|------|
| `cargo test -p agentj-ipc --lib` | **197 passed** |
| `cargo test --workspace` | **911 passed / 0 failed** |
| `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` | FMT-OK / 0 warning |
| `./scripts/check-version.sh` | `version-ok`（0.1.0 / 0.18.0） |

## v0.29.12（审批不再「查无此单」+ grep 不再依赖 ripgrep）✅

### 缺陷与根因

| # | 现象 | 根因 |
|---|------|------|
| 6a | 一轮连发多条 `bash` 时 `tool 'bash' permission denied: request not found` | `PermissionModule::respond` 先 `pending.remove(request_id)` 再投递决定，而工具侧 `await_approval` 是「先查表、后订阅 watch」；客户端答得比工具挂起更快时决定随条目一起被删 → 工具只拿到 `Deny { reason: "unknown or expired approval request" }`（旧文案即 `request not found`）。一轮里每条 `bash` 都要审批，任一次抢跑就把该工具判死 |
| 6a′ | 断线重连后每次审批立刻被拒（`disconnected`） | owner 在连接收尾时被 `clear_owner` 清空，而 `session/subscribe` 原先不认领归属 → `attach_permission_notifier` 见无 owner 即刻结算 |
| 6b | `tool 'grep' execution failed: No such file or directory (os error 2)` | `GrepTool` 无条件下 `Command::new("rg")`；机器上没装 ripgrep 即 ENOENT |

### 修复

- `crates/agentj-permission/src/module.rs`：`respond` **保留**待决条目、只投递决定；重复或
  迟到的第二次答复返回 `false`，不覆盖第一次；`view()`（只读窥视）/`session_of()`（归属查询）
  辅助；`await_approval` 在**决定送达后**才 `prune_consumed`；`cancel_session` 同样保留条目并
  投递真实原因（`disconnected` / `cancelled`），工具不再报「查无此单」；`PendingRequest.created_at`
  + `PENDING_TTL = 300s` + 注册时 `prune_pending` 保证 pending 表有界。
- `crates/agentj-ipc/src/lib.rs`：`session/subscribe` 在会话**没有 owner** 时把发起订阅的连接
  登记为 owner（已有 owner 不抢）→ 掉线重连的 Web/TUI 重新订阅后继续持有审批权。
- `crates/agentj-tools/src/builtins.rs`：`grep` 优先调用 `rg`（补 `--line-number` 统一输出
  形状）；`rg` 不存在时回退**进程内检索**（`regex` + `globset` + `walkdir`：按行匹配、
  `include` 按文件名或相对路径匹配、`SKIP_DIRS` 跳过 `.git`/`target`/`node_modules` 等、
  跳过二进制与 >1MiB 文件、命中上限 2000 条 + 截断标记）。两个路径输出均为 `path:line:match`。

### 测试（RED → GREEN）

- permission（48 passed）：`approval_survives_a_respond_that_arrives_before_the_await`、
  `respond_refuses_a_second_decision_for_the_same_request`、`session_of_peeks_pending_request_session`、
  `view_peeks_without_consuming`、`stale_pending_entries_are_pruned`。把 `respond` 回退成
  `remove` 即 RED（5 个失败），关键断言
  `left: Deny { reason: "unknown or expired approval request" } / right: Allow`。
- ipc：`subscribe_reclaims_ownership_of_an_unowned_session`。
- tools：`grep_falls_back_to_an_in_process_search_without_ripgrep`、
  `grep_fallback_matches_deep_files_and_honours_the_include_glob`、
  `grep_fallback_skips_binary_files_and_reports_bad_regex`。

### 验证结果（0.29.12，2026-10-17）

| 门禁 | 结果 |
|------|------|
| `cargo test --workspace` | **908 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `./scripts/check-version.sh` | `version-ok`（0.29.12 / 0.18.0） |
| 真实网关 E2E（`beam-free` + `ask` + 故意重复/迟到答复 + `PATH` 去掉 ripgrep） | 8 次审批（6×bash、1×grep、1×bash）全部放行、0 次 `permission denied`；grep 回退命中 `crates/agentj-core/src/turn_runner.rs:189` |

### 同分支已修复的前序缺陷（0.29.8–0.29.11）

- 0.29.8：工具往返落库（发「继续」不再失忆）+ 截断流不再当结论；输入框模型标签跟随实际选择。
- 0.29.9：工具轮次用尽仍强制「无工具收尾」并给出结论；`[agent] max_tool_iterations` 可配（1..=1000）。
- 0.29.10：模型写成纯文本方言的工具调用（`<rfl:parameter>` / `<tool_call>`）被还原执行，而非当结论。
- 0.29.11：Web 模型选择器不再虚构 `deepseek-chat` 占位（前端，见 `_fe.md`）。
## 已完成（0.29.0 – 0.29.11，详见 `docs/changelog/v0.1.md`）

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
| ⑦ | 工具结果落库上限 | 落库前对单个工具结果设上限（或摘要 + 「已截断」标记），避免 7.9MB 级 `glob` 结果撑爆快照 | 0.29.8 已落库工具往返（存储侧尚无上限） | 待实施 |
| ⑧ | `session/run` 回显生效模型 | 结果/trajectory 记录实际使用的 `provider`/`model`（加性 wire），便于排障与前端一致性校验 | 0.29.8 前端已按选择下发并显示 | 待实施 |
| ⑨ | `glob` 路径语义 | 让 `glob` 支持路径型 pattern（`crates/*/src/*.rs`）与 `**`，而不是只按 basename 匹配（当前路径型 pattern 恒空） | 0.29.12 已引入 `globset`（grep 回退依赖） | 待实施 |
| ⑩ | 【债】IPC 协议层拆分 | `crates/agentj-ipc/src/lib.rs`（实测 **7,339 行 / 295,749 B**）按 dispatch 域拆为 `dispatch_session` / `dispatch_permission` / `dispatch_question` / `dispatch_file` / `dispatch_provider` 等子模块；`impl IpcServer`（行 486–1931，约 **1,445 行**、30+ 方法）切分为多个 impl 块或独立 handler | 无（纯重构，wire 面不变） | 待实施 |
| ⑪ | 【债】超长函数治理 | 实测 lib 代码 831 个函数中 **26 个 >50 行**（违反 `AGENTS.md` 「function ≤50 lines」）：`crates/agentj-cli/src/main.rs:466 fn main`（**379 行**）、`crates/agentj-core/src/turn_runner.rs:189 fn run_turn`（**362 行**）、`crates/agentj-ipc/src/lib.rs:861 fn dispatch_session_run`（**252 行**）、`crates/agentj-ipc/src/lib.rs:1420 fn dispatch_permission_respond`（80 行）——按域抽子函数 | 与 ⑩ 同批执行 | 待实施 |
| ⑫ | 【债】锁中毒与 panic 隔离 | 非测试代码 **170 处 `.lock().unwrap()`**：内核是常驻多客户端守护进程，任一线程持锁 panic → mutex 中毒 → 后续每次取锁全部 panic（单会话 bug 可升级为整内核不可用）。改 `unwrap_or_else(|e| e.into_inner())` 或统一封装；请求入口（`agentj-ipc` dispatch / `turn_runner`）补 `catch_unwind` + 错误响应（现全仓仅 `agentj-core/src/hooks.rs:117`、`agentj-question/src/module.rs:356` 两处隔离） | 无 | 待实施 |
| ⑬ | 【债】permission / question 待决机制收敛 | `crates/agentj-permission/src/module.rs` 与 `crates/agentj-question/src/module.rs` 是同一套 pending / owner / settle 的双份镜像（`new` / `set_notifier` / `respond` / `session_of` / `cancel_session` / `clear_all` + `PENDING_TTL` 一一对应）；抽出共用 `PendingRegistry`，消除「修一处漏一处」（BUG-017 已证明代价） | 无 | 待实施 |
| ⑭ | 【修复】e2e 测试环境隔离 | `e2e_cli` / `e2e_permission` **未设** `XDG_DATA_HOME` / `XDG_CONFIG_HOME`（实测仅 `e2e_auth.rs` / `e2e_serve.rs` 设了）→ 本地跑 `cargo test --workspace` 会读写真实 `~/.local/share/agentj/`（会话快照 + `auth.json`），与 0600/0700 凭据纪律相悖 | 无 | 待实施 |
| ⑮ | 【债】Rust 门禁硬化 | ① `[workspace.lints]` 落仓（现 lint 策略只存在于 `ci.yml` 命令行，本地 `cargo build` 不继承；仓内 13 处 `#[allow]`，其中 5 处 `type_complexity` 说明长签名已开始堆积）；② 覆盖率棘轮（`ci.yml:15` `COVERAGE_THRESHOLD: 50` 只有下界、无基线，覆盖率可一路退化到 50% 仍绿） | 无 | 待实施 |
| ⑯ | 【债】crate 粒度收敛 | `agentj-context` 87 行 / `agentj-stats` 133 / `agentj-skills` 132 / `agentj-license` 149 均为 <150 行的独立 crate，而 `agentj-ipc` 单 crate 12,514 行；评估合并或降级为模块（触及 `agentj-cli` 装配与 CI） | 无 | 待实施 |
| ⑰ | 模型元数据与 token 预算 | `schemas/ModelEntry.json` 补 `context_window`（加性 wire），把 `estimate_tokens` 的「ASCII/4 + 非 ASCII×1」字符启发式换成真 tokenizer，或至少按模型真实窗口裁剪（现默认 128k 是保守假设） | 取决于上游目录是否提供窗口字段 | 待实施 |
| ⑱ | `ask` 问题默认超时 | `question` 通道无默认 `timeout_s`：无人应答时本轮**永久挂起**（实测探测客户端须手动杀）；加默认超时 + idle 提示（可镜像 permission 的 60s） | 无 | 待实施 |
| ⑲ | CLI / 配置健壮性 | ① `run --yes` 或非 TTY 自动拒绝（现非 TTY 遇需审批工具一直等到超时）；② `[permission] timeout_ms` 下界校验（`0` 现静默 = 立即拒绝，非法值不报错） | 无 | 待实施 |
| ⑳ | 【修复】远端命名与 `AGENTS.md` 对齐 | 实测 `git remote -v`：`origin` = `git@gitcode.com:jeill/agentj.git`（镜像）、`github` = `git@github.com:JeillZhang/agentj.git`（权威），与 `AGENTS.md`「Remote & Merge Policy」**完全相反**——照文档执行 `git pull --ff-only origin main` / `git push -u origin <branch>` 会打到镜像。二者必须一致（改文档或改 remote 名，重命名方向需用户定夺） | 无（需用户决策） | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- v0.29.12 代码 + 测试 + 版本元数据 + changelog + guide + 两份 summary 已收尾；剩余：提交
  → 推送**两个远端**（本机 `github` = GitHub 权威、`origin` = GitCode 镜像；与 AGENTS.md 的
  远端命名相反）→ 更新 GitHub PR #81（用户负责合并）。
- 本轮真实网关 E2E 暴露的观察：
  - `beam-free` 在 ask 模式下 8 次审批全部放行（含故意重复/迟到答复），证明 6a 已修；
  - 无 ripgrep 的 `PATH` 下 grep 走回退路径并命中 `turn_runner.rs:189`，证明 6b 已修；
  - 模型仍会在收尾时只回标题回声（38 字符）——属**模型能力问题**，非内核丢失结论。
- 读法提醒：`<sid>.trajectory` 里的 `usage` 行是**每轮一条、整轮累计**（不是单次请求的
  prompt 大小），反推单次 prompt ≈ 20–30k tokens，仍在 128k 预算内。
- 后端候选：上表 ①②④⑥⑦⑧⑨（特性）+ ⑩–⑳（工程债 / 修复，见本表新增行）；或前端跟进见
  `summary_and_next_fe.md`（含 ㉑–㉕ 前端债 / 修复）。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝）；`ask` 问题默认超时；把「会话已裁剪
  N 条消息」作为可观测事件；`ModelEvent` 增加 `reasoning_content` 承载（`openai_compat`
  目前丢弃该字段，思考过程不可见）。
