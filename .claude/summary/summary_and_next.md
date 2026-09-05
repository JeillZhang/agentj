# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui 三端统一前端（Web / TUI / Desktop + SDK，v0.11.0）**
> ——设计文档 `docs/superpowers/specs/2026-09-05-agentj-ui-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-05-agentj-ui-plan.md`
> （web-dashboard → agentj-ui 重命名 + SDK 抽取 + Web 对齐 + TUI 全量 + Tauri 打包，v0.11.0 发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS 绑定） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| ✅ 完成 | G2 修复：单全局 in-flight 会话槽 → per-session 路由（T1-T6，v0.6.0） | v0.5 | P0 | ✅ 已实现 v0.6.0 |
| ✅ 完成 | G4 会话持久化生产接线（FileBackedStore + 唯一 tmp + 目录 fsync，v0.7.0） | v0.6.0 | P0 | ✅ 已实现 v0.7.0 |
| ✅ 完成 | G3 token 级用量统计（TokenUsage + 4 提供方 + SessionRunResult.usage，v0.8.0） | v0.7.0 | P0 | ✅ 已实现 v0.8.0 |
| ✅ 完成 | G5 License gating + G6 MCP wiring（[license] + McpServerSpec + close_all，v0.9.1） | v0.8.0 | P0 | ✅ 已实现 v0.9.1 |
| ✅ 完成 | G7 EventBus 重入防护 + G8 WebSocket 网关（v0.10.0，G1-G8 全闭环） | v0.9.1 | P0 | ✅ 已实现 v0.10.0 |
| ✅ 完成 | **agentj-ui 三端统一前端（Web / TUI / Desktop + SDK，v0.11.0）**（T0-T18） | v0.10.0 | P0 | ✅ 已实现 v0.11.0 |

## Phase: agentj-ui 三端统一前端 (v0.11.0) ✅ (本次完成)

### 进度与摘要

- ✅ **根因/动机**：`web-dashboard` 单端、逻辑与协议 SDK 耦合于 Web、TUI/Desktop 缺失；
  需在零后端改造前提下沉淀三端共享 SDK + 类型管线，并交付对齐的 Web + 全功能 TUI + Tauri 壳。

- ✅ **方案**（设计 D1-D8）：
  单一 npm workspaces monorepo `agentj-ui/`（`apps/*` + `packages/*`，frontend stays in-repo）；
  `packages/protocol` 迁 `bindings` 生成管线（schemas → types.d.ts，drift 门禁前置）；
  `packages/sdk` 抽取 `client`/`router`/`transports`/`config`/`methods`（生成而非手写）；
  Web 就地对齐（常量化/`?token=`/`protocol_version` 同源/收藏/composer/流式/审批/usage）；
  TUI 基于 `@opentui/react` + Bun 全功能（连接/会话/composer/@补全//palette/流式/审批/模型只读/主题/usage/优雅退出）；
  Desktop Tauri 壳套 `apps/web` 产物，四件套打包 + check-artifacts 校验。

- ✅ **任务分解（T0-T18）已完成**：
  - P0 基建：
    - T0 — `web-dashboard` → `agentj-ui/apps/web` 重命名 + workspaces 根
      （`apps/*`/`packages/*`，`@agentj/web` 重命名，`AgentJ UI` 标题）。
    - T1 — `bindings` → `agentj-ui/packages/protocol`
      （`generate.mjs` 重定向至 `../../../schemas`，`@agentj/protocol`，`types.d.ts` + tsc）。
    - T2 — `packages/sdk` 抽取 `client.ts`/`router.ts`（零逻辑改动，
      `@agentj/sdk` exports 直指 TS 源码，`src/index.ts` 统一再导出）。
    - T3 — `config.ts`（`PROTOCOL_VERSION="0.11.0"` 单一来源 + `resolveWsUrl`）
      + `methods.ts`（`protocol/methods.json` 生成 16 方法常量）。
    - T4 — `transports/tcp.ts`（Node `node:net` NDJSON + pending map + close）
      + `transports/websocket.ts` 从 `apps/web` 迁入。
    - T5 — Web 重接入 `@agentj/sdk`（`App.tsx` 改 `@agentj/sdk`，删除本地
      `jsonrpc.ts`/`transport/websocket.ts`，`@agentj/sdk:*` 依赖）。
  - P1 Web 对齐：
    - T6 — `ConnectionBar` + `resolveWsUrl`/`METHODS`/`PROTOCOL_VERSION` 接线
      + token 透传 + 版本横幅（expected vs got）+ 未连接引导。
    - T7a — `SessionPanel`（新建/凭 ID 继续 + localStorage 收藏 + 失效标记）。
    - T7b — `RunPanel`（composer 多行 + Send + ProgressFeed `$progress` + usage 结算）。
    - T7c — `ApprovalDialog`（`$permissionRequest` 模态 + Allow/Deny/AllowAll
      → `permission/respond`，失败内联报错不阻塞 run 视图）。
  - P2 TUI 全量：
    - T8 — `@agentj/tui` 脚手架 + 连接状态机（指数退避 30s 上限 + 403 停重试 + Ctrl-C 退出 0）。
    - T9 — 会话管理（本地历史 `session-history.json`，无 `session/list`，
      `session/new`/`status`/`subscribe` 切换，`@agentj/sdk` 消费）。
    - T10 — composer（多行 Textarea + 历史）+ runview（追加滚动）。
    - T11 — 审批对话框（Allow/Deny/AllowAll → `permission/respond`）。
    - T12 — 命令面板 `/` + 模型只读选择器（`default_model` 预选，无 `session/run.model`
      注入，附 `config.toml [model] default_model` 提示，anti-dead-affordance）。
    - T13a — `@` 文件补全（`completePath` 前缀匹配）。
    - T13b — 主题切换（dark/light + 持久化）。
    - T13c — usage 结算行（`formatUsage`，`undefined` → `usage: n/a`）。
    - T15 — 重连 `session/subscribe` 续订 + `restoreTerminal` + SIGINT 优雅退出
      + mock WS kernel e2e（`run → progress → permission → respond → done`）。
  - P3 打包与发布：
    - T16 — `apps/desktop` Tauri 壳（`tauri.conf.json` `frontendDist` → `../../web/dist`，
      `version: 0.11.0`，targets all，`check-artifacts.sh` 平台校验）。
    - T17 — workspace version `0.10.0` → `0.11.0`
      （`Cargo.toml` + `Cargo.lock` + `sdk/config.ts` + `desktop/tauri.conf.json` + desktop package.json）。
    - T18 — 本次：doc sync `AGENTS.md`/`README.md`/summary（agentj-ui 阶段整体替换）
      + spec TODO 标记完成。

