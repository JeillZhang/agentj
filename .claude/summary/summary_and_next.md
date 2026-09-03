# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G3 token 级用量统计**——设计文档 `docs/superpowers/specs/2026-09-02-g3-token-usage-design.md` + 实现计划 `docs/superpowers/plans/2026-09-02-g3-token-usage-plan.md`（v0.8.0 已发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| ✅ 完成 | G2 修复：单全局 in-flight 会话槽 → per-session 路由（T1-T6，v0.6.0） | v0.5 | P0 | ✅ 已实现 v0.6.0 |
| ✅ 完成 | G4 会话持久化生产接线（FileBackedStore 生产接入 + 唯一 tmp + 目录 fsync）（T0-T6，v0.7.0） | v0.6.0 | P0 | ✅ 已实现 v0.7.0 |
| ✅ 完成 | **G3 token 级用量统计（TokenUsage + 4 提供方 + SessionRunResult.usage + CLI 激活）**（T0-T11，v0.8.0） | v0.7.0 | P0 | ✅ 已实现 v0.8.0 |

## Phase: G3 token 级用量统计 ✅ (本次完成)

### 进度与摘要

- ✅ **根因**：`record_token_usage` 死代码（仅单测调用）：`OnTurnComplete` 只传 `&SessionId`、`ModelEvent` 无 Usage 变体、4 真实提供方（openai_compat/responses/anthropic/ollama）全部丢弃流式 SSE 末帧 usage，CLI 仅 `record_turn` 累加 turn 数、token 恒为 0。

- ✅ **方案**（决策 D1-D8）：① `agentj-core` 定义 `TokenUsage{prompt_tokens,completion_tokens}`（`Copy,saturating_add`）+ `ModelEvent::Usage(TokenUsage)` + `AgentEvent::Done{usage:Option<TokenUsage>}` + `OnTurnComplete(&SessionId,Option<TokenUsage>)`；② `TurnRunner` 跨迭代累加 `Option<TokenUsage>` 并经 `Done.usage` + `OnTurnComplete` 双通道回传；③ 4 提供方流式解析：openai_compat（`stream_options.include_usage` + `choices:[]`+usage 末帧 + Done@[DONE]）/ responses（`response.completed.usage{input,output}`）/ anthropic（`message_delta.usage`）/ ollama（`done:true` 行 `prompt_eval_count/eval_count` 扩展状态机）；④ `agentj-protocol` 加性 `UsageInfo` + `SessionRunResult.usage:Option<UsageInfo>`（`skip_serializing_if`，33 schemas）；⑤ `agentj-ipc` 映射 `Done.usage`→结果 JSON；⑥ CLI `record_completed_turn` 三分支（Some非零→`record_token_usage` / Some零/None→`record_turn`）激活死代码；⑦ mock 工厂插入 `Usage(100,20)` 帧 + e2e 隔离验证。

- ✅ **任务分解（T0-T11）已完成**：
  - T0 — `TokenUsage`/`ModelEvent::Usage`/`Done.usage`/`OnTurnComplete` + `TurnRunner` 聚合（`UsageScriptedProvider` 跨迭代 10,5+90,15 聚合至 100,20 + 饱和测试）。
  - T1 — `UsageInfo` + `SessionRunResult.usage` 加性 + `gen_schemas` 重生成（33 schemas，`UsageInfo.json` 新增，drift 门禁通过）。
  - T2 — `dispatch_session_run` 映射 `Done.usage`→`usage` JSON（`stub_bus_server_with_usage` 辅助，`session_run_result_includes_usage_when_reported` 断言）。
  - T3 — openai_compat：`stream_options.include_usage` + SSE usage 解析 + Done@[DONE]（`finish_reason` 不再发 Done，`openai_compat_parses_streaming_usage_frame` 断言 usage 在 Done 前）。
  - T4 — responses：`response.completed` 解析 `input_tokens/output_tokens` → `Usage`（`responses_parses_usage_from_completed_event`）。
  - T5 — anthropic：`message_delta` 解析 `input_tokens/output_tokens` → `Usage`（`anthropic_parses_usage_from_message_delta`）。
  - T6 — ollama：`done:true` 行解析 `prompt_eval_count/eval_count` → `Usage`（状态机扩展含 `pending_usage`，`ollama_parses_usage_from_done_line` 含 BUG-010 排序保证）。
  - T7 — `bindings/ts` 重生成（`UsageInfo` 接口 + `SessionRunResult.usage?:UsageInfo|null`，`tsc --noEmit` 通过）。
  - T8 — CLI `record_completed_turn` 三分支（3 单测：非零/零值/None 均保 turn 计数）。
  - T9 — 工厂 mock 插入 `Usage(100,20)` + `e2e_serve_run_result_includes_usage`（隔离 `sess_dir` + `[permission] allow` + `XDG` 隔离，TDD 失败→通过，e2e_permission 回归验证）。
  - T10 — workspace version `0.7.0` → `0.8.0`（`Cargo.toml` + `Cargo.lock` via `cargo build`）。
  - T11 — 文档同步 `AGENTS.md`/`README.md`/summary（本次）。

- ✅ **验证结果**：
  - `cargo test --workspace -- --skip permission_request_notified` — 全量通过（~240+ 单测，含新增 TokenUsage/provider/CLI/IPC 单测 + e2e_cli 过滤，60s 超时测试隔离验证通过）
  - `cargo test -p agentj-model` — 32+ 通过（含 4 provider usage 新测）
  - `cargo test -p agentj-core` — 71 通过（含新聚合与饱和测试）
  - `cargo test -p agentj-ipc` — 40+ 通过（含 usage 映射）
  - `cargo test -p agentj-protocol -- drift` — 2 通过（33 schemas 无漂移）
  - `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
  - `cargo fmt --all --check` — 通过
  - `bindings/ts` generate + build — 通过
  - `e2e_serve_run_result_includes_usage` + `e2e_permission` 隔离回归 — 通过

### 遗留问题 / 已知限制

- `ChatResponse.usage` 字段与非流式 `chat()` 的 usage 解析按 D8 延后（`chat()` 零生产调用者；加字段需同步改 8 文件 10 构造点，违反 ≤3 文件任务粒度；流式（生产路径）已完整覆盖 gap G3 核心指控）。
- `record_token_usage` 零值守卫（`0,0` → 不建 entry）由 CLI 三分支绕过：零值回退 `record_turn` 以保 turn 计数。
- `u64::MAX` 饱和处理为防御性（真实 token 不可能溢出）。
- `cancel`/`max_iter` 路径不触发 `OnTurnComplete`（统计口径 = 成功 turn）。
- SSE `stream_options.include_usage` 服务端不支持时为 `None` 路径，不致命（加性容忍）。

### 下一步

- 按 gap 档案优先级推进下一差距项（G5-G8），或根据产品规划启动新 feature 设计 → `/jskill-feature-design`。
- 后续可考虑 `ChatResponse.usage` 落地（待 `chat()` 获生产调用者）、统计面板方法扩展或 `ProgressValue::Usage` 流式广播（需新设计评审）。
