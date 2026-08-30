# AGENTS.md

> Updated 2026-08-30 — v0.5.0 released (16 crates，内核调度全重构：KernelMessage 总线化（UserRequest+ToolCallRequest 目标路由+ServiceCall 同步/异步）、TurnRunner 总线化 turn 编排、ToolExecutorModule/AgentLoopModule 内核模块化、子代理经 AsyncKernelService `subagent/dispatch`、IpcServer/CLI 全面接总线；v0.4 全保留：守护内核多客户端常驻 TCP `serve --port`/`run --attach`/`dashboard`、方法面 16、$/progress 广播+权限属主路由、agentj-stats 用量统计、agentj-license 校验、Web Dashboard）

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。v0.5 已落地：**内核调度全重构**（决策 #18 兑现）——`KernelMessage` 混合总线（`UserRequest{reply:mpsc::Sender<AgentEvent>}` + `ToolCallRequest{target,reply:ToolCallReply}` 目标路由 + `ServiceCall` 同步/异步双分支（`AsyncKernelService`）+ `Shutdown`）、`TurnRunner` 总线化 turn 编排（工具边界发 `ToolCallRequest` 并 suspend 于 oneshot，model deltas 走 per-turn `mpsc` 通道）、`ToolExecutorModule`（`execute_async` 权限门禁保留）、`AgentLoopModule`（`UserRequest` 消费 + `agent/cancel` ServiceCall）、`Dispatcher` 子代理经 `subagent/dispatch` 异步服务、`IpcServer`/`CLI` 全面接总线（`session/run`→`UserRequest`、`$/cancelRequest`→`agent/cancel`、`kernel/status` 经 `Kernel::modules()`）、`AgentEvent`/`OnTurnComplete` 迁至 `types.rs` 并由旧路径 re-export 至 T10 移除；v0.4 全保留：**守护内核**（统一 Connection 抽象：stdio 哨兵 + `TcpConnection`/`TcpIpcServer` 多客户端常驻，`--token`/`--max-connections`）、**多客户端协议分发**（initialize token 认证 + run 会话互斥 `-32805` + permission 属主门控 `-32806` + `session/subscribe`/`session/unsubscribe` 广播订阅）、**`$/progress` 进度广播**、**权限属主定向路由**（`origin_client`）、**agentj-stats 用量统计**（UsageCollector，经 TurnRunner on-turn-complete 接线）、**agentj-license**（Ed25519 许可证校验）、**Web Dashboard**；v0.3 全保留：**事件总线**（`EventBus` + 订阅/发布）、**子代理调度器**（现经 `subagent` 异步服务）、**skills 加载器**、**插件系统**（libloading）、**MCP 长驻连接管理**、**会话过期清理 + FileBackedStore**、**Ollama provider**；v0.2 全保留：**IPC 协议契约**（16 方法，wire 零改动）+ **权限审批系统**。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`bindings/ts/` TS 绑定测试与 `web-dashboard/` 构建需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试（~310 通过；v0.5 新增 bus 密封测试：ToolCallRequest 目标路由、async ServiceCall、TurnRunner 取消/广播、agent_loop_module、subagent dispatch；v0.4 起 e2e_serve 常驻内核生命周期密封测试、stats/license 单测、tcp 多客户端与生命周期单测、protocol 订阅 wire-shape 测试）
- `cargo clippy --workspace -- -D warnings` — lint 门禁（零警告）
- `cargo bench -p agentj-model` — 模型流式基准（criterion：`mock_stream_10_events`）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（生成结果 == 提交的 schemas）
- `cd bindings/ts && npm install && npm run generate && npm run build` — TS 绑定测试

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试流水线（PR + push main + 手动）：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、覆盖率门禁 `cargo llvm-cov --workspace --fail-under-lines 50`（门槛随开发上调至 80%）
- `.github/workflows/smoke.yml` — 冒烟流水线（PR + push main + 手动）：`cargo build --release --locked` + `agentj --version/--help/run --help/serve --help` 退出码断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E 流水线（main 合并后 + 手动）：`cargo build --release --locked` + `cargo test --release --test e2e_cli --test e2e_permission`（Mock LLM 密封测试，无真实 API key）
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`（依赖-free，可选 PyYAML 严格解析）

## Architecture

> 全部 16 个 crate 位于 `crates/` 目录（工作区成员 `crates/*`），另有前端项目 `web-dashboard/`（工作区外），下文 `agentj-*` 均为 `crates/agentj-*`。

- `agentj-protocol`（leaf crate）：IPC 协议契约的权威 wire 类型（JSON-RPC 2.0 信封、错误码、16 个方法 params/result、流式类型、订阅/权限类型），serde + schemars，零内核耦合。v0.1 的 `agentj-core`/`agentj-ipc` 应消费其类型（导入而非重定义）。
- `agentj-core`（内核）：Kernel 调度（`kernel.rs`：`KernelMessage` 混合总线 `UserRequest`/`ToolCallRequest`/`ServiceCall`/`Shutdown` + `modules()` 查询 + `ToolCallRequest` 目标路由 + `ServiceCall` 同步/异步双分支）+ 模块 trait 注册表（`module.rs`：ModelProvider / SessionStore / ToolRegistry / ContextPipeline / ConfigProvider + `ToolCallReply`）+ `ServiceRegistry` 服务定位（`resolver.rs`：`KernelService` 同步 + `AsyncKernelService` 异步，`register_kernel_service`/`register_kernel_async_service`）+ 总线化 turn 编排（`turn_runner.rs`：`TurnRunner` 持有 `KernelSender` + `tool_executor_id`，`spawn_turn`/`cancel`/`shutdown`，`AgentEvent` 迁至 `types.rs`）+ 内核模块（`tool_executor.rs`：`ToolExecutorModule` 目标路由执行；`agent_loop_module.rs`：`AgentLoopModule` 消费 `UserRequest` + 注册 `agent/cancel`；`dispatcher.rs`：`SubAgentService` async `subagent/dispatch` + `Dispatcher` 客户端；`types.rs`：`AgentEvent`/`OnTurnComplete`）。
- `agentj-config`：TOML 配置（`[model]`：`provider` + `providers[]`（name/base_url/api_key_env/**kind**）+ `default_model`；`[permission]`：per-tool `ToolPolicy`（mode/path_rules/command_rules）），ConfigProvider 读取。
- `agentj-session`：会话生命周期（InMemoryStore：状态机 SessionStatus + 快照恢复；FileBackedStore 磁盘持久化 + 过期清理 `spawn_expiry_loop`）。
- `agentj-permission`：权限系统——决策引擎（规则求值 Deny > Allow > Mode）+ 策略模型（`ToolPolicy`、`[permission]` 配置）+ `PermissionModule`（审批等待/超时 60s 自动 Deny、allow_always 会话级内存态、取消即拒绝）。
- `agentj-model`：模型提供方——三线 wire-format 提供器（`openai_compat` / `responses` / `anthropic`）+ `opencode_go` 门面（OpenCodeGoProvider，按 `models/opencode-go.json` 目录路由）+ `factory`（ProviderFactory，按 kind 实例化）+ `mock`（MockModelProvider）+ `ollama`（OllamaProvider）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write）+ ToolExecutor。
- `agentj-context`：上下文管线（纯消息构造器）。
- `agentj-stats`：用量统计（UsageCollector：per-session/global totals 与 turns 计数；poison-tolerant；由 CLI 经 AgentLoop on-turn-complete 接线）。
- `agentj-license`：Ed25519 许可证校验（`base64(payload).base64(sig)` 密钥格式，entitlements + 过期策略；design-pending 商用门控接入）。
- `agentj-mcp`：MCP 客户端——短生命周期子进程连接 + 工具发现注册，以及 **McpConnectionManager**（v0.3 长驻连接：引用计数共享 + 空闲超时回收 + 指数退避重连 + 级联 ghost-tool 注销）。 已保留，v0.5 接线不变。
- `agentj-skills`：skills 加载器（`.aj/.agents/.claude/skills` 优先级合并，同名校验 SKILL.md，注册语义随 manifest-compat 研究延后）。
- `agentj-plugin`：插件系统（manifest 解析 + 生命周期状态机 + 依赖图 + libloading 动态加载，`plugin-manager` / `state_machine` / `libloading_loader`）。
- `agentj-ipc`：JSON-RPC 2.0 IPC 服务（stdio + TCP 多客户端）。统一 Connection 抽象（StdioConnection/TcpConnection）+ ConnectionRegistry（订阅广播/属主表/run 互斥）+ 连接感知分发（initialize token 认证、run 会话互斥 `-32805`、permission 属主门控 `-32806`、subscribe/unsubscribe）+ `$/progress` 广播与权限属主路由（`origin_client`）；TcpIpcServer 常驻模式支持 --token/--max-connections。
- `agentj-cli`：`agentj` 二进制（`run [--attach tcp://…] "prompt"` / `serve [stdio] | serve --port [--token/--max-connections]` 常驻 / `dashboard --port`，AgentLoopModule/ToolExecutorModule/SubAgentService 以模块注册进 Kernel；v0.3 接线保留：MCP 空闲回收 task + 会话过期循环 + skills 加载；v0.4 接线：UsageCollector on-turn-complete + TCP 常驻分支 + e2e_serve；v0.5 接线：`TurnRunner` 经 bus 驱动 `session/run`→`UserRequest` + `$/cancelRequest`→`agent/cancel`）。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具（`libloading_loader` 集成测试用）。
- `schemas/*.json`：schemars 从 Rust 类型生成的 32 个 JSON Schema（机器事实来源，提交件 + CI 漂移门禁）。
- `protocol/methods.json`：方法注册表（method ↔ params/result schema、方向、类型）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节，中文）。
- `bindings/ts/`：TS 绑定测试（json-schema-to-typescript + tsc）。
- `web-dashboard/`：Web Dashboard 前端（Vite + React + TS；RpcClient over node:net TCP，kernel/status 模块面板；vitest 单测 + vite 构建）。
- **守护内核（v0.4 已实现）**：统一 `Connection` 传输抽象（stdio 哨兵 / TCP），`serve --port` 常驻多客户端 + `run --attach` 附着 + `dashboard`；协议加性扩展已落地（`-32805 SessionBusy`/`-32806 NotAuthorized`、`connection_id`/`multi_client`/`token`、`session/subscribe`/`unsubscribe`、`$/progress`、`origin_client` 属主路由）——设计见 `docs/superpowers/specs/2026-08-15-agentj-daemon-kernel-design.md`。

## Conventions

- 协议 wire 字段一律 snake_case（`protocol_version`、`client_info`、`session_id` 等）。
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；未知能力/枚举值容忍，不致命）。
- 值枚举（Role/ModuleState/SessionStatus/PermissionDecision）带 `#[serde(other)]` + `#[schemars(skip)]` `Unknown` 变体（向前兼容；schema 只列已知值）。

## Rules

- Commit messages in English. `docs/superpowers/specs/` docs in Chinese.
- Docs under `docs/` are tracked deliverables — commit them.
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`).
- Before planning/developing, read `.claude/summary/summary_and_next.md` and `docs/superpowers/` first.
- All diagrams in spec docs under `docs/superpowers/specs/` must use UML or Mermaid modeling diagrams — no other diagram formats.
- Update `docs/superpowers/specs` target spec docs TODO list after each phase.
- `.claude/summary/summary_and_next.md` only retains content for the **currently developing** feature design document: its progress, summary, remaining issues, and next steps. When a new feature starts, replace (don't append) the previous feature's content.
- After each plan completes, update `.claude/summary/summary_and_next.md` with the current feature's:
  - Progress and summary of this phase
  - Any remaining issues or known limitations
  - The next plan / next steps
- After each feature phase completes AND `cargo test --workspace` + `cargo clippy --workspace -- -D warnings` pass, automatically:
  - Update `AGENTS.md`: sync version, migrations count, new API routes, architecture notes, config vars, crate boundaries — any structural change introduced in the phase.
  - Update `README.md`: sync version tagline, Features checklist, Project Structure tree, API endpoint tables, migrations count, and config var table — any user-facing change introduced in the phase.
  - Update `.claude/summary/summary_and_next.md`: rewrite it with the current feature's "## {phase}: {title} ✅ (本次完成)" progress, verification results, remaining issues, and next steps — keeping only content for the currently developing feature design document, and updating the "## 待实施" table if needed.
  - Commit the three files together as `docs: auto-sync AGENTS.md, README.md, summary after {phase} completion`.
  - Do NOT wait for the user to request this — it is a mandatory post-phase step.
- Clean up temp files, Docker containers after each development phase.
- When a command hangs >120s, cancel and retry.
- PR creation: create the PR and share the link — the user handles merge.
- Rust function or method should less than 50 lines, and <= 120 characters for each line.
