# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui Visual 2.2 Opencode redesign（0.13.0）—— UnifiedSidebar 256px + local Project + composer 增强**
> ——设计文档 `docs/superpowers/specs/2026-09-06-agentj-opencode-redesign-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-06-agentj-opencode-redesign-plan.md`
> （11 任务 T0-T10，后端零改造，纯 `agentj-ui` 增量，0.12.3 → 0.13.0 minor）

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
| ✅ 完成 | **agentj-ui Visual 2.0 视觉重设计（v0.12.1）**（T0-T25：共享 tokens / trajectory+history / Web+TUI+Desktop 重设计 / brand 图标 + 4 acceptance fixes） | v0.11.1 | P0 | ✅ 已实现 v0.12.1 |
| ✅ 完成 | **agentj-ui Visual 2.1 refine（v0.12.3）**（T0-T10：tokens 19 keys + Web 精细化 shell/composer/session/conversation/trajectory/hero/dialog + lucide） | v0.12.1 | P0 | ✅ 已实现 v0.12.3 |
| ✅ 完成 | **agentj-ui Visual 2.2 Opencode redesign（v0.13.0）**（T0-T10：UnifiedSidebar 256px + local Project + composer enhance + density + TUI 32char + Desktop + a11y） | v0.12.3 | P0 | ✅ 已实现 v0.13.0 |

## Phase: agentj-ui Visual 2.2 Opencode redesign (v0.13.0) ✅ 已完成

### 进度与摘要

- ✅ **动机**：v0.12.3 已完成 Web 精细化但侧栏仍为 280px 扁平列表，与 Opencode Codex-style 统一 256px 差距显著。本次对标 Opencode 桌面 PR #20242，实现布局完全对标、色板混合保留（DeepSeek-blue 仍为主色）的 Visual 2.2。
- ✅ **方案**：`index.css` 新增 --aj-sidebar-width 256px/--aj-gap 12px/--aj-shadow-soft-sm/--aj-sidebar-bg-diff；新建 `UnifiedSidebar 256px`（Threads header + Filter/Sort/View + Add Project + 搜索 + body 按 projectMap 分组 + footer Settings/Help）；新建 `projectStore.ts`（localStorage agentj.projects/projectMap/collapsed/currentProject，dedup + corrupted fallback + groupSessionsByProject Today/7days/Older）；`App.tsx` 瘦身接入 UnifiedSidebar + status popover + projectMap 接线 + stale fallback；`RunPanel.tsx` Composer 增强（Project picker 下拉 + model chip + status popover + @/slash 前端本地）；`conversation.css` 密度收紧 gap 10→8 + Trajectory Project filter chips；`TUI sidebar.tsx` 文本版 32char（▶/▼ + indented + footer + projects.json 持久化）接入 app.tsx；版本同源 `0.13.0`（Cargo + sdk/config + desktop tauri.conf/package.json/src-tauri/Cargo）；`a11y`（aria-expanded/controls/selected + reduced-motion + CLS min-height 120px + >50 virtualize 分页 + unreachable 灰化）。
- ✅ **任务分解（T0-T10）已完成**：T0 density vars / T1 UnifiedSidebar shell / T2 projectStore / T3 App Shell slim / T4 Composer enhance / T5 density + Trajectory filter / T6 TUI 32char / T7 SDK 0.13.0 / T8 Desktop 0.13.0 / T9 a11y/perf / T10 gates + docs sync。

### 验证结果

- `cargo test --workspace -- --skip permission_request_notified` — 通过
- `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
- `cargo test -p agentj-protocol -- drift` — 通过（2 drift checks）
- `./scripts/check-version.sh` — version-ok（Cargo 0.13.0 + sdk VERSION 0.13.0 + PROTOCOL_VERSION 0.12.1 + desktop 0.13.0）
- `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — desktop-ui-ok（900/600/1100/760）
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — 通过（29 tests）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — 通过（13 files, 115 tests）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — 通过（1836 modules, 400kB）
- `bun --cwd agentj-ui/apps/tui test` — 通过（69 tests, 176 expects）

### 遗留问题 / 已知限制

- Project 仅前端本地 `localStorage` + `projects.json`，后端零改造；多项目文件树/分支选择为后续 feature。
- TUI mock WS e2e 覆盖 sidebar 32char 与 projects.json 持久化；`@opentui/react` pre-1.0 上游关注不变。
- Desktop 仅随 Web 产物与版本同步，无额外窗口质感变更（Tauri v2 壳）。

### 下一步

- 合并 `feature/agentj-opencode-redesign` → `main`，打 `v0.13.0` tag，发布。
- 运行 `/jskill-test-acceptance` 自动验收 → 验收 PASS 后自动创建 PR（含验收修复 + patch bump）。
- PR 合并后清理 acceptance 产物（如有）。

### Spec TODO 更新

- `docs/superpowers/specs/2026-09-06-agentj-opencode-redesign-design.md` TODO 已全部勾选（T0-T10）。
- `docs/superpowers/specs/2026-09-06-agentj-web-refine-design.md` 保持已完成（v0.12.3）。
- `docs/superpowers/specs/2026-09-06-agentj-ui-visual-design.md` 保持已完成（v0.12.1）。
