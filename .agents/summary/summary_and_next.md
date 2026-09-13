# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui 三栏工作台（0.14.4）—— Sessions/Files 左栏 + 会话中栏 + 运行详情右栏 + composer 模型/Agent/熵变选择器 + 模型提问卡片 + demo 模式**
> ——设计文档 `docs/superpowers/specs/2026-09-13-agentj-three-column-workbench-design.md`
> + 实现计划 `.superpowers/sdd/2026-09-13-agentj-three-column-workbench-plan/`
> （T0-T28 + final review fix wave + acceptance 3 bugs + 评审反馈修复 1 bugfix + 评审反馈修复 3 项 + 评审反馈修复 1 项（弹窗 portal），后端零改造，纯 `agentj-ui` 增量，0.13.0 → 0.14.4 patch）

## 进度与摘要

- ✅ **T0-T28 全部完成并提交**（分支 `feature/agentj-three-column-workbench`）。
- ✅ **T0-T2 数据与 tokens**：`palette.ts` 24 keys（radiusCtl/radiusCard/accentTint/diffAddTint/diffDelTint）+ `LAYOUT` 常量块；`deriveChanges` 从 write tool_call 派生变更行；`demoData`（agentj.demo / ?demo=1）驱动 demo。
- ✅ **T1-T7 布局原语**：`layoutStore`（宽度/折叠/持久化）+ `SplitPane`（拖拽/键盘/双击/separator a11y）+ `demoData` + `index.css` tokens 对齐 + radius 迁移 batch A/B + `TopbarMenu`。
- ✅ **T8-T24 组件与接线**：`TodoPanel` + `ChangeList`（unified diff 展开）+ `FileTree`（懒展开 + git 状态点）+ `WorkspaceShell` 三栏 + `RightPanel`（折叠导轨）+ `SidebarTabs` + `UnifiedSidebar` 重构 + `OptionPopover` + `ComposerOptions`（模型/Agent/熵变互斥 + 持久化）+ `AskQuestionCard`（折叠 + 问答记录）+ `App.tsx` 集成 + `<1024` 右抽屉 + a11y 门禁。
- ✅ **T25-T27 版本同步**：Cargo workspace + SDK `VERSION` + Desktop 壳 + `check-version.sh` 全部 0.14.0（`PROTOCOL_VERSION` 保持 0.12.1）。
- ✅ **T28 全量门禁 + 文档同步**：summary + `AGENTS.md` + `README.md` + spec TODO 刷新。
- ✅ **final review fix wave**（`fb4fd7f`）：splitter 几何、popover focus、filetree footer、sidebar rows、tab copy 收口修复。
- ✅ **验收 3 bugs 修复并回归验证**：BUG-001 右分栏折叠态锁定（`ad8e537`）/ BUG-002 提问卡折叠态摘要（`672209a`）/ BUG-003 短视口 composer 裁剪（`dab74c6`），acceptance 回归 3/3 verified。
- ✅ **验收后 patch bump 0.14.0 → 0.14.1**：Cargo workspace + Cargo.lock + SDK `VERSION` + Desktop 壳 + `check-version.sh` + 文档自动同步。
- ✅ **评审反馈修复（`7f2ef33`）**：移除 unreachable 时三栏间渲染的无样式 standalone dashboard 提示行（`App.tsx`），消除左栏与顶栏视觉割裂；顶栏状态行与左栏 `aj-sidebar--unreachable` 灰化保持不变；原 b007.repro 提示行测试随功能移除而废弃，App.test 改为断言提示行不存在。**验收数据不变**，随后 patch bump 0.14.1 → 0.14.2。
- ✅ **评审反馈修复 3 项（`ad3beeb`）**：① `.aj-right` 由内容高度改为 `flex:1 1 auto; min-height:0`，右栏始终从顶栏下沿延伸至视口底部（CDP：展开/折叠、demo 开/关 `bottom === innerHeight`）；② `TodoPanel`/`ChangeList` 标题行改为 `<button className="sec-head" aria-expanded aria-controls>`，点击折叠/展开，折叠后仅保留标题条 + 计数，body 始终挂载（空态不变）；③ `.sec-head` 重写为 14px/600 无边框无底色按钮行，与 `.aj-right__title` 风格统一（mockup 的「11px uppercase caption」被用户直需求覆盖）。随后 patch bump 0.14.2 → 0.14.3。
- ✅ **评审反馈修复 1 项（`9addeb7`）**：顶栏「连接设置…/继续会话…」弹窗改用 `createPortal` 渲染到 `document.body`，逃离 `.aj-topbar` 的 `backdrop-filter: blur(8px)` 包含块（否则 `position: fixed` 相对约 64px 顶栏定位，卡片被挤出视口顶部）；同时给 `.aj-dialog` 遮罩容器加 `onMouseDown`（点遮罩关闭、点卡片内部不关闭，X/Esc 行为保留）。CDP：`.aj-dialog-card` `parentElement === document.body`，1440×813 视口下四边均在视口内，`dCenterX=0`、`dCenterY=-0.01`；遮罩 (20, innerHeight/2) mousedown 后弹窗消失。随后 patch bump 0.14.3 → 0.14.4。

