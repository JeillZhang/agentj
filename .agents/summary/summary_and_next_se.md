# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.10 文本形式的工具调用不再被当成结论（修复）** —— 同分支叠加 0.29.11 的
> 前端修复（模型选择器不再虚构模型名，见 `summary_and_next_fe.md`）、0.29.9 的
> 「工具轮次用尽仍收尾」与 0.29.8 的「工具往返落库 + 输入框模型标签跟随实际选择」。
> 来源：用户报告四条缺陷（1）会话输入框右上角始终显示配置默认模型，切换模型后不变；
> （2）长任务执行完工具后只回 `passed total: 922\n</rfl:parameter>` 就停，发「继续」接不上；
> （3）「分析当前项目优缺点」读完文档与 summary 后只回一句「让我再快速看一下 git 状态…」
> 就结束，无最终结论；（4）同一提示词下模型只输出
> `analysis <|constrain|>xml<|message|><rfl:parameter name="command">…</rfl:parameter>`
> 标记，工具没执行、也没有结论。
> 后端范围：`crates/agentj-core/src/{turn_runner.rs,text_tool_call.rs}`、
> `crates/agentj-config/src/{lib,merge}.rs`、`crates/agentj-model/src/factory.rs`；
> 前端范围见 `summary_and_next_fe.md`。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.7 → **0.29.10**（纯修复，无新增方法 / schema）。

## v0.29.8–0.29.10（工具调用不再在半路丢失）✅

### v0.29.10 修复：模型把工具调用写成纯文本

证据（用户会话 `~/.local/share/agentj/sessions/69e0d7a0-…` 与 trajectory）：9 轮原生
`bash` 调用后，第 10 轮 assistant 文本变成

```text
analysis <|constrain|>xml<|message|><rfl:parameter name="command">
ls docs/changelog docs/guide 2>/dev/null; echo "---"; ls crates; … git status --short | head
</rfl:parameter>
```

`finish_reason` 为 `stop`、**没有** `tool_calls`（该轮 usage：`prompt_tokens 101076` /
`completion_tokens 568`），内核于是把这段标记当成「最终结论」落库——工具不跑，长任务断在
半路。同一会话稍后又出现同形式的一次泄漏（`git log --format='%ad %s' …`）。

修复（三层，均在 `crates/agentj-core`）：

1. **能无歧义还原就真执行**：新增模块 `text_tool_call.rs`（`looks_like_tool_call_text` /
   `recover_tool_call`），把 `<rfl:parameter name="command">…</rfl:parameter>` 还原成
   `bash {command}`；参数名映射 `command|cmd`→bash、`path|file_path`→read、
   `pattern|glob`→glob、`regex|query`→grep，另支持 `<tool_call>{…}</tool_call>` JSON 方言。
   `turn_runner.rs` 在本轮文本里说明参数时把 `tc_list` 补上该调用、清空 `content`，
   后续走**完全正常的工具路径**（权限门、会话落库、`$/progress`），并 `tracing::warn!`
   记一条线索（`model wrote its tool call as text; recovered it and executing the tool`）。
   合成 id 形如 `call-text-<nanos>`。
2. **认得出但无法还原**（只剩半截标记、未知参数名、多参数）→ 不当结论：附
   `TOOL_CALL_AS_TEXT_NUDGE`（「请改用工具调用通道；若不再需要工具就直接给出最终结论」）重发，
   与截断重发共用 `MAX_INCOMPLETE_RETRIES = 2`。
3. **仍不改** → `Err("model kept writing its tool call as text instead of calling the
   tool")`，不写任何助手消息。

测试：`text_tool_call.rs` 7 条（含逐字复制线上泄漏的 `recovers_the_live_leak_as_a_bash_call`）；
`turn_runner.rs` 新增 `turn_recovers_a_tool_call_the_model_wrote_as_text`（泄漏轮→执行→下一轮
才收尾，历史 4 条）与 `turn_refuses_to_present_tool_call_markup_as_an_answer`。

### v0.29.9：工具轮次用尽时不再丢掉结论

实测复现（真实 opencode-go 网关 + `agentj serve` + WebSocket 客户端）：预算 50 轮跑
「分析一下当前项目的优缺点」，预算耗尽后本轮以 `-32603 {"message":"max tool iterations
| exceeded"}` 结束——UI 只留下模型最后一句过渡语（「让我再快速看一下 git 状态与测试门禁
情况，以及代码规模」），既没有结论也没人告诉用户发生了什么。

