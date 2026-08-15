# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.1 内核（10 crate）**——实现计划 `docs/superpowers/plans/2026-08-09-agentj-v0.1-plan.md`（25 任务 T0-T24，全部完成并 PASS）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（`agentj-protocol` + 31 schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | 守护内核设计（Connection 契约）| 无（协议契约阶段） | P0 | 设计已批准，契约已随 v0.1 T13/T14 落地 |
| ✅ 完成 | **v0.1 核心内核**（`2026-08-09-agentj-v0.1-plan.md`，25 任务全部 PASS，118 测试） | 无（消费 `agentj-protocol` 类型） | P0 | **已完成（本阶段）** |
| Plan 2 | v0.2 生态扩展（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore） | Plan 1 | P1 | — |
| Plan 3 | v0.3 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License，T0/T3/T4 已改写） | Plan 2 | P2 | — |

## Phase: v0.1 内核（10 crate 核心）实现 ✅（本次完成）

### 进度与摘要

- ✅ **25 任务全部完成并通过 ajteam-task-reviewer 逐任务门禁**（T0-T24；T19 一次 FAIL→修复→复评 PASS）。
- ✅ 10 crate 落地：
  - `agentj-core`：Kernel 调度 + 模块 trait 注册表（Module/ModelProvider/SessionStore/ToolRegistry/ContextPipeline/ConfigProvider + ToolSource 所有权契约）、KernelContext、生命周期状态机（含整体优雅关闭超时）、AgentLoop 流式 API（`run_turn_stream` → `mpsc::Receiver<AgentEvent>` + `run_turn` 聚合，config 驱动 default_model）。
  - `agentj-config`：TOML 分层合并（默认→文件→env），`kind`（默认 openai-compat）+ `default_model`（默认 deepseek-chat）。
  - `agentj-session`：InMemoryStore 生命周期（Active/Archived/Deleted + `SessionError`）+ 快照恢复契约（snapshot/restore/load_all）。
  - `agentj-model`：Mock + 三线 wire-format 提供器（OpenAICompatible SSE / Responses / Anthropic，均含流式 tool-call 组装）+ `models/opencode-go.json` 目录 + OpenCodeGoProvider 门面 + ProviderFactory。
  - `agentj-tools`：5 个内置工具 + ToolExecutor（进程组隔离、超时整树 kill、drop 回收孤儿）。
  - `agentj-context` / `agentj-mcp` / `agentj-ipc` / `agentj-cli`：上下文管线、MCP 客户端桥、Connection 模型 JSON-RPC stdio IPC、CLI run/serve（信号处理）。
- ✅ **验证门禁全绿**：`cargo test --workspace` **118 通过**（agentj-protocol 41 + 其余 9 crate 77）；`cargo clippy --workspace -- -D warnings` 零警告；`cargo build --workspace` 通过；`cargo test -p agentj-cli` e2e 2 通过（session/new + SIGTERM 优雅退出）；`cargo bench -p agentj-model` 运行（`mock_stream_10_events`）。
- ✅ 版本：v0.1.0（初始发布，无 bump）；AGENTS.md / README.md 已同步（crate 列表、Features、项目结构树、Usage、config 示例）。

### 遗留问题 / 已知限制

- **T13 真实 I/O 缺陷修复**（`84a522c`，T14 e2e 暴露后由 T14 跨 crate 修复并复评 PASS）：`StdioConnection::send` 由 block_on 改 std 阻塞 stdout；`serve_stdio` 增加 `kernel_finished` 守卫防 JoinHandle 重复 poll；CLI 用 `std::process::exit(0)` 规避 runtime drop 挂起（信号路径下 stdin 阻塞线程）。
- **已知缺口（AGENTS.md 记录）**：e2e 流水线 `--test e2e` 目标名与 `agentj-cli/tests/e2e_cli.rs` 不一致（`e2e_cli`），Mock LLM 密封测试接入时对齐。
- **`agentj-config`/`agentj-context` 测试模块遗留 unused import 警告**（`std::io::Write`、`types::*`、`ContextPipeline`）——仅出现在 `--all-targets` 测试目标下，默认 clippy 门禁不触发；CI `--all-targets` 需注意。
- **v0.2 依赖**：FileBackedStore、McpConnectionManager（长驻连接）、Hook System 等均需 v0.2 计划执行。
- **Windows 限制**：ToolExecutor 进程组 kill 为已知限制（设计遗留）；`kill_on_drop` 单 pid kill 不足以覆盖进程组（保留 `kill(-pgid, SIGKILL)`）。

### 下一步

- 运行 `/jskill-test-acceptance` 启动 v0.1 测试验收流程（测试 → 修复 → 回归验证）。
- 随后运行 `/jskill-feature-next` 识别 v0.2 下一个功能（Hook / ModuleResolver / Sub-Agent / Skills / Plugin / Ollama / MCP 长驻连接 / Session 过期 / FileBackedStore），进入 design → plan → dev 管线。
- 每阶段完成后：运行测试 + lint 门禁 → 版本号 bump（v0.1.0 → v0.2.0）→ 自动同步 AGENTS.md / README.md / 本文件（仅保留当前功能内容）→ 更新「待实施」表。
