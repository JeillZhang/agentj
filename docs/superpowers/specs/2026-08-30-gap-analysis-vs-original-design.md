# 差距分析：当前实现 vs 最初设计目标（存档）

> **背景**：2026-08-30 对 AgentJ v0.4.1 实现与最初设计目标（`2026-08-09-agentj-core-design.md` / `2026-08-14-agentj-ipc-protocol-design.md` / `2026-08-15-agentj-daemon-kernel-design.md`）做系统性差距核查，三路并行 explore agent 取证（文件/行号/代码摘录）。本文件为**持久化存档**：记录全部差距项 G1-G8、取证证据与状态。G1 已立项设计（见 `2026-08-30-agentj-kernel-scheduling-refactor-design.md`）；G2-G8 留待后续设计，后续接手者直接以本文件为入口，无需重新勘察。

## 核查方法

- 三路并行 `explore` agent：
  - ① 内核调度架构现状（KernelMessage 变体 / AgentLoop 调用链 / Sub-Agent 总线化 / 变体接线 / IPC run 路径）
  - ② inflight 会话槽 / UsageCollector / provider usage 携带 / License 依赖面
  - ③ MCP 空闲回收生产布线 / EventBus 重入 / FileBackedStore 原子写与生产使用 / Web Dashboard 传输形态
- 关键文件人工复核：`agent_loop.rs` / `kernel.rs` / `dispatcher.rs` / `module.rs` / `resolver.rs`

## 差距全景

| # | 差距 | 设计宣称 | v0.4.1 实测现状 | 证据 | 严重度 | 状态 |
|---|------|---------|----------------|------|--------|------|
| **G1** | 内核调度（决策 #18） | "内核调度 + 可插拔模块"——AgentLoop/Sub-Agent 全量迁入 `KernelMessage` 总线 | `KernelMessage` 仅 3 变体；`ModelResponse/ToolCallRequest/ToolCallResult` **代码中不存在**（0 命中，仅设计稿 L644）；IPC `session/run` 直调 `agent_loop.run_turn_stream`（`agentj-ipc/src/lib.rs:613`）；`Dispatcher` 直建 AgentLoop（`dispatcher.rs:53`）；CLI `AgentLoopModule::handle()` 空实现（`agentj-cli/src/main.rs:61-65`） | `module.rs:559-574` / `kernel.rs:149-191` / `agent_loop.rs:125-207` | 🔴 核心理念未兑现 | ✅ 已实现（`docs/superpowers/plans/2026-08-30-agentj-kernel-scheduling-refactor-plan.md` v0.5.0，T1-T13） |
| **G2** | 单全局 in-flight 会话槽 | 并发 run 于不同会话互不干扰 | 互斥已 per-session（`pending_runs: HashSet<SessionId>` ✓）；但 `IpcServer.session: Arc<Mutex<Option<String>>>` 单全局槽——`permission/respond` 属主门控（`lib.rs:670-684`）与 `$/cancelRequest`（`lib.rs:722-725`）读最近一次覆盖值，并发跨会话 run 互相污染 | `agentj-ipc/src/lib.rs:282-285,595,670-684,722-725` | 🟠 并发正确性 | ✅ 已实现（design + plan + impl，v0.6.0，见 [`2026-08-31-g2-per-session-inflight-routing-design.md`](2026-08-31-g2-per-session-inflight-routing-design.md) + [`2026-08-31-g2-per-session-inflight-routing-plan.md`](../plans/2026-08-31-g2-per-session-inflight-routing-plan.md)） |
| **G3** | token 级用量 | UsageCollector 统计 tokens | `record_token_usage` 为死代码（仅单测）；`OnTurnComplete` 只传 `&SessionId`（`agent_loop.rs:27`）；**6 个 provider 全部丢弃 SSE 末帧 usage**——`ModelEvent` 无 Usage 变体、`ChatResponse` 无 usage 字段（`types.rs:76-87`）；CLI 仅 `record_turn`（`main.rs:259-268`） | `agentj-stats/src/lib.rs:8-78` / `agent_loop.rs:27,55` / `openai_compat.rs:110-193` 等 | 🟠 统计不完整 | 待设计 |
| **G4** | 磁盘持久化生产未接线 | v0.3"FileBackedStore 磁盘持久化已落地" | CLI `main.rs:223` 用 `InMemoryStore::new()`——**FileBackedStore 生产零引用**（全仓 grep 仅 lib.rs pub use + 定义 + 测试）；守护内核 G3「历史级接续」依赖落空；另有 tmp 固定命名 `<sid>.json.tmp`（`file_store.rs:130`）跨进程写同 session 互相覆盖，无唯一命名/`flock` | `agentj-cli/src/main.rs:223` / `agentj-session/src/file_store.rs:14-140` | 🔴 宣称≠实际 | 待设计 |
| **G5** | License 商用门控 | agentj-license 能力 | API 完备（`LicenseValidator::validate` / `check_entitlement`）但**全仓零生产依赖**（`agentj-cli/Cargo.toml` 无 agentj-license）；design-pending 悬置 | `agentj-license/src/lib.rs:8-75` | 🟠 商业件悬空 | 待设计 |
| **G6** | MCP 生产接线 | MCP 长驻连接 + 空闲回收 | 回收循环**已布线**（`main.rs:297-312` tokio::spawn 10s 轮询 + 60s 超时，修复了旧"不可达"）；但 `[[mcp.servers]]` 配置发现→`connect` 链路缺失，`connections` 恒空、回收空转 | `agentj-cli/src/main.rs:297-312` / `connection_manager.rs:248-350` | 🟠 特性空转 | 待设计 |
| **G7** | EventBus 重入 | 事件总线 | 死锁已修复（快照发布，`hooks.rs:56-65`，BUG-001 已验）；但**无递归深度防护**——handler 自触发同类型事件形成无限递归栈溢出（`hooks.rs:50-67` 同步发布） | `agentj-core/src/hooks.rs:18-76` | 🟡 设计债 | 待设计 |
| **G8** | WebSocket 网关 | 设计明确列为非目标（预留） | Dashboard `RpcClient` 硬编码 `node:net` TCP（`jsonrpc.ts:1,108-121`）；vite 无 external/polyfill，bundle 仍含 `createConnection`——浏览器打开即 `TypeError`；全仓无 `ws`/`WebSocket` 命中 | `web-dashboard/src/jsonrpc.ts` / `vite.config.ts` / `dist/assets/*.js:10` | 🟡 形态限制（设计已声明延期） | 待设计 |

