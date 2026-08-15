# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.1 内核（10 crate）**——实现计划 `docs/superpowers/plans/2026-08-09-agentj-v0.1-plan.md`（25 任务 T0-T24，全部完成并 PASS；测试验收通过，v0.1.1 发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 31 schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | 守护内核设计（Connection 契约）| 无（协议契约阶段） | P0 | 设计已批准，契约已随 v0.1 T13/T14 落地 |
| ✅ 完成 | **v0.1 核心内核**（`2026-08-09-agentj-v0.1-plan.md`，25 任务全部 PASS，121 测试 + 验收通过） | 无（消费 `agentj-protocol` 类型） | P0 | **已完成（本阶段）** |
| Plan 2 | v0.2 生态扩展（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore） | Plan 1 | P1 | — |
| Plan 3 | v0.3 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: v0.1 内核（10 crate 核心）测试验收 ✅（本次完成）

### 进度与摘要

- ✅ **测试验收通过（v0.1.1）**：`/jskill-test-acceptance` 完整跑通「测试 → 修复 → 回归」循环，**1 轮测试发现 7 个 Bug → 逐 Bug 修复（每 Bug 一提交）→ 回归第 1 轮全部 verified，零新 Bug**。
- ✅ **修复的 7 个 Bug**（`fix: BUG-00N` 提交）：
  - `9d5c535` BUG-001：CI `cargo fmt --all --check` 失败（19 个文件未 rustfmt，全仓格式化）
  - `72515cd` BUG-002：CI `clippy --all-targets` 失败（3 处测试模块 unused import）
  - `be66909` BUG-003：e2e.yml `--test e2e` 目标名 → `e2e_cli`
  - `0344fc8` BUG-007：e2e.yml `-- --ignored` 与测试未标 `#[ignore]` 不一致 → e2e 静默空跑 0 测试
  - `80c167b` BUG-004：JSON-RPC 错误响应丢失请求 `id` + 错误码错误（统一回显 id，-32602/-32601/-32603 语义化）
  - `eb1bc8a` BUG-005：InMemoryStore 锁顺序不一致 ABBA 死锁（统一 `statuses → sessions` 顺序 + 并发回归测试）
  - `bf34b55` BUG-006：README/AGENTS.md 声称「16 方法」→ 实际 12（文档对齐机器注册表）
- ✅ **验证门禁全绿**：`cargo test --workspace` **121 通过**（+3：BUG-004 ipc 错误路径 2 个 + BUG-005 session 并发 1 个）；`cargo clippy --workspace --all-targets -- -D warnings` 零警告；`cargo fmt --all --check` 零 diff；`cargo build --workspace` 通过；`cargo test -p agentj-cli` e2e 2 通过；`cargo test -p agentj-protocol -- drift` 通过；`cargo test --release --test e2e_cli` 2 passed（e2e 流水线真实执行）。
- ✅ 版本：**v0.1.0 → v0.1.1**（bugfix → patch）；AGENTS.md / README.md 已同步（版本、测试计数、e2e 命令、方法计数 12）。

### 遗留问题 / 已知限制

- **Windows 限制**：ToolExecutor 进程组 kill 为已知限制（设计遗留）；`kill_on_drop` 单 pid kill 不足以覆盖进程组（保留 `kill(-pgid, SIGKILL)`）。
- **已消除的旧缺口**：e2e 目标名不一致（BUG-003）、`--all-targets` unused import（BUG-002）、e2e `--ignored` 空跑（BUG-007）均已修复并回归验证。
- **v0.2 依赖**：FileBackedStore、McpConnectionManager（长驻连接）、Hook System、subscribe/permission 协议方法（12→16）等均需 v0.2 计划执行。
- **多 provider 路由**：ProviderRegistry `get_default` 恒返回首个 provider，v0.1 单 provider 无影响，多 provider 按 default_model 路由属 v0.2 范围。

### 下一步

- 运行 `/jskill-feature-next` 识别 v0.2 下一个功能（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore），进入 design → plan → dev 管线。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump（v0.1.1 → v0.2.0）→ 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
