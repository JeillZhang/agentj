# AGENTS.md

> Updated 2026-08-15 — daemon-kernel (resident API service) design phase.

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。当前处于 v0.1 前序阶段：**统一 UI 接口契约（IPC 协议）已落地**（16 方法）；**守护内核（常驻 API 服务）设计已批准**——统一 Connection 传输抽象契约 v0.1 固化、多客户端常驻能力 v0.3 落地（见 v0.1/v0.3 计划）；内核（agentj-core 等）尚未开发。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（仅 `bindings/ts/` TS 绑定测试需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试（agentj-protocol：31 单元 + 7 契约 + 3 注册表）
- `cargo clippy --workspace -- -D warnings` — lint 门禁（零警告）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（生成结果 == 提交的 schemas）
- `cd bindings/ts && npm install && npm run generate && npm run build` — TS 绑定测试

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试流水线（PR + push main + 手动）：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、覆盖率门禁 `cargo llvm-cov --workspace --fail-under-lines 50`（门槛随开发上调至 80%）
- `.github/workflows/smoke.yml` — 冒烟流水线（PR + push main + 手动）：`cargo build --release --locked` + `agentj --version/--help/run --help/serve --help` 退出码断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E 流水线（main 合并后 + 手动）：`cargo build --release --locked` + `cargo test --release --test e2e -- --ignored`（Mock LLM 密封测试，无真实 API key）
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`（依赖-free，可选 PyYAML 严格解析）
- ⚠️ 已知缺口：smoke/e2e 流水线引用 `agentj-core`/`agentj`，内核 crate 尚未创建（v0.1 计划执行后才会存在）。

## Architecture

- `agentj-protocol`（leaf crate）：IPC 协议契约的权威 wire 类型（JSON-RPC 2.0 信封、错误码、16 个方法 params/result、流式类型、订阅/权限类型），serde + schemars，零内核耦合。v0.1 的 `agentj-core`/`agentj-ipc` 应消费其类型（导入而非重定义）。
- `schemas/*.json`：schemars 从 Rust 类型生成的 31 个 JSON Schema（机器事实来源，提交件 + CI 漂移门禁）。
- `protocol/methods.json`：方法注册表（method ↔ params/result schema、方向、类型）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节，中文）。
- `bindings/ts/`：TS 绑定测试（json-schema-to-typescript + tsc）。
- **守护内核（设计已批准，契约 v0.1 / 能力 v0.3）**：统一 `Connection` 传输抽象（stdio / TCP / Unix socket），`agentj serve --port` 常驻内核多客户端共享；协议加性扩展（`multi_client` / `connection_id` / `session/subscribe` / `permission` 定向 / `-32805`/`-32806`）——见 `docs/superpowers/specs/2026-08-15-agentj-daemon-kernel-design.md`。v0.1 计划 T13/T14 已按 Connection 模型改写；v0.3 计划 T0/T3/T4 已改为多客户端常驻。

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
