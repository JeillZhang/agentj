# AGENTS.md

> Updated 2026-09-07 — v0.12.3 agentj-ui Visual 2.1 refine (DeepSeek-blue #4176E6/#679EFE
>  + tokens 19 keys + Web polished: shell/composer/session/conversation/trajectory/hero/dialog)
> (16 crates + agentj-ui monorepo：Web / TUI / Desktop + SDK + tokens，
> npm workspaces `apps/*`+`packages/*`，共享 SDK + protocol + tokens，
> WebSocket 网关同端口复用，后端零改造，frontend stays in-repo)

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。
v0.11 已落地：**agentj-ui 三端统一前端**
（`web-dashboard` → `agentj-ui/apps/web` 重命名，`bindings` 迁入
`agentj-ui/packages/protocol`，新建 `packages/sdk` 共享 SDK，
三端同源消费 `@agentj/sdk` + `@agentj/protocol`，后端零改造；
Web 对齐：方法常量化/`?token=` 透传/`protocol_version` 同源/
会话收藏/composer/`$/progress`/审批对话框/usage；
TUI 全功能：Bun + @opentui/react，连接管理/会话列表/新建/继续/
composer 多行/@补全//palette/流式渲染/审批模态/模型只读/主题/usage/
优雅退出 + mock WS e2e；
Desktop：Tauri v2 壳套 `apps/web` 产物，四件套 deb/AppImage/dmg/msi，
`check-artifacts.sh` 校验；G7/G8/WebSocket 等 0.10 能力全保留）。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`agentj-ui/apps/web` + `agentj-ui/packages/protocol`
  + `agentj-ui/packages/sdk` 构建与测试需要）
- Bun ≥1.2（`agentj-ui/apps/tui` dev/build/test 需要）
- Rust 工具链（`agentj-ui/apps/desktop` Tauri 壳构建需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试
  （~320+ 通过；含 bus 密封 + FileBackedStore/Config/CLI 单测
  + e2e_cli/e2e_permission/e2e_serve/e2e_ws 密封测试）
- `cargo clippy --workspace --all-targets -- -D warnings` — lint 门禁（零警告）
- `cargo bench -p agentj-model` — 模型流式基准（criterion）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（33 schemas 无漂移）
- `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` — 生成 protocol 类型
  （`schemas/*.json` → `agentj-ui/packages/protocol/types.d.ts`）
- `npm --prefix agentj-ui run build --workspace=@agentj/protocol` — `tsc --noEmit` 校验
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — SDK 单测（client/router/transports/config）
- `npm --prefix agentj-ui run generate --workspace=@agentj/sdk` — 生成 `METHODS` 常量
  （`protocol/methods.json` → `agentj-ui/packages/sdk/src/methods.ts`）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — Web 单测（vitest）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — Web 构建（`tsc -b && vite build`）
- `bun --cwd agentj-ui/apps/tui test` — TUI 单测（含 mock WS e2e）
- `./agentj-ui/apps/desktop/check-artifacts.sh linux` — Desktop 产物校验
- `./scripts/check-version.sh` — 版本一致性（Cargo + sdk/config + desktop 均为 0.12.3，PROTOCOL_VERSION 0.12.1）

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试（PR + push main + 手动）：
  `cargo fmt`、`cargo clippy`、`cargo test --workspace --locked`、
  `cargo llvm-cov --workspace --fail-under-lines 50`
