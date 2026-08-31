# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.5 内核调度全重构（决策 #18 兑现）**——实现计划 `docs/superpowers/plans/2026-08-30-agentj-kernel-scheduling-refactor-plan.md`（13 任务 T1-T13 全部完成；版本 0.4.1 → 0.5.0）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布：事件总线 + ModuleResolver + 子代理调度 + skills + 插件 + Ollama + MCP 长驻 + 会话过期 + FileBackedStore） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；方法面 14→16；测试验收 6/6 Bug 修复，v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成（本阶段） | **v0.5 内核调度全重构**：KernelMessage 总线化（UserRequest/ToolCallRequest 目标路由/ServiceCall 异步）、TurnRunner 转子代理、ToolExecutorModule/AgentLoopModule 内核模块化、子代理经 `subagent/dispatch` 异步服务、IpcServer/CLI 全面接总线 | v0.4 | P0 | 已完成 |

## Phase: v0.5 内核调度全重构 开发完成 ✅（本次完成）

### 进度与摘要

- ✅ **13 任务全部完成并逐任务 TDD + 集成验证**：
  - 基础重构（T1-T3）：`AgentEvent`/`OnTurnComplete` 迁至 `types.rs`（re-export 过渡）；`KernelMessage::UserRequest{reply}` + `ToolCallRequest{target,reply:ToolCallReply}` 目标路由 + `Kernel::modules()` 查询；`AsyncKernelService` 异步分支（`ServiceRegistry::register_kernel_async_service` + kernel.spawn 异步 reply）。
  - 转子与执行（T4-T5）：`TurnRunner` 总线化 turn 编排（mpsc per-turn 通道，工具边界发 `ToolCallRequest` suspend 于 oneshot，`cancel`/`shutdown` via oneshot，`on_turn_complete` 钩子）；`ToolExecutorModule`（`execute_async` 权限门禁保留，ghost reply 容忍）。
  - 回路与子代理（T6-T7）：`AgentLoopModule` 消费 `UserRequest` → `spawn_turn` + 注册 `agent/cancel` 同步服务；`SubAgentService` async `subagent/dispatch`（fresh session + `spawn_turn` 收集 final_response）+ `Dispatcher` 客户端 `ServiceCall` 封装。
  - 协议接入（T8-T9）：`IpcServer` 全面接总线（`new(kernel, session)`、`run_turn_stream` 发 `UserRequest`、`dispatch_session_run` 映射 `cancelled`→`-32800`、`dispatch_cancel_request` 附加 `agent/cancel`、`kernel/status` 经 `modules()`）；`TcpIpcServer::bind` 签名更新；`CLI` 注册 `ToolExecutorModule`+`AgentLoopModule`+`subagent` 异步服务，`UsageCollector` 经 `TurnRunner::set_on_turn_complete` 接线。
  - 收尾（T10-T13）：移除 `agent_loop.rs` legacy、集成测试迁至 `e2e_streaming_with_real_modules_via_bus`；版本 bump 0.4.1→0.5.0；AGENTS.md/README.md/summary 同步；gap 档案与设计 TODO 完成。
- ✅ **验证门禁全绿**：`cargo test --workspace` ~310 通过（含新增 bus 密封测试：目标路由、async ServiceCall、TurnRunner 取消/广播、agent_loop_module、subagent dispatch；e2e_cli/e2e_permission/e2e_serve 常驻内核密封测试）；`cargo clippy --workspace --all-targets -- -D warnings` 零警告；`cargo fmt --all --check` 干净；`bindings/ts` 构建绿；drift 门禁绿（schemas 零改动，wire 零改动）。
- ✅ 执行方式备注：T1-T2 顺序执行，T3/T4/T5 并行，T6/T7 并行，T8→T9 顺序，T10→T13 顺序；每任务独立子代理 TDD（test first, minimal impl, verify），控制器逐任务验收与门禁检查。

### 遗留问题 / 已知限制

- **单全局 in-flight 会话槽** 仍保留（G2 deferred）：与 v0.4 相同，按会话互斥已由 run-mutex 保证；后续设计再议。
- **`$cancelRequest` 语义**：TurnRunner 取消仅对等待工具 reply 的挂起 turn 生效；model 流式中的取消通过 `cancel_rx` 抢占 `stream.next()` 实现。
- **MCP 空闲回收生产布线** 仍为后台任务（v0.3 遗留 MEDIUM），与 bus 调度无冲突。
- **EventBus 重入 / FileBackedStore tmp 竞争** 等 v0.3 遗留 MEDIUM 项不变。
- **Web Dashboard** 仍为 JSON-RPC 协议消费端临时形态（node:net 外部化，需 Node 环境或 WebSocket 网关）。

### 下一步

- 创建 PR 合并 `feature/kernel-scheduling-refactor`（含 T10 移除旧路径与版本 0.5.0）。
- 进入测试验收（`jskill-test-acceptance` 自动衔接 → 验收 PASS 后自动创建 PR）。
