# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能设计文档：`docs/superpowers/specs/2026-08-15-agentj-daemon-kernel-design.md`
> 当前实现计划：`docs/superpowers/plans/2026-08-15-agentj-daemon-kernel-plan.md`（12 任务 T1-T12，momus 2 轮验证 PASS）

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 27 schemas + registry + spec + TS binding，41 测试通过） | 无 | P0 | 已完成 |
| ✅ 完成 | 守护内核设计 + 协议契约（`2026-08-15-agentj-daemon-kernel-design.md` + Part A 契约扩展 16 方法 / 31 schemas） | 无（协议契约阶段） | P0 | 已完成（Part B 文档对齐 T9-T12 已执行） |
| Plan 1 | v0.1 核心 Agent 循环 + 基础模块（`2026-08-09-agentj-v0.1-plan.md`，T13/T14 已按 Connection 模型改写，momus 验证 PASS） | 无（需消费 `agentj-protocol` 类型） | P0 | 待开发 |
| Plan 2 | v0.2 生态扩展（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore） | Plan 1 | P1 | — |
| Plan 3 | v0.3 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: 守护内核（常驻 API 服务）设计 + 协议契约（2026-08-15）✅（本次完成）

### 进度与摘要

- ✅ 设计批准（中文，Mermaid 架构图）：统一 `Connection` 传输抽象（stdio/TCP/Unix socket），契约 v0.1 固化、能力 v0.3 落地；`agentj serve --port` 按需常驻内核多客户端共享（不做 OS 级守护，生态主流为可选 server 模式）；运行级接管 = 广播订阅 + 单发起方控制；权限审批定向发给发起方。
- ✅ 决策记录（6 项）：D 三者都要 / A 按需常驻 / A 广播订阅 / A 定向审批 / A' 契约合并进 v0.1 / B 统一传输抽象。
- ✅ 实现计划通过 momus 2 轮验证 PASS（修复：G3/G5 显式 delegate v0.2、T10 权限边缘语义补充）。
- ✅ Part A 协议契约（守护内核计划 T1-T8 定义，待 feature-dev 执行）：
  - 4 新 wire 类型：`SessionSubscribeParams` / `PermissionRequestParams` / `PermissionDecision` / `PermissionRespondParams`
  - 错误码 `-32805 SessionBusy` / `-32806 NotAuthorized`；`InitializeResult.connection_id` + `capabilities.multi_client`
  - 注册表 12 → 16 方法（`session/subscribe` / `session/unsubscribe` / `permission/respond` / `$/permissionRequest`）
- ✅ Part B 文档对齐（T9-T12 已执行提交）：
  - v0.1 计划 T13/T14 改为 Connection 模型（`ea302da`）
  - v0.3 计划 T0/T3/T4 改为多客户端常驻（`ce0dfb6`）
  - `docs/specs/ipc-protocol.md` 升级 v0.2.0（16 方法 + 新错误码 + shutdown 多连接语义，`2f60762`）
  - AGENTS.md / README.md / summary 同步（本提交）

### 遗留问题 / 已知限制

- **协议契约代码未落地**：Part A（T1-T8：新 wire 类型、schema、注册表、契约测试、TS 绑定）为计划定义，尚未进入 `/jskill-feature-dev` 执行——`schemas/` 仍为 27 个、注册表仍为 12 方法、TS 绑定未更新。
- **CI 流水线引用未创建 crate**：`smoke.yml` 的 `cargo test -p agentj-core -- smoke` 与 `e2e.yml` 引用 `agentj-core`/`agentj`，内核 crate 尚未创建（v0.1 计划执行后才会存在）——既有缺口。
- **G3/G5 委托 v0.2**：历史级接续（FileBackedStore）与资源级复用（McpConnectionManager）显式 delegate 到 v0.2 计划，本特性不实现。
- **运行级接管范围**：只做「广播订阅 + 单发起方控制」；控制权转移（暂停/接管/重定向）属后续独立设计。
- **v0.1 协调点**：v0.1 `agentj-core` T2 应从 `agentj-protocol` 导入共享类型而非重定义；v0.1 `agentj-ipc` T13 按 Connection 模型构建（T14 CLI 装配不变）。

### 下一步

- 运行 `/jskill-feature-dev docs/superpowers/plans/2026-08-15-agentj-daemon-kernel-plan.md`（守护内核计划，先执行 Part A T1-T8 协议契约代码）。
- 随后运行 `/jskill-feature-dev docs/superpowers/plans/2026-08-09-agentj-v0.1-plan.md`（v0.1 内核，T13/T14 已按 Connection 模型改写）。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
