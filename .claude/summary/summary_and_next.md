# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能设计文档：`docs/superpowers/specs/2026-08-14-agentj-ipc-protocol-design.md`
> 当前实现计划：`docs/superpowers/plans/2026-08-14-agentj-ipc-protocol-plan.md`（11 任务 T0-T10，已执行完成）

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 27 schemas + registry + spec + TS binding，T0-T10，41 测试通过） | 无 | P0 | 已完成 |
| Plan 1 | v0.1 核心 Agent 循环 + 基础模块（`2026-08-09-agentj-v0.1-plan.md`，T0-T18，19 任务，momus 验证 PASS） | 无（需消费 `agentj-protocol` 类型） | P0 | 待开发 |
| Plan 2 | v0.2 生态扩展（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期） | Plan 1 | P1 | — |
| Plan 3 | v0.3 商业扩展（TCP 多连接 IPC / 用量统计 / License / Web Dashboard） | Plan 2 | P2 | — |

## Phase: IPC 协议契约（2026-08-14）✅（本次完成）

### 进度与摘要

- ✅ 设计文档已批准（中文，Mermaid 架构图）：JSON Schema 为权威工件 + 方法注册表 + snake_case wire 字段 + `initialize` 握手版本化 + `$/progress` token 解耦流式 + 错误码分段（agentj 块 `-32800..-32804`）。
- ✅ 实现计划通过 momus 3 轮验证 PASS（修复：T7 方向测试自相矛盾、并行任务 lib.rs 共享 + T4→T5 编译耦合、枚举向前兼容缺失、T0 空 workspace 无法构建）。
- ✅ 开发完成（`feature/ipc-protocol` 分支，11 任务 T0-T10，每任务过 `ajteam-task-reviewer` 门禁 PASS）：
  - `agentj-protocol` leaf crate（serde + schemars，零内核耦合）：JSON-RPC 2.0 信封、错误码、12 个 v0.1 方法 params/result、流式类型
  - 27 个 JSON Schema（`schemas/*.json`，schemars 生成 + 提交件 + 漂移门禁）
  - `protocol/methods.json` 方法注册表（12 方法）
  - `docs/specs/ipc-protocol.md` 人类可读规范（11 节）
  - 契约测试（wire 形状）+ TS 绑定测试（`bindings/ts/`）
- ✅ 验证：`cargo test --workspace` → **41 passed**（31 单元 + 7 契约 + 3 注册表）；`cargo clippy --workspace -- -D warnings` clean。无版本 bump（本阶段建立首个 workspace 0.1.0）。

### 遗留问题 / 已知限制

- **CI 流水线引用未创建 crate**：`smoke.yml` 的 `cargo test -p agentj-core -- smoke` 与 `e2e.yml` 引用 `agentj-core`/`agentj`，内核 crate 尚未创建（v0.1 计划执行后才会存在）——当前状态 CI 冒烟/E2E 会失败，属既有缺口。
- **运行时 IPC 服务器**（initialize 握手强制、方法分发、流式服务器、未知枚举/kind 容忍行为）属 v0.1 `agentj-ipc`（T13/T14），本阶段只立契约。
- **v0.1 协调点**：v0.1 `agentj-core` T2 应从 `agentj-protocol` 导入共享类型（Role/ChatMessage/ToolCall/Attachment/ToolDefinition/SessionId/ModuleState）而非重定义；v0.1 `agentj-ipc` T13/T14 消费 `agentj-protocol` 的信封/方法类型。
- **次要观察**（非阻塞）：`bindings/ts/types.d.ts` 有 6 处孤儿 doc-comment（dedupe 步骤残留，不影响编译）；错误码测试仅断言 7/10 常量；16 MiB 消息上限为加性默认值（`docs/specs/ipc-protocol.md` §2.4）。
- schemars 适配：`schemars::schema::RootSchema` → `schemars::Schema`（schemars 1.2.2 移除了私有 `schema` 模块），已在 T6 实现并验证。

### 下一步

- 运行 `/jskill-feature-dev docs/superpowers/plans/2026-08-09-agentj-v0.1-plan.md`（v0.1 内核），执行前先按「v0.1 协调点」调整 v0.1 计划 T2/T13/T14 以消费 `agentj-protocol` 类型。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
