# AGENTS.md

> Updated 2026-09-04 — v0.10.0 released (16 crates，G7 EventBus 重入防护：`EventGuard` thread_local `EVENT_DEPTH`/`EVENT_STACK` + `MAX_EVENT_DEPTH=16` + 同栈循环检测 + `tracing::warn!` 丢弃 + RAII Drop 回卷 + `catch_unwind` 叠加；G8 WebSocket 网关：`ws://127.0.0.1:<port>/ws` 同端口连接分类器（`MSG_PEEK` + `looks_like_http`）+ `WsConnection`（`TransportKind::WebSocket` + 原生 Text 帧 + mpsc writer 桥）+ `WsServerState`/`ws_router`/`serve_http`（axum 0.8 ws + hyper-util `serve_connection_with_upgrades`，`?token=` 403 + `initialize.token` -32806 双通道，`max_connections` 独立计数，WS 不参与 last-disconnect 全局关闭）+ 前端 `WebSocketTransport`（原生 WebSocket，`VITE_AGENTJ_WS_URL`）+ `ResponseRouter` 通知分叉（BUG-005 修复）+ CLI `serve --port` 打印 `websocket on ws://…/ws`；G5 License gating：`[license] key` + `--license-key` + `AGENTJ_LICENSE_KEY` env + `AGENTJ_LICENSE_PUBKEY` override + per-command entitlements `run`/`serve`/`dashboard` + free mode；G6 MCP wiring：`[[mcp.servers]] name/command/args/env` + `McpServerSpec` connect + `close_all` + startup wiring warn-and-continue；G3 token 级用量统计：`TokenUsage` + `ModelEvent::Usage` + `AgentEvent::Done.usage` + `OnTurnComplete(Option<TokenUsage>)` + 4 提供方流式 usage 解析（openai_compat `stream_options.include_usage` + Done@[DONE] + responses `response.completed` + anthropic `message_delta` + ollama `prompt_eval_count/eval_count`）+ `SessionRunResult.usage` 加性 + CLI `record_token_usage` 激活 + 33 schemas + TS 绑定；G4 会话持久化生产接线：FileBackedStore 生产接入（CLI `resolve_session_dir` XDG 默认 + `[session] dir` 覆盖 + 单点实例化共享于 run/serve/serve --port/dashboard）+ `SessionStore` trait 化 `scan_expired`/`spawn_expiry_loop`（`InMemoryStore`/`FileBackedStore` 各自实现）+ `FileBackedStore::persist` 唯一 tmp `.<uuid>.json.tmp` + 目录 fsync；v0.6 全保留：per-session in-flight 路由（pending_runs 事实来源 + session_of 查表门控 + CancelParams.session_id 精准路由）；v0.5 全保留：内核调度全重构：KernelMessage 总线化、TurnRunner 总线化、ToolExecutorModule/AgentLoopModule、subagent/dispatch、IpcServer/CLI 全面接总线；v0.4 全保留：守护内核多客户端常驻、方法面 16、$/progress 广播+权限属主路由、stats/license/Web Dashboard）

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。v0.10 已落地：**G7 EventBus 重入防护**——`EventBus::publish` 入口 `EventGuard`（thread_local `EVENT_DEPTH: Cell<u8>` + `EVENT_STACK: Vec<EventType>`，`MAX_EVENT_DEPTH=16`，同栈循环 `stack.contains(event)` 即时拦截 + 跨类型环深度兜底，超限/循环 `tracing::warn!` 丢弃，RAII Drop 回卷 + `catch_unwind` 叠加，调用点零改动）；**G8 WebSocket 网关**——`ws://127.0.0.1:<port>/ws` 同端口共存 raw TCP（NDJSON）与 HTTP/WS（单一 `TcpListener` + `MSG_PEEK` 分类器 + `looks_like_http`，`run --attach tcp://` 零破坏），`WsConnection` 实现 `Connection`（`TransportKind::WebSocket`，原生 Text 帧，无 `\n` 拼接，mpsc writer 桥复用半连接写句柄），服务端 `WsServerState`/`ws_router`/`ws_upgrade`/`serve_http`（axum 0.8 ws + hyper-util `serve_connection_with_upgrades`，鉴权双通道 `?token=` 403 + `initialize.token` -32806，`max_connections` 独立计数，WS 不参与 last-disconnect 全局关闭），前端 `WebSocketTransport`（原生 `WebSocket`，`VITE_AGENTJ_WS_URL` 可配，`onStatusChange` + `close` + `ResponseRouter` 通知分叉修复 BUG-005，删除 `node:net`），CLI `serve --port` 打印 `websocket on ws://…/ws`；**G5 License gating**——`[license] key` + `--license-key` + `AGENTJ_LICENSE_KEY` env + `AGENTJ_LICENSE_PUBKEY` override + per-command entitlements `run`/`serve`/`dashboard` + free mode（无 key 零门控，有 key Ed25519 校验 + entitlement 门控，CLI 命令边界 fail-fast）；**G6 MCP wiring**——`[[mcp.servers]] name/command/args/env` + `McpServerSpec` connect（`Command::new` + args/env，消除 `sh -c` 注入面）+ `close_all` + startup 遍历 warn-and-continue；G3 token 级用量统计——`TokenUsage` + `ModelEvent::Usage` + `AgentEvent::Done.usage` + `OnTurnComplete(Option<TokenUsage>)` + 4 提供方流式 usage 解析（openai_compat `stream_options.include_usage` + Done@[DONE] + responses `response.completed` + anthropic `message_delta` + ollama `prompt_eval_count/eval_count`）+ `SessionRunResult.usage` 加性 + CLI `record_token_usage` 激活 + 33 schemas + TS 绑定；G4 会话持久化生产接线：`FileBackedStore` 成为 CLI 生产会话存储（`resolve_session_dir`：`[session] dir` 覆盖 → `$XDG_DATA_HOME/agentj/sessions` → `$HOME/.local/share/agentj/sessions`，单点 `Arc<FileBackedStore>` 共享于 `run`/`serve`/`serve --port`/`dashboard`，`spawn_expiry_loop` 在具体 `Arc<FileBackedStore>` 上驱动磁盘版过期），`SessionStore` trait 化 `scan_expired`/`spawn_expiry_loop`（`InMemoryStore`/`FileBackedStore` 各自实现，默认 `Ok(0)`），`FileBackedStore::persist` 唯一 tmp `.<uuid>.json.tmp` + 目录 `fsync`（防并发 tmp 互踩 + rename 持久性）；v0.6 全保留：**per-session in-flight 路由**（`pending_runs` 事实来源 + `session_of` 查表门控 + `CancelParams.session_id` 精准路由 + `$/permissionRequest` `session_id` @since 0.6.0）；v0.5 全保留：**内核调度全重构**（`KernelMessage` 混合总线、`TurnRunner` 总线化、`ToolExecutorModule`/`AgentLoopModule`、`subagent/dispatch`、`IpcServer`/`CLI` 全面接总线）；v0.4 全保留：**守护内核**（统一 Connection 抽象 + 多客户端常驻 + 方法面 16 + `$/progress` 广播 + 权限属主路由 + stats/license/Web Dashboard）；v0.3 全保留：**事件总线** + **子代理调度** + **skills** + **插件** + **MCP 长驻连接** + **会话过期 + FileBackedStore** + **Ollama**；v0.2 全保留：**IPC 协议契约** + **权限审批**。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`bindings/ts/` TS 绑定测试与 `web-dashboard/` 构建需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试（~320 通过；v0.5 新增 bus 密封测试：ToolCallRequest 目标路由、async ServiceCall、TurnRunner 取消/广播、agent_loop_module、subagent dispatch；v0.4 起 e2e_serve 常驻内核生命周期密封测试、stats/license 单测、tcp 多客户端与生命周期单测、protocol 订阅 wire-shape 测试）
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

- `agentj-protocol`（leaf crate）：IPC 协议契约的权威 wire 类型（JSON-RPC 2.0 信封、错误码、16 个方法 params/result、流式类型、订阅/权限类型），serde + schemars，零内核耦合。v0.6 加性：`CancelParams.session_id` + `PermissionRequestParams.session_id`（`@since 0.6.0`，`Option<SessionId>`）；v0.8 加性：`UsageInfo` + `SessionRunResult.usage: Option<UsageInfo>`（`skip_serializing_if`，33 schemas）。v0.1 的 `agentj-core`/`agentj-ipc` 应消费其类型（导入而非重定义）。
- `agentj-core`（内核）：Kernel 调度（`kernel.rs`：`KernelMessage` 混合总线 `UserRequest`/`ToolCallRequest`/`ServiceCall`/`Shutdown` + `modules()` 查询 + `ToolCallRequest` 目标路由 + `ServiceCall` 同步/异步双分支）+ 模块 trait 注册表（`module.rs`：ModelProvider / SessionStore（`scan_expired`/`spawn_expiry_loop` trait 化，默认 `Ok(0)`） / ToolRegistry / ContextPipeline / ConfigProvider + `ToolCallReply`）+ `ServiceRegistry` 服务定位（`resolver.rs`：`KernelService` 同步 + `AsyncKernelService` 异步，`register_kernel_service`/`register_kernel_async_service`）+ 总线化 turn 编排（`turn_runner.rs`：`TurnRunner` 持有 `KernelSender` + `tool_executor_id`，`spawn_turn`/`cancel`/`shutdown`，`AgentEvent` 迁至 `types.rs` + `TokenUsage` 聚合、`ModelEvent::Usage` 处理、`AgentEvent::Done.usage` 回传、`OnTurnComplete(Option<TokenUsage>)` 回调）+ 事件总线重入防护（`hooks.rs`：`EventGuard` thread_local `EVENT_DEPTH: Cell<u8>` + `EVENT_STACK: Vec<EventType>`，`MAX_EVENT_DEPTH=16`，同栈循环检测 + `tracing::warn!` 丢弃 + RAII Drop 回卷 + `catch_unwind` 叠加，调用点零改动）+ 内核模块（`tool_executor.rs`：`ToolExecutorModule` 目标路由执行；`agent_loop_module.rs`：`AgentLoopModule` 消费 `UserRequest` + 注册 `agent/cancel`；`dispatcher.rs`：`SubAgentService` async `subagent/dispatch` + `Dispatcher` 客户端；`types.rs`：`TokenUsage`/`ModelEvent::Usage`/`AgentEvent::Done.usage`/`OnTurnComplete`）。
- `agentj-config`：TOML 配置（`[model]`：`provider` + `providers[]`（name/base_url/api_key_env/**kind**）+ `default_model`；`[permission]`：per-tool `ToolPolicy`（mode/path_rules/command_rules）；`[session]`：`dir`（会话持久化目录，覆盖 XDG 默认）；`[license]`：`key`（Ed25519 签名 key，可选）；`[[mcp.servers]]`：`name`/`command`/`args`/`env`（MCP 服务配置）），ConfigProvider 读取。
- `agentj-session`：会话生命周期（InMemoryStore：状态机 SessionStatus + 快照恢复；FileBackedStore 磁盘持久化 + 过期清理 `spawn_expiry_loop`（trait 化 `scan_expired`/`spawn_expiry_loop`，FileBacked 磁盘版 + 唯一 tmp `.<uuid>.json.tmp` + 目录 fsync））。
- `agentj-permission`：权限系统——决策引擎（规则求值 Deny > Allow > Mode）+ 策略模型（`ToolPolicy`、`[permission]` 配置）+ `PermissionModule`（审批等待/超时 60s 自动 Deny、allow_always 会话级内存态、取消即拒绝；`session_of(request_id)` 只读 peek 供 per-session 门控）。
- `agentj-model`：模型提供方——三线 wire-format 提供器（`openai_compat` / `responses` / `anthropic`）含流式 usage 解析（openai_compat `stream_options.include_usage` + `choices:[]`+`usage` 帧 + Done@[DONE]；responses `response.completed.usage`；anthropic `message_delta.usage`；ollama `prompt_eval_count`/`eval_count`）+ `opencode_go` 门面（OpenCodeGoProvider，按 `models/opencode-go.json` 目录路由）+ `factory`（ProviderFactory，按 kind 实例化，mock 脚本含 `Usage` 帧）+ `mock`（MockModelProvider）+ `ollama`（OllamaProvider）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write）+ ToolExecutor。
- `agentj-context`：上下文管线（纯消息构造器）。
- `agentj-stats`：用量统计（UsageCollector：per-session/global totals 与 turns 计数；poison-tolerant；由 CLI 经 AgentLoop on-turn-complete 接线 `record_token_usage` 激活）。
- `agentj-license`：Ed25519 许可证校验（`base64(payload).base64(sig)` 密钥格式，entitlements + 过期策略；现已 CLI 生产接入：`check_license_gate` per-command entitlement 门控 + free mode + `AGENTJ_LICENSE_PUBKEY` override）。
- `agentj-mcp`：MCP 客户端——短生命周期子进程连接 + 工具发现注册，以及 **McpConnectionManager**（v0.3 长驻连接：引用计数共享 + 空闲超时回收 + 指数退避重连 + 级联 ghost-tool 注销；v0.9 升级 `McpServerSpec` connect（`Command::new` + args/env，消除 `sh -c` 注入面）+ `close_all` 优雅关闭）。
- `agentj-skills`：skills 加载器（`.aj/.agents/.claude/skills` 优先级合并，同名校验 SKILL.md，注册语义随 manifest-compat 研究延后）。
- `agentj-plugin`：插件系统（manifest 解析 + 生命周期状态机 + 依赖图 + libloading 动态加载，`plugin-manager` / `state_machine` / `libloading_loader`）。
- `agentj-ipc`：JSON-RPC 2.0 IPC 服务（stdio + TCP 多客户端 + WebSocket 网关）。统一 Connection 抽象（StdioConnection/TcpConnection/WsConnection `ws.rs`，`TransportKind::WebSocket`）+ ConnectionRegistry（订阅广播/属主表/run 互斥 `pending_runs`，WS 复用 `register_writer` + `serve_connection_requests` 零改动）+ 同端口连接分类器（`TcpIpcServer` 单一 `TcpListener` + `MSG_PEEK` + `looks_like_http`，HTTP→`serve_http` / 非 HTTP→`TcpConnection`，`run --attach tcp://` 零破坏）+ `ws.rs`（`WsServerState`/`ws_router`/`ws_upgrade`/`serve_http`：axum 0.8 ws + hyper-util `serve_connection_with_upgrades`，`GET /ws?token=` 403 + `initialize.token` -32806 双通道鉴权，`max_connections` 独立计数，WS 不参与 last-disconnect 全局关闭）+ `WsConnection`（`TransportKind::WebSocket`，原生 Text 帧，无 `\n` 拼接，mpsc writer 桥复用半连接写句柄）+ per-session 路由（`permission/respond` 经 `module.session_of` 查表门控 + `$/cancelRequest` 经 `CancelParams.session_id` 精准路由，缺省单会话兜底、多会话 no-op + `tracing::warn!`）+ `$/permissionRequest` 携带 `session_id` + `$/progress` 广播与权限属主路由（`origin_client`）+ `dispatch_session_run` 映射 `Done.usage` → `SessionRunResult.usage`；TcpIpcServer 常驻模式支持 --token/--max-connections。
- `agentj-cli`：`agentj` 二进制（`run [--attach tcp://…] "prompt"` / `serve [stdio] | serve --port [--token/--max-connections]` 常驻 / `dashboard --port`，AgentLoopModule/ToolExecutorModule/SubAgentService 以模块注册进 Kernel；v0.3 接线保留：MCP 空闲回收 task + 会话过期循环 + skills 加载；v0.4 接线：UsageCollector on-turn-complete + TCP 常驻分支 + e2e_serve；v0.5 接线：`TurnRunner` 经 bus 驱动 `session/run`→`UserRequest` + `$/cancelRequest`→`agent/cancel`；v0.7 接线：`FileBackedStore` 生产接入（`resolve_session_dir` XDG 默认 + `[session] dir` 覆盖，单点 `Arc<FileBackedStore>` 共享于 run/serve/dashboard，`spawn_expiry_loop` 磁盘版）；v0.8 接线：`record_completed_turn` 三分支（`Some(non-zero)`→`record_token_usage`/`Some(zero)|None`→`record_turn`）+ `FileBackedStore` 单点接线复用；v0.9 接线：`check_license_gate` per-command 门控（`[license] key` + `--license-key` + `AGENTJ_LICENSE_KEY` + `AGENTJ_LICENSE_PUBKEY` override + free mode）+ `[[mcp.servers]]` 遍历 `McpServerSpec` connect warn-and-continue + `close_all` 优雅关闭；v0.10 接线：`serve --port` 同端口 TCP+WS（`serve_tcp` 打印 `websocket on ws://…/ws`，`?token=` 透传 WS 握手 + `max_connections` 独立限流）。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具（`libloading_loader` 集成测试用）。
- `schemas/*.json`：schemars 从 Rust 类型生成的 33 个 JSON Schema（机器事实来源，提交件 + CI 漂移门禁）。
- `protocol/methods.json`：方法注册表（method ↔ params/result schema、方向、类型）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节，中文）。
- `bindings/ts/`：TS 绑定测试（json-schema-to-typescript + tsc）。
- `web-dashboard/`：Web Dashboard 前端（Vite + React + TS；`WebSocketTransport` over 原生 `WebSocket`（`VITE_AGENTJ_WS_URL` 可配，`onStatusChange` + `close`），`ResponseRouter` 通知分叉（BUG-005 修复），`App.tsx` `ws://…/ws` 直连，无 `node:net`；vitest 单测 + vite 构建）。
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