| 位置 | 缺陷 |
|---|---|
| `crates/agentj-core/src/turn_runner.rs` | `'outer: for _iter in 0..max_iter` 走完即 `AgentEvent::Error("max tool iterations exceeded")`；整轮的采集成果与中间回答全部作废 |
| `crates/agentj-config/src/lib.rs` | `turn_runner` 会读 `agent.max_tool_iterations`，但 `ConfigProvider::get_int` 只认 `model.max_context_tokens` / `model.quota_warn_percent`——**该键任何配置都改不动**，实际永远是硬编码 `50` |
| `crates/agentj-config/src/merge.rs` | 无 `[agent]` 节可合并（同上，配置层根本没有这个节） |

修复：

- **无工具收尾一趟**（`turn_runner.rs`）：新增 `const BUDGET_EXHAUSTED_NUDGE: &str = "工具调用轮次已用尽。请立刻基于你已经获得的信息给出最终结论，不要再请求任何工具。";`，循环结束后把该指令 push 进 `messages`，调 `Self::conclude_without_tools(provider.as_ref(), &pipeline, messages, &default_options, context_budget, &reply, &mut cancel_rx, &mut usage)` → `chat_stream(request, Vec::new(), options)`（**不下发任何工具定义**），收下的文本作为本轮答复落库并以 `AgentEvent::Done` 结算（`tool_calls` 为全程累计的 `all_tool_calls`）；
  收尾返回 `Ok(None)` = 取消 / 客户端断开 → 发 `AgentEvent::Error("cancelled")` 后返回；
  收尾文本为空 → `Err("max tool iterations exceeded ({max_iter}) and the model returned no
  conclusion")`，**不写助手消息**。
- **`[agent]` 节补全**（`agentj-config`）：新增 `AgentSection { max_tool_iterations: i64 }`
  （`default_max_tool_iterations() = 50`）、`MIN_TOOL_ITERATIONS = 1` / `MAX_TOOL_ITERATIONS =
  1_000` / `clamp_tool_iterations()`，`AgentJConfig` 加 `#[serde(default)] pub agent: AgentSection`，
  `get_int("agent.max_tool_iterations")` 返钳制值，`merge_config` 里文件层非默认值胜出，
  `docs/guide/configuration.md` 新增 `[agent]` 节说明。

### v0.29.8：工具往返落库 + 截断流不再当成结论

### 缺陷与根因

用户会话快照 `0b45b81f-…json` 里只有 4 条消息：
`user「分析一下当前项目的优缺点。」` → `assistant「passed total: 922\n</rfl:parameter>」`
→ `user「继续」` → 同样的残句；而同一会话的 trajectory 有 100 条 NDJSON、含 21 轮工具调用。
即：**工具轮次从未落库**，最终那句残句被当成「最终结论」。

| 位置 | 缺陷 |
|---|---|
| `crates/agentj-core/src/turn_runner.rs` | 只 `session.append(user_input)` 与**最终** `append(ChatMessage::assistant(content))`；每轮工具往返仅经 `pipeline.reflux(&mut messages, …)` 进入本轮请求的内存 vec，进程结束即丢 |
| 同上（流收尾） | `for _iter in 0..max_iter` 内层遇到 `ModelEvent::Done` 或流结束就 break；`tc_list.is_empty()` 即 `AgentEvent::Done{final_response}` 并 `return Ok(())`——**没有 `saw_done` 判定、不看 `finish_reason == "length"`**，半截回复/退化输出一律当作成功收尾 |
| `crates/agentj-model/src/factory.rs` | mock 脚本（`mock` / `mock-workbench` / `mock-ask`）以裸 `ModelEvent::ToolCall` 结束一轮、**不带 `Done`**——夹具本身违反 ChatStream 契约 |

「继续」失效的量化证据：后续 run 的 `usage.prompt_tokens` 只有 **527**（历史里没有工具上下文），
而首次 run 是 **731,528**。

### 修复

