# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.24.2 右栏遮挡修复（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）**
> ——来源：用户直报「拖拽右边栏运行详情展宽时，文字逐渐被中间栏右侧遮挡」。
> 前端范围：仅 `agentj-ui/apps/web`（`layout.css` 三栏 flex 契约 + `workspaceShell.test.tsx` CSS 契约守卫）；工作区 0.24.1 → **0.24.2**。后端部分未改动，`summary_and_next_se.md` 不变。

## v0.24.2（前端部分）✅（本次完成）

- ✅ **根因（三层，实机复现）**：① `.aj-shell__center { flex: 1 1 auto }` 基准取内容宽度——中栏长代码块/宽工具卡把 flex 行撑破；② 行溢出时 `.aj-shell__right { flex: 0 1 auto }`（可收缩）被压到 `--aj-right-w` 以下；③ 内层 `.aj-right` 宽度固定 `var(--aj-right-w)` + `align-self: flex-end` 右缘锚定，左侧溢出被 `overflow: hidden` 裁剪，又因中栏 `position: relative`（定位元素绘制在非定位兄弟之上）被中栏盖住 → **遮挡量随拖拽宽度线性增长**，正是用户观察到的「逐渐被中间栏右侧遮挡」。
- ✅ **修复（两行 CSS）**：中栏 `flex: 1 1 0`（基准 0，宽度只取剩余空间，内容不再撑破行，仍受 `min-width: 480px` 保护）；右栏 `flex: 0 0 auto`（用户拖出的宽度权威，`flex-basis: auto` 仍取 `width`，v0.16.5 折叠揭示动画与 `data-dragging` 过渡守卫全部保持不变）。
- ✅ **回归守卫**：`workspaceShell.test.tsx` 新增 CSS 契约测（右栏 `0 0 auto` 且不得回退 `0 1 auto`；中栏 `1 1 0` 且不得回退 `1 1 auto`），并同步既有断言；web 610 → **611 tests（52 files）**。

## 实机验证（Chromium headless，Vite dev `?demo=1`）

| 场景 | 修复前 | 修复后 |
|---|---|---|
| 中栏 1600px 超宽内容 + 拖拽展宽 8 帧 @1440 | 遮挡 6/6 帧（inner 440px vs outer 254px，溢出 186px） | 遮挡 0/8 帧 |
| 1024 / 1100 / 1280 / 1440 × 空内容 + 超宽内容，各拖拽 8 帧 | — | 遮挡 0/32 帧，`--aj-right-w` 与实测宽逐帧一致 |
| 折叠 → 展开 16 帧 | — | 内层 `.aj-right` 宽度恒 480（零回流揭示保持），外层 480→40 单调 |

## 门禁（2026-09-29）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **611 passed（52 files）**（610 → +1 CSS 契约守卫） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | **54 passed（6 files）**（仅版本串 0.24.1 → 0.24.2） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（dist/version.json product_version **0.24.2** / protocol 0.15.0） |
| `./scripts/check-version.sh` | version-ok（0.24.2；PROTOCOL_VERSION 0.15.0） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |
| `cargo test --workspace --no-fail-fast` | Rust 侧无代码改动；全绿除**既有环境性失败** `agentj-cli e2e_serve_exits_gracefully_on_sigterm`（SIGTERM 未被优雅处理 → exit 15；已用 `git stash` 在干净树上复现同一失败，与本次变更无关） |
| `bun --cwd agentj-ui/apps/tui test` | **未执行**：本机未安装 `@opentui/core` / `@opentui/react`（`bun --cwd agentj-ui/apps/tui install` 未跑过），11 个文件报 `Cannot find module '@opentui/react/jsx-dev-runtime'`；本次未触碰 TUI 任何文件 |

## 遗留问题 / 已知限制（前端）

- **中栏内容不再影响列宽**：`flex: 1 1 0` 后超宽内容在中栏内部裁剪（`.aj-shell__center { overflow: hidden }`），代码块自身 `overflow-x` 负责横向滚动；这是「右栏宽度权威」的对价，若未来需要中栏整体横向滚动需另行设计。
- **键盘调宽的 8px 瞬时差**：键盘改宽时外层走 200ms 过渡而内层立即跳变，动画期内左缘最多 8px 被裁；拖拽路径（`data-dragging` 守卫）无此问题。未修：给内层加宽度过渡会破坏 v0.16.5「折叠揭示零回流」契约（`rightPanel.test.tsx` 显式守卫 `.aj-right` 无 transition）。
- **TUI 依赖未安装**（见门禁表），需 `bun --cwd agentj-ui/apps/tui install` 后复跑 tui 门禁。
- **run 分组仅运行期**：`summary`/`history`/`activeRunId` 为内存态（切换会话重置），无 localStorage 持久化；`__legacy__` 桶与当前 run 桶并存，跨 run 不迁移。
- **history 无上限**：历史抽屉 append-only 无分页/截断，超长会话依赖 `session/history` 回放边界。
- **TUI 侧未发 project_root**：展开态键仍为内核 cwd 回退（与后端遗留一致）。
- **Web 无搜索态**：设计非目标 ③，`sidebarHint` 搜索态文案组件存在但不可达（与 v0.22.0 遗留一致）。
- **收缩态为会话内瞬态**：不持久化，切会话/刷新默认展开（设计非目标 #1，符合预期）。
- **`.aj-conv` 不可聚焦**：键盘滚动不可达（不新增 `tabindex`，避免与现有焦点模型/键位冲突）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- v0.24.2 右栏遮挡修复完成（实机回归 0 遮挡帧，web 611 tests）；待提交/推送。
- 下一特性从「待实施」表选择（候选 ③ Web 接入服务端搜索；后端候选见 `summary_and_next_se.md`）。
