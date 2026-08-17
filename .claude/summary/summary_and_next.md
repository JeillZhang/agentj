# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.3 生态扩展**——实现计划 `docs/superpowers/plans/2026-08-09-agentj-v0.2-plan.md`（12 任务 T0-T11，全部完成并通过逐任务评审 + 三分区终审；版本 0.2.1 → 0.3.0）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 31 schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.1 核心内核**（25 任务全部 PASS，v0.1.1 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.2 权限审批系统**（13 任务 T0-T12 全部 PASS，v0.2.1 发布） | v0.1 内核 | P0 | 已完成 |
| ✅ 完成 | **v0.3 生态扩展**（`2026-08-09-agentj-v0.2-plan.md`，T0-T11 全部 PASS：事件总线 + ModuleResolver/ServiceCall + 子代理调度 + skills 加载 + 插件系统 + LibloadingLoader + Ollama provider + MCP 长驻连接 + 会话过期 + FileBackedStore + CLI 布线；**v0.3.0 发布，233 测试全绿**） | v0.1+v0.2 | P1 | **已完成（本阶段）** |
| Plan 3 | **v0.4 商业扩展**（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: v0.3 生态扩展 开发完成 ✅（本次完成）

### 进度与摘要

- ✅ **12 任务全部 PASS**：T0 事件总线（EventBus publish/subscribe + 9 个 v0.3 EventType 变体 + Kernel 生命周期事件发布）、T1 ModuleResolver 服务定位（ServiceRegistry + `KernelMessage::ServiceCall` 唯一允许的 breaking 变更 + ToolRegistry source-enable 原语）、T2 子代理调度器（Dispatcher 并行 AgentLoop + 独立会话）、T3 agentj-skills 多路径优先级加载（.aj > .agents > .claude）、T4 agentj-plugin 核心（manifest/PluginLoader/PluginInstance）、T5 插件生命周期状态机 + 依赖图（拓扑 init / 逆拓扑 shutdown / DependencyCycle / ToolConflict / set_plugin_enabled）、T6 LibloadingLoader + cdylib fixture（ABI 版本守卫 + Box 所有权转移）、T7 OllamaProvider（NDJSON SSE）、T8 McpConnectionManager 长驻连接（传输线程模型 + 引用计数 + 空闲回收 + 指数退避重连 + ghost-tool 级联清理）、T9 会话过期 TTL 扫描（SessionExpired 事件）、T10 FileBackedStore 磁盘持久化（原子写 + .corrupt 备份 + 字符集守卫）、T11 CLI 布线 + 版本 0.3.0 + 文档同步。
- ✅ **逐任务评审全部 PASS**（T8 1 轮修复：函数拆分 ≤50 行；T11 1 轮修复：AGENTS.md ModuleResolver 注释 + crate 数 13→14）；**三分区终审 PASS**（无 CRITICAL/HIGH，MEDIUM 发现已记录）。
- ✅ **验证门禁全绿**：`cargo test --workspace` **233 通过**（protocol 47 + permission 40 + core 51 + cli 6 + 其余 9 crate 89）；`cargo clippy --workspace -- -D warnings` 与 `--all-targets` 零警告；`cargo fmt --all --check` 通过；`cargo test -p agentj-protocol -- drift` 通过（2 项）；`cargo build --workspace --locked` + `--release` 通过；e2e（`--release --test e2e_cli --test e2e_permission`）3 通过。
- ✅ 版本：**v0.2.1 → v0.3.0**（feature minor bump）；协议无 wire 变更（无 schema/methods 变化）；14 crates（+ agentj-skills、agentj-plugin、agentj-plugin-fixture dev-only）。

### 遗留问题 / 已知限制

- **MCP 空闲回收在生产布线中不可达**（终审 MEDIUM）：`McpConnectionManager` 引用计数设计（connect 初始 refs=1）与真实使用路径（`McpSharedTool::execute` 不 acquire/release）脱节，reap_idle 的 `refs==0` 门永不触发；且 CLI 布线无 `[mcp]` 配置、无 connect 调用——长驻 MCP 能力当前仅库级可用（测试覆盖），二进制内不可达。**修复方向**：T11 后补 config 接线 + 引用计数语义对齐（execute 期间 refs++ 或 reaper 改为 idle_since 门控）。
- **并发 execute 失败 → 双 reconnect**（终审 MEDIUM）：同一 server 的两个并发工具调用同时失败会触发两条独立 reconnect 路径（瞬态双进程，kill_on_drop 清理输家）——自愈但浪费，无 in-flight 去重。
- **EventBus handler 重入死锁风险**（终审 MEDIUM）：`publish` 持有 handlers Mutex 期间调用 `on_event`，handler 内 re-enter publish/register 会死锁；handler panic 会传播到发布方任务并永久毒化总线。当前无 handler 重入，属文档化风险，修复方向 = 先克隆 handler 列表再释放锁。
- **FileBackedStore 共享 tmp 路径竞争**（终审 MEDIUM）：同一会话并发 append 时两个 persist 竞争 `<sid>.json.tmp`（瞬态 ENOENT 或磁盘 lost-update）；单客户端 stdio 拓扑下不会触发。修复方向 = per-sid 写锁或唯一 tmp 名。另：rename 后无目录 fsync（持久性弱化，LOW）。
- **ModuleRegister::with_services 未接线**（parked，计划 T11 显式延后）：CLI 仍用 `new(...)`，模块经 register 注册的 service 不可达 `Kernel::resolve`/ServiceCall——无 v0.3 模块注册服务，属潜伏陷阱，计划注释已说明切换时机。
- **ServiceCall 在 kernel 循环内同步执行**（终审 MEDIUM/LOW）：慢服务会阻塞全部 kernel 消息广播——当前无真实服务，文档化限制。
- **v0.4 前置**：守护内核（daemon-kernel）设计已批准，其 G3 历史级接续依赖 FileBackedStore（已落地）；T0/T3/T4 已按 Connection 模型改写。

### 下一步

- 运行 `/jcmd-test-acceptance` 启动测试验收流程（测试 agent 发现 Bug → Bug-fix agent 修复 → 回归验证），验收通过后创建 PR。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