- **整轮落库**（`turn_runner.rs`）：新增 `let mut round: Vec<(ToolCall, String)> =
  Vec::with_capacity(tc_list.len());`，工具执行后 `round.push((tc, tool_result))`；工具循环结束
  且**未取消**时写一条 `ChatMessage { role: Role::Assistant, content, tool_call_id: None,
  tool_calls: round.iter().map(|(tc, _)| tc.clone()).collect() }`，再为每个条目写一条
  `ChatMessage::tool_result(tc.id, result)`。落库**延到工具执行之后**：取消/失败的一轮不写任何
  东西，不会留下没有结果的悬空 `tool_calls`（上游会 400）。
- **截断流不再算收尾**（`turn_runner.rs`）：新增 `let mut saw_done = false;`（`ModelEvent::Done`
  置位）与 `const MAX_INCOMPLETE_RETRIES: usize = 2;`；`if !saw_done && tc_list.is_empty() &&
  !content.trim().is_empty()` → 未超上限则 `continue 'outer`（整轮重发），超限返回
  `Err("model stream ended before completion")`。**带工具调用的轮次不受影响**：`openai_compat`
  只在收到 `finish_reason == "tool_calls"` 后才吐 `ToolCall`，那本身就是上游已完成该消息的信号。
  曾试过更严的 `!saw_done || 空文本` 与 `!saw_done && heard_any`，两者都让 14 条 `agentj-ipc`
  用例失败（`RepeatingMock` 等夹具的脚本轮不带 `Done`，被误判为截断后重试吞掉了下一轮）。
- **夹具补齐 `Done`**（`factory.rs`）：`MockStateful`、`MockWorkbench::script` 两轮、`MockAsk`
  的脚本各补 `Ok(ModelEvent::Done)`；对应单测断言从 `[ToolCall]` 改为 `[ToolCall, Done]`。
- 前端侧（标签取 `selected?.name ?? 配置默认值`、持久化选择重新水合）见
  `summary_and_next_fe.md` 与 `docs/changelog/v0.29.md` 的 0.29.8 段。

### 测试

| 用例 | 位置 | 修的缺陷 |
|---|---|---|
| `turn_persists_tool_round_trips_so_the_next_turn_has_context` | `crates/agentj-core/src/turn_runner.rs` | 修前历史只有 `user` + 最终 `assistant`；修后 4 条：`user`、`assistant{tool_calls:[c1]}`、`tool{c1}`、`assistant` |
| `turn_replays_a_round_whose_stream_never_finished` | 同上 | 脚本 `[Delta("half")]` → 重发一次，只落最终答案（history len 2） |
| `turn_fails_when_no_round_ever_completes` | 同上 | 恒截断 → 共 `1 + MAX_INCOMPLETE_RETRIES` 次请求后如实报错，历史不落库 |
| `turn_accepts_an_empty_but_terminated_round` | 同上 | 有 `Done`、无文本的正常收尾不重试（守卫不能误伤空回复） |
| `factory_creates_mock_workbench` / `factory_creates_mock_ask` | `crates/agentj-model/src/factory.rs` | 夹具契约：工具调用后必有 `Done` |

### 验证结果（0.29.9–0.29.10，2026-10-17）

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace` | **900 passed, 0 failed**（886 → 891 → 900；0.29.10 新增 9 条：`text_tool_call` 7 + `turn_runner` 2） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo fmt --all` | clean |
| `./scripts/check-version.sh` | `version-ok`（0.29.10 / 0.18.0） |
| 真实 key E2E：预算压到 3 轮 | `[agent] max_tool_iterations = 3` + 「统计 crates/ 下每个 crate 的名字与行数」→ 3 轮工具后给出**完整 18-crate 表格结论**（不再 `-32603 max tool iterations exceeded`），快照末尾是对应 1527 字符的 `assistant`（无 `tool_calls`） |
| 真实 key E2E：默认 50 轮 | 「分析一下当前项目的优缺点」→ 272.5s / 44 次工具调用后给出完整分析报告（优点 6 条 / 缺点与风险 / 总体评价）；发「继续」后模型不再重复残句，而是带完整上下文（44 次调用累计 `prompt_tokens` **5,728,872**，单次≈128k 预算）反问「希望我接着往哪个方向深入？」（`ask` 工具 + 3 选项） |
| 真实 key 探测（0.29.10） | 网关模型 id 必须给**裸 id**（`deepseek-v4.1-flash`）；带 `opencode-go/` 前缀一律 `Upstream request failed: Model is unavailable.`。37 个线上模型单轮探测：可用的全部走**原生** `tool_calls`，`minimax-m3` / `minimax-m2.5` 各跑完一轮完整「分析项目优缺点」（~10 轮工具、`FAILURES=0`、结尾 8248 / 2459 字符结论）**没有复现泄漏**——泄漏模型仍未定位（用户 Web 端选择也未记录到快照/轨迹） |