- `.github/workflows/smoke.yml` — 冒烟（PR + push main + 手动）：
  `cargo build --release` + `agentj --version/--help` 断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E（main 合并后 + 手动）：
  `cargo test --release --test e2e_cli --test e2e_permission`
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`

## Architecture

> 全部 16 个 crate 位于 `crates/`（`crates/*`），前端 monorepo 位于
> `agentj-ui/`（npm workspaces，frontend stays in-repo，不拆独立仓），
> 下文 `agentj-*` 均为 `crates/agentj-*`，`@agentj/*` 为 `agentj-ui` 包。

- `agentj-protocol`（leaf）：IPC 协议权威 wire 类型（JSON-RPC 2.0、
  错误码、16 方法 params/result、流式/订阅/权限类型），serde + schemars，
  零内核耦合。v0.6 加性 CancelParams/PermissionRequestParams
  session_id；v0.8 加性 UsageInfo/SessionRunResult.usage（33 schemas）。
- `agentj-core`（内核）：Kernel 混合总线 + 模块 trait 注册表
  + ServiceRegistry（同步/异步） + TurnRunner 总线化 turn 编排
  + 事件总线重入防护（EventGuard thread_local + MAX_EVENT_DEPTH=16）。
- `agentj-config`：TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）。
- `agentj-session`：会话生命周期（InMemoryStore/FileBackedStore + 过期清理）。
- `agentj-permission`：权限系统（决策引擎 + 60s 超时 + allow_always）。
- `agentj-model`：模型提供方（三线 wire-format + opencode-go + Mock + Ollama）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write）。
- `agentj-context`：上下文管线。
- `agentj-stats`：用量统计 UsageCollector。
- `agentj-license`：Ed25519 许可证校验（entitlements + free mode）。
- `agentj-mcp`：MCP 客户端 + McpConnectionManager 长驻连接
  （McpServerSpec + close_all）。
- `agentj-skills`：skills 加载器。
- `agentj-plugin`：插件系统（manifest + 状态机 + 依赖图 + libloading）。
- `agentj-ipc`：JSON-RPC IPC（stdio/TCP/WS + 同端口分类器
  + WsConnection + WsServerState/ws_router/serve_http + per-session 路由）。
- `agentj-cli`：`agentj` 二进制（run/serve/dashboard，模块注册进 Kernel）。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具。
- `schemas/*.json`：33 JSON Schema（schemars 生成，提交件 + drift 门禁）。
- `protocol/methods.json`：方法注册表（method ↔ schema/方向/类型）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节）。
- `agentj-ui/packages/protocol`：协议包（原 `bindings` 迁入，
  `schemas/*.json` → `types.d.ts`，`json-schema-to-typescript` + `tsc --noEmit`，
  为 drift 门禁后第一门）。
- `agentj-ui/packages/sdk`：共享 SDK（`src/client.ts` RpcClient +
  `src/router.ts` ResponseRouter/RpcTransport + `src/config.ts`
   `VERSION="0.12.3"` + `PROTOCOL_VERSION="0.12.1"` + `resolveWsUrl` + `src/methods.ts` METHODS
  + `transports/websocket.ts` WebSocketTransport
  + `transports/tcp.ts` TcpTransport；被 Web/TUI/Desktop 同源消费）。
- `agentj-ui/packages/tokens`：共享语义 tokens（`palette.ts` 19 keys × dark/light + `SEMANTIC_KEYS`，`tokens.css/tokens.json/docs/ui-tokens.md` 生成，三端约束）。
- `agentj-ui/apps/web`：Web 前端（Vite + React 19，消费 `@agentj/sdk` + `@agentj/tokens` + `lucide-react`；
  布局/会话/Composer/对话/轨迹/Hero/对话框拆 `components/*.css`，vitest + vite build）。
- `agentj-ui/apps/tui`：TUI 前端（Bun + @opentui/react，全功能：
  connection/session/composer/runview/permission/palette/models/
  completion/theme/usage + mock WS e2e；`testRender` 快照）。
- `agentj-ui/apps/desktop`：Desktop 壳（Tauri v2，`frontendDist` →
   `../../web/dist`，与 workspace 版本同源 `0.12.3`，四件套打包）。
- `agentj-ui/` 根：`package.json` workspaces `["apps/*","packages/*"]`
  （frontend stays in-repo，前端不拆独立仓，与 Rust 共享 0.12.3）。
- **守护内核（v0.4 已实现）**：统一 Connection 抽象 + `serve --port`
  常驻多客户端 + `run --attach` + `dashboard`（见 daemon-kernel 设计文档）。

> Crate 边界：Rust 16 crate 保持 `crates/*` 边界纯净，
> 前端（TS/React/Bun/Tauri）全部收敛于 `agentj-ui/` 内，
> 通过 `protocol/methods.json` + `schemas/*.json` 与后端契约对齐，
> 不向 Rust workspace 注入前端构建产物。

## Conventions

- 协议 wire 字段一律 snake_case（`protocol_version`、`session_id` 等）。
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；
  未知能力/枚举值容忍）。
- 值枚举带 `#[serde(other)]` + `#[schemars(skip)]` Unknown 变体（向前兼容）。

## Rules

- Commit messages in English. `docs/superpowers/specs/` docs in Chinese.
- Docs under `docs/` are tracked deliverables — commit them.
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`).
- Before planning/developing, read `.claude/summary/summary_and_next.md` and `docs/superpowers/` first.
- All diagrams in spec docs under `docs/superpowers/specs/` must use UML or Mermaid.
- Update `docs/superpowers/specs` target spec docs TODO list after each phase.
- `.claude/summary/summary_and_next.md` only retains currently developing feature:
  progress, remaining issues, next steps. Replace, don't append.
- After each plan completes, update summary with current feature's progress,
  remaining issues, next plan/next steps.
- After each feature phase completes AND `cargo test` + `clippy` pass, auto:
  - Update `AGENTS.md` / `README.md` / `.claude/summary/summary_and_next.md`
  - Commit as `docs: auto-sync AGENTS.md, README.md, summary after {phase}`.
  - Do NOT wait for user — mandatory post-phase step.
- Clean up temp files, Docker containers after each phase.
- When a command hangs >120s, cancel and retry.
- PR creation: create PR and share link — user handles merge.
- Rust function ≤50 lines, ≤120 chars per line.
