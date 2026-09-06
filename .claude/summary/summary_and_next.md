# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui Visual 2.0 视觉重设计（DeepSeek-blue rebrand + 共享 tokens + trajectory/history 表面 + Web/TUI/Desktop 重设计，v0.12.1）**
> ——设计文档 `docs/superpowers/specs/2026-09-06-agentj-ui-visual-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-06-agentj-ui-visual-plan.md`
> （26 任务 T0-T25，按依赖分层编排（root 层 6 个并行任务 → … → 版本/文档收尾），标题标注 P0-P4 组织波；v0.12.1 实现完成 + acceptance 4 fixes verified）。

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
| ✅ 完成 | **agentj-ui Visual 2.0 视觉重设计（v0.12.1）**（T0-T25：共享 tokens / trajectory+history / Web+TUI+Desktop 重设计 / brand 图标 + 4 acceptance fixes B-101..B-104） | v0.11.1 | P0 | ✅ 已实现 v0.12.1 |

## Phase: agentj-ui Visual 2.0 视觉重设计 (v0.12.1) ✅ 已完成（验收通过）

### 进度与摘要

- ✅ **根因/动机**：v0.11.1 三端功能已闭环，但视觉仍是 per-app 硬编码色板（Web/TUI 各自 Tokyo Night 系、Desktop 68 字节占位图标），色彩/资产无单一来源；spec 定稿 Visual 2.0 DeepSeek-blue rebrand（#4176E6/#679EFE）+ trajectory/history 后端表面，v0.12.1 minor + patch。
- ✅ **方案**（design 定稿）：`packages/tokens` 共享语义 tokens（15 key × dark/light + SEMANTIC_KEYS，三端同源）；Rust 侧加 trajectory wire kinds + session/list（store 枚举）+ history NDJSON 回放存根 + kernel/status 字段 + run-path 广播与 respond 落盘；Web 三色系统重设计（Conversation timeline / ApprovalDialog / SessionPanel / App shell）；TUI theme 别名共享 palette + 标准帧 transcript（RunTranscript/rowsFromFrame）+ App 装配（实时 transcript + composer）；Desktop brand 图标（1024 主图 + tauri icon 六件套）+ 900px 最小窗口；统一 0.12.1 发布（T23 workspace+SDK / T24 desktop）+ docs 同步（T25）+ acceptance 4 fixes。
- ✅ **任务分解（T0-T25，26 任务）已完成**：P0 基建（T0 tokens / T1 favicon / T2 wire kinds / T3 trajectory types / T6 SessionStore::list / T7 TrajectoryStore / T22 图标与窗口）；P1 后端波（T4 session+history wire / T5 方法注册表 / T8 kernel 字段 / T9-T11 广播/落盘/派发 / T12 parseProgress）+ web tokens.css（T13）；P2 Web 波（T14 timeline / T15 ApprovalDialog / T16 SessionPanel / T17 App shell）；P3 TUI 波（T18 theme / T19 mock-kernel / T20 runview / T21 App 装配）；P4 收尾（T23 workspace 0.12.1 / T24 desktop 0.12.1 / T25 docs）+ acceptance 4 fixes（B-101 prompt→content, B-102 allow_always decision, B-103 reduceRunFeed, B-104 accentOnAccent variable）。

### 验证结果（dev + acceptance 末端，全量重跑）

- `cargo test --workspace -- --skip permission_request_notified` — 通过（415 passed）
- `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
- `cargo fmt --all --check` — 通过
- `cargo test -p agentj-protocol -- drift` — 通过（schemas 无漂移）
- `./scripts/check-version.sh` — `version-ok`（Cargo 0.12.1 + sdk/config 0.12.1 + desktop 0.12.1）
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — 29 passed
- `npm --prefix agentj-ui test --workspace=@agentj/web` — 71 passed
- `bun --cwd agentj-ui/apps/tui test` — 63 pass
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — 278 modules, 372kB
- `cd agentj-ui/apps/desktop && node check-desktop-ui.mjs` — `desktop-ui-ok`
- **验收**：4 bugs (B-101 high TUI prompt, B-102 high allow_always, B-103 medium reduceRunFeed, B-104 low CSS variable) → 4/4 fixed and verified in 1 regression round, 门禁 10/10 pass

### 遗留问题 / 已知限制

- trajectory/history 为 design 界定范围：session/list 走 store 枚举，history 以 NDJSON 事件落盘 + 回放为基线；真实事件溯源索引/回放仍在后端 roadmap。
- TUI 模型选择器保持只读（v0.11.1 决定）；交互式下发仍为 NEXT-VERSION（`session/run.model` 加性字段）。
- @opentui/react pre-1.0 上游关注不变；Web 主题三态（system-follow，C7）、TUI 两态，色板均来自 `@agentj/tokens`。

### 下一步

- 合并 `feature/agentj-ui-visual` → `main`，打 `v0.12.1` tag，发布。
- 可选：spec TODO 打勾，PR 合并后清理 `.superpowers/acceptance/bugs.md`（acceptance 产物）。

### Spec TODO 更新

- `docs/superpowers/specs/2026-09-06-agentj-ui-visual-design.md` TODO 待验收后标记（设计已冻结，T0-T25 + 4 fixes 实现完成，验收通过）。