> 注意：CLI 的 `--model` 才写全 `opencode-go/deepseek-v4.1-flash`（CLI 自己剥 provider 前缀）；
> 直连网关时反过来，必须裸 id。

### 验证结果（0.29.8，2026-10-17）

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace` | **886 passed, 0 failed**（882 → 886，+4 条新增；`agentj-core` 153 / `agentj-ipc` 193 / `agentj-model` 75） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings（12.85s） |
| `cargo fmt --all` | clean |
| Web / SDK / TUI | 658 passed（655 → 658，+3）· 55 passed · 177 passed |
| `./scripts/check-version.sh` | `version-ok`（0.29.8 / 0.18.0） |
| 真实 key E2E（opencode-go `deepseek-v4.1-flash`） | `agentj run "…本仓库 crates/ 下有哪些 crate…"` → ~5 轮工具后输出完整 18-crate 表格；会话快照实记 `user` + 5 组 `assistant{tool_calls}`/`tool` + 最终 `assistant`（674 字符） |

> 注意：模型 id 必须写全 `opencode-go/deepseek-v4.1-flash`；只给 `deepseek-v4.1-flash` 会得到
> `unknown model 'deepseek-v4.1-flash' for provider 'opencode-go'`。

## 已完成（0.29.0 – 0.29.7，详见 `docs/changelog/v0.29.md`）

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
- **泄漏模型未定位 / 网关翻译不确定**（0.29.10）：把工具调用写成文本的模型仍未确定（用户 Web 端选择不落快照/轨迹，44 个线上 id 单轮探测全部返回原生 `tool_calls`），因此 0.29.10 的还原能力只靠**参数名**推断工具（`command`→bash 等）；多参数调用（如 write 的 path+content）与未知参数名仍走「纠偏重发 → 如实报错」。若再拿到具体模型，可补该模型方言的完整解析（含外层工具名）。
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

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- v0.29.10 代码 + 测试 + 版本元数据 + changelog + 两份 summary 已收尾；剩余：提交 → 推送**两个远端**（本机 `github` = GitHub 权威、`origin` = GitCode 镜像；与 AGENTS.md 的远端命名相反）→ 更新在 GitHub 的 PR（用户负责合并）。
- 用户已确认泄漏模型是 **`beam-free`**（2026-10-17）。用它的**真实会话前缀**（`69e0d7a0-…`
  trajectory 前 19 条消息 / 105KB）回放 3 次、以及完整 48 轮前缀回放，全部返回原生 `tool_calls`；
  用 harness 跑完整「分析项目优缺点」（budget 50）也没泄漏——**泄漏是随机的、无法稳定复现**，
  只能依赖 0.29.10 的还原 + 纠偏重试兜底。同一 harness run 里 `beam-free` 的收尾质量很差：
  12 轮工具后只回 38 字符 `# Summary & Next — Backend (SE / 后端)`（文档标题回声），
  属于**模型能力问题**，不是内核丢失结论（修复前那种「工具没跑 + 只回标记」已排除）。
- 次要观察（已单独修）：用户轨迹里那条 `unknown model 'deepseek-chat'` 来自 Web 选择器的假占位模型，
  0.29.11 已修；内核的 `model/list` / `kernel/status` 本身给的是 37 个真实 id。
- 读法提醒：`<sid>.trajectory` 里的 `usage` 行是**每轮一条、整轮累计**（`0b45b81f` 731,528、
  `69e0d7a0` 第二轮 1,070,650 ≈ 48 次调用之和），不是单次请求的 prompt 大小；据此反推单次
  prompt ≈ 20–30k tokens，仍在 128k 预算内（`fit`/`fit_messages` 生效）。别把它误判成预算失效。
- 后端候选：上表 ①②④⑥⑦⑧；或前端跟进见 `summary_and_next_fe.md`。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝）；`ask` 问题默认超时；把「会话已裁剪
  N 条消息」作为可观测事件；`ModelEvent` 增加 `reasoning_content` 承载（`openai_compat`
  目前丢弃该字段，思考过程不可见）。
