# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui 三端统一前端（Web / TUI / Desktop + SDK，v0.11.1）**
> ——设计文档 `docs/superpowers/specs/2026-09-05-agentj-ui-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-05-agentj-ui-plan.md`
> （web-dashboard → agentj-ui 重命名 + SDK 抽取 + Web 对齐 + TUI 全量 + Tauri 打包，v0.11.0 交付 + v0.11.1 acceptance 9 fixes）。

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
| ✅ 完成 | **agentj-ui 三端统一前端（Web / TUI / Desktop + SDK，v0.11.1）**（T0-T18 + 9 acceptance fixes） | v0.10.0 | P0 | ✅ 已实现 v0.11.1 |

## Phase: agentj-ui 三端统一前端 (v0.11.1) ✅ (本次完成)

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

- ✅ **任务分解（T0-T18）已完成**：见 v0.11.0 交付（P0基建 T0-T5，P1 Web T6-T7c，P2 TUI T8-T15，P3 打包 T16-T18，version 0.11.0）。

- ✅ **验收 fixes（v0.11.1 patch，9 bugs，B-001..B-009）**：
  - B-001 high — `RpcClient.initialize()` 默认 `0.4.0` → `PROTOCOL_VERSION=0.11.1`（`sdk/src/client.ts`，import 单一来源，新增 `init-default.test.ts`）。
  - B-002 medium — Web `node:net` externalized：`sdk/src/index.ts` 移除 `TcpTransport` barrel 导出，`package.json` 增 `exports ./transports/tcp`，Web build 26 modules 零警告。
  - B-003 high — `resolveWsUrl` 丢 token 当 `envUrl`：`sdk/src/config.ts` 改为 `?`/`&` 拼接 `encodeURIComponent(token)`，`envUrl` 含 query 时正确。
  - B-004 high — TUI `formatUsage` `input_tokens`→`prompt_tokens`：`tui/src/runview.tsx` type + 模板对齐 `types.d.ts`/`session.rs`，`footer.test.tsx` 同步。
  - B-005 medium — `ConnectionManager` 403 仍重连：`tui/src/connection.ts` `handleStatus closed` 加 `isAuthError`（403/-32806）守卫，停止指数退避，与 App 一致。
  - B-006 medium — `tui/src/e2e/mock-kernel.ts` usage `input_tokens`→`prompt_tokens`，e2e 不再假绿，与 B-004 对齐。
  - B-007 medium — Web 引导 `agentj dashboard --port 4096` 恒 4096：`apps/web/src/App.tsx` hint 改为 `new URL(resolveWsUrl({envUrl:envWsUrl})).port || fallback`，支持 `VITE_AGENTJ_WS_URL` 端口。
  - B-008 low — `ModelSelector` 硬编码 `deepseek-chat`：`tui/src/models.tsx` 增 `MODEL_CONFIG` + `useModelConfig(client)` via `kernel/status`，`app.tsx` 消费变量而非字面量。
  - B-009 low — TUI tests `act` 警告 42→0：5 个 test 文件包 `act`，`bun test` 50 pass 干净。

- ✅ **决策 D1-D8 兑现**：同 v0.11.0，验收后全保留。

### 验证结果（回归轮 2026-09-06）

- `cargo test --workspace -- --skip permission_request_notified` — pass 7.6s
- `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
- `cargo fmt --all --check` — 通过
- `cargo test -p agentj-protocol -- drift` — 2 passed（33 schemas）
- `./scripts/check-version.sh` — `version-ok`（Cargo 0.11.1 + sdk/protocol 0.11.1 + desktop 0.11.1）
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — 20 passed（B-001/B-003 新增）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — 14 passed（B-007 新增）
- `bun test agentj-ui/apps/tui` — 50 passed 0 fail 0 act warnings（此前 47 + act 警告）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — 26 modules，无 `node:net`（此前 28 + warning）
- `./agentj-ui/apps/desktop/check-artifacts.sh unknown` — `unknown platform unknown` + exit 1 正确；`linux` 无 bundle 时 exit 1 预期
- `! grep -q 'web-dashboard/' README.md AGENTS.md` — 通过
- `grep -q 'agentj-ui' && grep -q '0.11.1'` — 通过
- 回归验证：B-001..B-009 9/9 `verified`（0 open，0 回退，0 新 Bug，1 轮回归）

### 遗留问题 / 已知限制

- `@opentui/react` + `@opentui/core` pre-1.0，6 平台预构建；act 警告已清，仍需关注上游 breaking。
- `session/list` 无方法：Web localStorage + TUI `session-history.json` 弥补；`cargo-dist` 仅评估未实现。
- TUI 模型选择器现已从 `kernel/status` 动态读取 `default_model`（B-008），但仍为只读展示；交互式下发仍为 NEXT-VERSION 目标（`session/run.model` 加性字段）。
- WSS/TLS、HTTP REST、CORS、速率限流非本阶段目标。
- gap 档案 G1-G8 + agentj-ui → ✅ 全闭环 v0.11.1。

### 下一步

- v0.11.1 已全闭环，按发布流程：PR 合并 `feature/agentj-ui-unified-frontend` → `main`，打 `v0.11.1` tag。
- 可选增强（非阻塞）：`cargo-dist` 落地、WSS/TLS、事件追溯日志 / `session/list` 枚举（如产品决定）。

#### NEXT-VERSION 目标（已记录，不在本阶段实现）

- **交互式模型选择器**：为 `session/run` 增加加性 `model?: string` wire 字段
  （`SessionRunParams` 扩展，`skip_serializing_if` 保持向后兼容），
  前端（Web/TUI）模型选择器从只读升级为可下发：
  选中模型随 `session/run` 透传至服务端，服务端 `AgentLoopModule` 优先取
  请求级 `model` 覆盖 `[model] default_model`，未知模型容忍回退默认模型。
  设计与实施另起 feature，不改动已冻结的 16 方法面与 33 schemas 的只读消费假设。

### Spec TODO 更新

- `docs/superpowers/specs/2026-09-05-agentj-ui-design.md` TODO 已标记完成
  （P0-P3 全量兑现 + 9 acceptance fixes，v0.11.1）。