## G1 摘要（已立项，详见独立设计文档）

- **目标**：兑现决策 #18——`KernelMessage` 增加 `ModelResponse` / `ToolCallRequest` / `ToolCallResult` 变体；AgentLoop 降为 KernelModule（`run_inner` 直调逻辑状态机化 + `handle()` 事件驱动）；Sub-Agent 接入总线；内核新增 request_id→订阅者路由层（镜像 `$/progress` token 解耦）；`UserRequest` 生产路径接线。
- **边界**：协议面零变化（`session/run` 一请求一最终响应 + `$/progress` 流式保留）；权限门保留在 `ToolRegistry::execute_async`；`ServiceCall` 通道与 `Shutdown` 优先检测保留。
- **前置就绪**：`ModuleResolver` + `ServiceCall`（v0.3 T1）已在；v0.4 ConnectionRegistry 订阅广播在 IPC 层（需与内核路由层对齐语义，见设计文档）。

## G2-G8 待设计提示（后续接手者入口）

每项均含具体改造方向（来自取证结论）：

- **G2**（✅ 已实现，v0.6.0，见 [`2026-08-31-g2-per-session-inflight-routing-design.md`](2026-08-31-g2-per-session-inflight-routing-design.md) + [`2026-08-31-g2-per-session-inflight-routing-plan.md`](../plans/2026-08-31-g2-per-session-inflight-routing-plan.md)）：删除全局槽 → cancel 走 wire `session_id`（加性，`pending_runs` 单会话兜底）、respond 走 `PermissionModule::session_of(rid)` 查表 + `session_owner` 门控；`$/permissionRequest` 加性携带 `session_id`。
- **G3**：`types.rs` 扩展 `ModelEvent::Usage{prompt,completion}`（或 `Done` 携带）；6 provider 解析 SSE 末帧 `usage`；`OnTurnComplete` 改 `Fn(&SessionId, Option<SessionUsage>)`；CLI 改调 `record_token_usage`。
- **G4**：CLI 切换 `InMemoryStore` → `FileBackedStore::new(&xdg_data_dir)` + `load_all` 恢复；`persist` 改唯一 tmp 命名（`sid.<uuid>.tmp`）+ 目录 fsync 或 `fs2` flock。
- **G5**：CLI 注入 `LicenseValidator`，`session/run`/`kernel/status`/dashboard 前 `check_entitlement`；`--license-key` 配置。
- **G6**：`[[mcp.servers]]` 配置解析 → 启动 `mcp_manager.connect` + 关闭 `close`，复用已有 `reap_idle` 循环。
- **G7**：递归深度 guard（thread_local depth<16）+ 循环检测。
- **G8**：方案 A 新增 `agentj-ipc` WebSocket `Connection` 实现 + 前端 `WebSocketTransport`（推荐）；方案 B Electron 壳 + `ssr.external` 明示 Node 运行时。

## 后续开发流程

- 下一份设计：`docs/superpowers/specs/2026-08-30-agentj-kernel-scheduling-refactor-design.md`（G1）。
- 差距项状态更新：每完成一项设计/落地，将本表对应行状态改为「✅ 已设计 / ✅ 已实现」并附设计文档链接。