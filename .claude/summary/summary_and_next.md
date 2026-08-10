# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能设计文档：`docs/superpowers/specs/2026-08-09-agentj-core-design.md`
> 当前实现计划：`docs/superpowers/plans/2026-08-09-agentj-core-plan.md`

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 预估天数 |
|------|------|------|--------|----------|
| Phase A | v0.1 核心 Agent 循环（T0-T15，16 任务，已迁移到 Task Template Specification） | 无 | P0 | — |
| Phase A | v0.1 基础模块（Session / Tool / Model / MCP Bridge / Config / Context） | 核心循环 | P0 | — |
| Phase B | v0.2 生态扩展（Plugin Manager / Skills Loader / Sub-Agent Dispatcher / Hook System） | Phase A | P1 | — |
| Phase C | v0.2+ 模型扩展（OpenAI 兼容 / Ollama） | Phase B | P1 | — |
| Phase D | v0.3+ 商业扩展（Web Dashboard / 用量统计 / License） | Phase C | P2 | — |

## 下一步

- 运行 `/jskill-feature-dev docs/superpowers/plans/2026-08-09-agentj-core-plan.md`，从 T0（Workspace Scaffold）开始。
- 计划已按 **Task Template Specification** 结构化：16 个任务（T0-T15），每任务含 id / files(≤3) / depends_on / ac / regression / test_code / impl_code / verify。
- 迁移已修复的问题：伪 RED（编译错误→断言失败）、impl_code 残稿（Wait.../unimplemented! 已清除）、ContextPipeline 改为 core trait（AgentLoop 纯 trait 依赖）、execute_with_timeout 阻塞 bug（join→recv_timeout）、Task 14 E2E 无断言（改为 CARGO_BIN_EXE 真实断言）、benchmark 语法错误。

## Phase A: v0.1 核心 Agent 循环（设计+规划完成，待开发）

### 进度与摘要

- ✅ 设计文档已批准（中文，Mermaid 架构图）：消息总线 + Trait 注册的内核架构，hub-and-spoke crate 布局（agentj-core 定义共享 trait，领域 crate 实现，agentj-ipc 包 JSON-RPC，agentj-cli 为唯一入口）。
- ✅ 实现计划已迁移到新模板（2026-08-09-agentj-core-plan.md）：16 任务、依赖图无环、AC 可观察、每任务有 regression 护栏、TDD 顺序强制。
- ✅ **模块生命周期设计已修复闭环**（设计审查后回填）：新增「模块生命周期状态机」章节（Unloaded→Initializing→Ready→Stopping→Stopped/Failed，状态由 Kernel 维护）；Kernel 收到 Shutdown 逆序调用 module.shutdown()（带超时，ShutdownTimeout）；init 失败返回 Err 且状态置 Failed 不拖垮内核；IPC shutdown 真实发 Shutdown 并等待优雅关闭；CLI 生产路径创建 Kernel 并注册 AgentLoopModule（轻量接线，编排仍直接调 trait 对象）；MCP 子进程 kill_on_drop(true) 防泄漏；EventType 预留 ModuleLoaded/Failed/Stopped 契约（v0.2 Hook System 消费）。涉及 T3/T4/T12/T13/T14 修订 + 决策 #8/#9。
- ⬜ 开发未开始（无代码提交，workspace 尚未 scaffold）。

### 遗留问题 / 已知限制

- 完整 `KernelMessage` 路由管线（`ModelResponse` / `ToolCallRequest` / `ToolCallResult` 变体）推迟到 v0.2 Sub-Agent Dispatcher 引入；v0.1 的 agent 循环直接调用 trait 对象。
- v0.1 的 `parse_sse_chunk` 对 tool_call delta 直接返回 Done，增量 tool_call 组装是 v0.2 工作。
- Session 生命周期（delete/archive/expire）推迟 v0.2（H1 决策）：v0.1 的 `SessionStore` 仅 create/append/get/estimate_tokens，会话只增不减。
- MCP 连接复用 / 长驻 server 生命周期属 v0.2：v0.1 每次调用新建 MCP server 子进程（已 kill_on_drop 防泄漏）。

### 下一步

- 执行 `/jskill-feature-dev docs/superpowers/plans/2026-08-09-agentj-core-plan.md`，从 T0（Workspace Scaffold）开始，按依赖图推进（Group A 7 个并行任务）。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新"待实施"表。