- ✅ **决策 D1-D8 兑现**：
  - D1 SDK 边界抽取、D2 生成而非手写、D3 纯 WS 客户端 + 指数退避、
    D4 TUI 全功能清单、D5 Web 完整交互清单、D6 Tauri 仅壳不嵌后端、
    D7 `git mv` 改名保留历史、D8 OpenTUI 兜底条件（未触发）。

### 验证结果

- `cargo test --workspace` — 全量通过
  （含既有 e2e_cli/e2e_permission/e2e_serve/e2e_ws + bus/协议/会话/模型 单测 + G7/G8 回归）
- `cargo test -p agentj-protocol -- drift` — 2 通过（33 schemas 无漂移，agentj-ui 为消费端零协议变化）
- `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
- `cargo fmt --all --check` — 通过
- `./scripts/check-version.sh` — `version-ok`（Cargo 0.11.0 + sdk/config 0.11.0 + desktop 0.11.0）
- `npm test --workspace=@agentj/sdk` — 通过（client/router/Bug-002/005/transport/config/methods）
- `npm run generate --workspace=@agentj/protocol && npm run build --workspace=@agentj/protocol` — 通过
- `npm test --workspace=@agentj/web && npm run build --workspace=@agentj/web` — 通过
- `bun test --prefix agentj-ui/apps/tui` — 通过（含 tui-flow + permission + palette + completion + theme + mock WS e2e）
- `./agentj-ui/apps/desktop/check-artifacts.sh linux` — 存在性校验通过（deb + AppImage，经 Tauri build 产出）
- `! grep -q 'web-dashboard/' README.md AGENTS.md` — 通过（已迁至 `agentj-ui/apps/web`）
- `grep -q 'agentj-ui' README.md AGENTS.md` + `grep -q '0.11.0' README.md AGENTS.md` — 通过

### 遗留问题 / 已知限制

- `@opentui/react` + `@opentui/core` 为 pre-1.0，含 6 平台预构建二进制；
  已通过 `testRender` spike 验证，仍需关注上游 breaking 变更。
- `session/list` 协议无此方法：Web 为 localStorage 收藏、TUI 为本地 `session-history.json`；
  凭收藏 ID 继续失败（已归档/删除）需手工新建并标记失效。
- TUI 模型选择器为只读：`SessionRunParams` 无 `model` 字段，当前仅展示
  `default_model` + providers + `config.toml [model] default_model` 提示；
  交互式下发为 NEXT-VERSION 目标（见下）。
- WSS/TLS、HTTP REST 面、CORS、每连接速率限流非本阶段目标，随产品需求独立设计。
- `cargo-dist` 二进制分发仅出评估结论，未在本阶段实现（Tauri 壳已覆盖安装包面）。
- 重连后 `session/subscribe` 续订依赖内存中 `activeSessionId` 存活；全新启动无会话则回 SessionList。
- gap 档案 G1-G8 + agentj-ui 全部 → ✅ 已实现，全闭环。

### 下一步

- v0.11.0 已全闭环，按发布流程执行：PR 合并 `feature/agentj-ui` → `main`，打 `v0.11.0` tag。
- 可选增强（非阻塞）：
  - `cargo-dist` 评估落地（`agentj` 二进制跨平台分发）。
  - WS WSS/TLS + 速率限流（随部署需求）。
  - 事件追溯日志 / `session/list` 服务端枚举（如产品决定）。

#### NEXT-VERSION 目标（已记录，不在本阶段实现）

- **交互式模型选择器**：为 `session/run` 增加加性 `model?: string` wire 字段
  （`SessionRunParams` 扩展，`skip_serializing_if` 保持向后兼容），
  前端（Web/TUI）模型选择器从只读升级为可下发：
  选中模型随 `session/run` 透传至服务端，服务端 `AgentLoopModule` 优先取
  请求级 `model` 覆盖 `[model] default_model`，未知模型容忍回退默认模型。
  设计与实施另起 feature，不改动 v0.11.0 已冻结的 16 方法面与 33 schemas 的只读消费假设。

### Spec TODO 更新

- `docs/superpowers/specs/2026-09-05-agentj-ui-design.md` TODO 已标记完成
  （P0-P3 全量兑现，见本文档验证结果；基准设计 D1-D8 无遗留）。
