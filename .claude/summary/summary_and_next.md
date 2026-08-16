# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.2 权限审批系统**——实现计划 `docs/superpowers/plans/2026-08-15-agentj-permission-plan.md`（13 任务 T0-T12，全部完成并通过评审；测试验收通过，v0.2.1 发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 31 schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.1 核心内核**（`2026-08-09-agentj-v0.1-plan.md`，25 任务全部 PASS，测试验收通过，v0.1.1 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | **v0.2 权限审批系统**（`2026-08-15-agentj-permission-plan.md`，13 任务 T0-T12 全部 PASS；权限闭环：per-tool Allow/Ask/Deny + path/command rules + IPC 审批流；**测试验收通过，3 Bug 修复 BUG-001/002/003，v0.2.1 发布**） | v0.1 内核 | P0 | **已完成（本阶段）** |
| Plan 2 | **v0.3 生态扩展**（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore，计划文档 `2026-08-09-agentj-v0.2-plan.md`） | Plan 1 | P1 | — |
| Plan 3 | **v0.4 商业扩展**（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: v0.2 权限审批系统 测试验收完成 ✅（本次完成）

### 进度与摘要

- ✅ **测试验收通过（v0.2.1，bugfix 版本 bump）**：`/jskill-test-acceptance` 全流程（测试 agent 发现 3 Bug → 逐 Bug 修复 → 回归验证 → 全 verified），修复提交：`23f34fb`（BUG-001）、`bf71ab1`（BUG-002）、`f323072`（BUG-003），每 Bug 恰好一提交。
- ✅ **BUG-001（medium）**：grep `pattern`（正则搜索词）被当作文件路径参与 path_rules 匹配 → 合法搜索误判 Deny。修复：`path_arg_keys` 按工具绑定路径语义键（grep→`include`、glob→`pattern`、read/write→`path`），新增 3 回归测试。
- ✅ **BUG-002（medium）**：`rule_key_for` 对非 bash 工具仅取 `args["path"]`，grep/glob 无 `path` 字段 → allow_always 指纹坍缩为 `(tool,"")` 会话级通配授权。修复：指纹键工具相关化（grep→pattern+include、glob→pattern、bash→command、read/write→path），新增 4 回归测试。（即评审遗留 NEW-2，本阶段闭环）
- ✅ **BUG-003（low）**：`e2e_permission` 测试被 `#[ignore]` 且未接入任何 CI workflow → 权限 e2e 覆盖永不执行。修复：移除 `#[ignore]` + e2e.yml 增加 `--test e2e_permission`（Mock 密封测试）。
- ✅ **验证门禁全绿**：`cargo test --workspace` **192 通过**（protocol 47：37 单元 + 7 契约 + 3 注册表；permission 40；agentj-cli e2e_permission 1；其余 9 crate 104）；`cargo clippy --workspace -- -D warnings` 与 `--all-targets` 零警告；`cargo build --workspace` 通过；`cargo test -p agentj-protocol -- drift` 通过（31 schemas）；`scripts/validate-workflow.sh e2e.yml` PASS。
- ✅ 版本：**v0.2.0 → v0.2.1**（bugfix）；协议无 wire 变更，`protocol/methods.json` protocol_version 保持 0.2.0（14 方法）；AGENTS.md / README.md / 本文件已同步。

### 遗留问题 / 已知限制

- **stdio 顺序 serve 循环无法交互式审批**：v0.1 `serve_stdio` 单线程顺序处理（读一行 → 处理 → 响应），`session/run` 等待审批期间无法读取 stdin 上的 `permission/respond`——交互式 respond 路径仅在 handler 级单元测试覆盖；E2E 走 60s 超时自动 Deny 路径。**需 v0.3 守护内核并发派发（daemon-kernel 设计已批准）解决**，届时恢复「respond deny → final response 含 denied」字面断言。
- **pending 超时条目不回收**：`await_approval` 超时分支不移除 pending map 条目（仅 respond/cancel/shutdown 移除）——v0.2 CLI 短生命周期可接受；**v0.3 常驻 daemon 前必须修**（超时分支内加移除，map 锁内安全）。
- **`$/cancelRequest` 可达性**：IPC 层已接线（T9 fix 记录 in-flight session），但 stdio 顺序模型下取消与审批竞争窗口极小；多客户端模型下需按 v0.3 设计重审。
- **Windows 限制**：ToolExecutor 进程组 kill 为已知限制（设计遗留）。
- **多 provider 路由**：ProviderRegistry `get_default` 恒返回首个 provider，多 provider 按 default_model 路由属 v0.3 生态范围。

### 下一步

- 运行 `/jskill-feature-next` 识别 v0.3 下一个功能（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore），进入 design → plan → dev 管线。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump → 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