## 验证结果（验收 + bump 门禁，逐条记录）

1. `npm --prefix agentj-ui test --workspace=@agentj/web` — ✅ 28 files / 182 tests passed。
2. `npm --prefix agentj-ui run build --workspace=@agentj/web` — ✅ `tsc -b && vite build` 通过。
3. `npm --prefix agentj-ui test --workspace=@agentj/sdk` — ✅ 30 passed。
4. `cargo test --workspace` — ✅ 414 passed。
5. `cargo clippy --workspace --all-targets -- -D warnings` — ✅ 零警告。
6. `cargo test -p agentj-protocol -- drift` — ✅ 2 passed（33 schemas 无漂移）。
7. `bun --cwd agentj-ui/apps/tui test` — ✅ 69 pass / 0 fail。
8. `./scripts/check-version.sh` — ✅ `version-ok`（Cargo 0.14.4 + sdk 0.14.4 + PROTOCOL_VERSION 0.12.1 + desktop 0.14.4）。
9. `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — ✅ `desktop-ui-ok`。
10. acceptance 回归 — ✅ 3/3 verified。
11. CDP（chrome headless 1440×900，`http://127.0.0.1:8092/?demo=1`）— ✅ 右栏 bottom === innerHeight（展开/折叠/demo 关均 delta 0）；标题点击折叠/展开 + aria-expanded 切换；截图 `rp-expanded.png`/`rp-folded.png`。
12. CDP 弹窗 portal（chrome headless 1440×900，`http://127.0.0.1:8093/`）— ✅ `.aj-dialog-card` `parentElement === document.body`；卡 rect `left=440 top=225 right=1000 bottom=588`（视口 1440×813，四边均在视口内），`dCenterX=0`、`dCenterY=-0.01`；遮罩 (20, innerHeight/2) mousedown 后 `[aria-label="连接设置"]` 消失；截图 `dialog-centered.png`。

## 遗留问题 / 已知限制

- **协议 gap（阶段二待协议支持）**：无 TODO 事件、无文件 diff 事件、无目录列举方法 → 以 demo/空态呈现（`TodoPanel`/`FileTree` 显示「待协议支持」，`ChangeList` 仅渲染真实派生数据，不伪造 diff）。
- `$/askQuestion` + `question/respond` 未实现，`AskQuestionCard` 当前仅由 demo 驱动。
- 后端 `session/status` 对未知会话返回 `-32601`（方法未实现观察项）。
- protocol-version banner 与 `PROTOCOL_VERSION` 展示存在不一致观察项。
- T23 后「继续会话」不再自动收藏该会话，且连接失败时缺少错误反馈。
- Desktop 独立 `src-tauri/Cargo.lock` 仍停留在旧版本 `agentj-ui 0.11.0`（历史遗留，非本次 bump 范围）。

## 下一步

- push 分支 + 创建 PR（用户负责合并）。
- 合并后打 `v0.14.4` tag 并清理 acceptance 产物。
- 阶段二（独立 feature）：按 `protocol-extension-prompt.md` 落地 TODO 事件 / 文件变更事件 / `file/list` / `$/askQuestion` + `question/respond`，替换 demo 数据源。
