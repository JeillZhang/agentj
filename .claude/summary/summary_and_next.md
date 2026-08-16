# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.2 权限审批系统**——实现计划 `docs/superpowers/plans/2026-08-15-agentj-permission-plan.md`（13 任务 T0-T12，全部完成并通过评审；v0.2.0 发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 31 schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.1 核心内核**（`2026-08-09-agentj-v0.1-plan.md`，25 任务全部 PASS，测试验收通过，v0.1.1 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.2 权限审批系统**（`2026-08-15-agentj-permission-plan.md`，13 任务 T0-T12 全部 PASS；权限闭环：per-tool Allow/Ask/Deny + path/command rules + IPC 审批流） | v0.1 内核 | P0 | **已完成（本阶段）** |
| Plan 2 | v0.2 生态扩展（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore） | Plan 1 | P1 | — |
| Plan 3 | v0.3 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: v0.2 权限审批系统（13 任务）开发完成 ✅（本次完成）

### 进度与摘要

- ✅ **权限闭环落地（v0.2.0，feature 版本 bump）**：`agentj-permission` leaf crate（决策引擎 + 策略模型 + PermissionModule）+ `PermissionChecker` 能力 trait（agentj-core）+ `ToolRegistry::execute_async` 单一拦截点 + AgentLoop 路由 + `$/permissionRequest` / `permission/respond` 协议方法 + `[permission]` 配置段 + CLI 接线 + E2E 测试。
- ✅ **13 任务 T0-T12 全部完成并通过 per-task 评审**（T9 经 1 轮 fix 后通过）；**最终全分支评审发现 1 CRITICAL（H1）+ 4 MEDIUM（M1-M4）+ 4 minor，全部修复并经 scoped re-review 通过**：
  - H1（CRITICAL）：bash 参数字段不匹配——工具 schema 用 `"command"`，权限引擎读 `"cmd"` → deny 规则对真实模型调用不生效、allow_always 指纹塌缩为会话级 bash 通配符。修复：引擎读 `"command"`（兼容 `"cmd"` 回退），mock/E2E 改用真实 schema 字段，新增回归测试。
  - M1：无 id 通知（如 `$/cancelRequest`）不再返回 JSON-RPC 响应（§2.1）；M2：`eval_external` 尊重 Deny（Deny > Allow > Ask 覆盖 MCP/plugin 工具）；M3：glob/grep 的 `pattern`/`include` 参数纳入 path 规则求值 + 词法逃逸检查；M4：README 配置示例字段 `pattern` → `prefix`（字面前缀非正则）。
- ✅ **验证门禁全绿**：`cargo test --workspace` **185 通过**（协议 47：37 单元 + 7 契约 + 3 注册表；permission 34；其余 9 crate 104）；`cargo clippy --workspace -- -D warnings` 零警告；`cargo fmt --all --check` 零 diff；`cargo test -p agentj-protocol -- drift` 通过（31 schemas）；E2E `cargo test --release --test e2e_permission -- --ignored` 通过 + `e2e_cli` 2 通过。
- ✅ 版本：**v0.1.1 → v0.2.0**（feature）；`protocol/methods.json` protocol_version 同步 0.2.0（14 方法）；AGENTS.md / README.md / 协议规范已同步（T12 + 本阶段计数修正）。

### 遗留问题 / 已知限制

- **stdio 顺序 serve 循环无法交互式审批**：v0.1 `serve_stdio` 单线程顺序处理（读一行 → 处理 → 响应），`session/run` 等待审批期间无法读取 stdin 上的 `permission/respond`——交互式 respond 路径仅在 handler 级单元测试覆盖；E2E 走 60s 超时自动 Deny 路径。**需 v0.3 守护内核并发派发（daemon-kernel 设计已批准）解决**，届时恢复「respond deny → final response 含 denied」字面断言。
- **pending 超时条目不回收**：`await_approval` 超时分支不移除 pending map 条目（仅 respond/cancel/shutdown 移除）——v0.2 CLI 短生命周期可接受；**v0.3 常驻 daemon 前必须修**（超时分支内加移除，map 锁内安全）。
- **非 bash 工具 allow_always 指纹塌缩**（H1 同族，评审记录 NEW-2）：`rule_key_for` 对非 bash 工具仅取 `args["path"]`，glob/grep（`pattern`/`include` 参数）的 allow_always 会塌缩为 `("glob","")`/`("grep","")` 会话级通配——**待后续修复**（与 H1 相同的 `or_else` 回退模式即可）。
- **`$/cancelRequest` 可达性**：IPC 层已接线（T9 fix 记录 in-flight session），但 stdio 顺序模型下取消与审批竞争窗口极小；多客户端模型下需按 v0.3 设计重审。
- **Windows 限制**：ToolExecutor 进程组 kill 为已知限制（设计遗留）。
- **多 provider 路由**：ProviderRegistry `get_default` 恒返回首个 provider，多 provider 按 default_model 路由属 v0.2 生态范围。

### 下一步

- 运行 `/jskill-test-acceptance` 启动测试验收流程（测试 agent 发现 Bug → Bug-fix agent 修复 → 回归验证）。
- 验收通过后运行 `/jskill-feature-next` 识别 v0.2 下一个功能（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore），进入 design → plan → dev 管线。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
