# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.24.0 中栏 Composer 滚动自动收缩（纯前端，无协议变更，PROTOCOL_VERSION 保持 0.15.0）——前端部分**
> ——来源：`docs/superpowers/specs/2026-09-25-agentj-composer-scroll-collapse-design.md` + 实现计划 `docs/superpowers/plans/2026-09-25-agentj-composer-scroll-collapse-plan.md`（T0–T7）。
> 前端范围：仅 `agentj-ui/apps/web`（`composerCollapse.ts` 纯状态机 + `ConversationFeed` 意图判定 + `RunPanel` 接线 + `ComposerOptions` `collapsed` prop + `composer.css` 契约）；工作区 0.23.1 → **0.24.0**。后端部分未改动，`summary_and_next_se.md` 不变。

## v0.24.0（前端部分）✅（本次完成）

- ✅ **(a) `composerCollapse.ts` 纯状态机**：`collapsed` 迁移规则从 DOM 剥离（先例 `changes.ts`/`todoState`），48/40 迟滞带 + 迁移守卫（待答提问 > 上滚收缩）+ 260ms 动画锁；7 单测覆盖迁移全表、守卫优先级、迁移幂等。
- ✅ **(b) `ConversationFeed` 意图双判 / 程序化排除**：① `wheel`/`touchmove` 手势；② 非程序化 `scroll` 方向（覆盖滚动条拖拽/惯性）；程序化 `scrollIntoView`（流式自动跟随）由自标记排除，不触发收缩；5 单测。
- ✅ **(c) `RunPanel` 状态接线 + `ComposerOptions` prop**：`data-collapsed` 属性；发送按钮双职责（收缩态 `ChevronsUp`「展开输入框」、`aria-label`/`aria-expanded`/`aria-controls` 同步、X/Y 零位移）；待答提问守卫与强制展开；收缩期冻结 `--aj-composer-h`；收缩同时关闭 composer 弹层；点击路径 `focus(textarea, preventScroll)`；`ComposerOptions` 新增 `collapsed` prop；10 单测。
- ✅ **(d) `composer.css` 分段收拢契约 + reduced-motion**：header / 输入区 / 工具栏选择器逐段收拢为 0 高、选择器淡出且不占位、按钮行高与按钮不参与收拢；过渡使用 `--aj-motion`/`--aj-ease`；reduced-motion 复用既有 `@media (prefers-reduced-motion: reduce)` + `:root[data-reduced-motion="true"]` 双层机制；4 CSS 契约单测。
- ✅ **(e) 版本 T4–T6**：Cargo workspace + `Cargo.lock`（17 成员）+ `scripts/check-version.sh`（5 断言）+ sdk `VERSION`/config 测试 + desktop 三清单与 lockfile 同步至 0.24.0；`PROTOCOL_VERSION` 保持 0.15.0。

## 验证结果（前端门禁，2026-09-26）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **606 passed（52 files）**（580 → +26） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | **54 passed（6 files）**（本侧未改；Rust / protocol 不受本侧影响） |
| `bun --cwd agentj-ui/apps/tui test` | **146 passed（19 files）**（本侧未改） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（dist/version.json product_version 0.24.0 / protocol 0.15.0） |
| `./scripts/check-version.sh` | version-ok（0.24.0；PROTOCOL_VERSION 0.15.0；methods.json 0.15.0） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> 本侧为纯前端特性，Rust / protocol 未改动（cargo fmt / 660 Rust tests / clippy / drift 见后端门禁，非本侧变更）。实机 Playwright 验收**待 `jskill-test-acceptance` 执行**——验收项：发送按钮 X/Y 零位移（±1px）/ 200ms 平滑动画 / reduced-motion 瞬切 / 窄视口（≤390px）无溢出 / 亮暗主题无残留 / 草稿保留 / 程序化流式跟随不收缩 / 待答提问强制展开。

## 遗留问题 / 已知限制（前端）

- **run 分组仅运行期**：`summary`/`history`/`activeRunId` 为内存态（切换会话重置），无 localStorage 持久化；`__legacy__` 桶与当前 run 桶并存，跨 run 不迁移。
- **history 无上限**：历史抽屉 append-only 无分页/截断，超长会话依赖 `session/history` 回放边界；`sortBySeq` 对无 `seq` 的旧数据回退 `eventSeq`（TrajectoryEvent.seq），无 `eventSeq` 时保持稳定序。
- **TUI 侧未发 project_root**：展开态键仍为内核 cwd 回退（与后端遗留一致）；rank 三处镜像问题待后端 ④。
- **Web 无搜索态**：设计非目标 ③，故 `sidebarHint` 搜索态文案组件存在但不可达（与 v0.22.0 遗留一致）。
- **收缩态为会话内瞬态**：不持久化，切会话/刷新默认展开（设计非目标 #1，符合预期而非缺陷）。
- **`.aj-conv` 不可聚焦**：键盘滚动不可达（不新增 `tabindex`，避免与现有焦点模型/键位冲突）；作为扩展点记录：若未来 feed 可聚焦，按相同方向语义纳入。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- **v0.24.0 已开发完成，待 `jskill-test-acceptance` 实机验收 → PASS 后自动创建 PR**（用户负责合并）。
- 下一特性从「待实施」表选择（候选 ③ Web 接入服务端搜索；后端候选见 `summary_and_next_se.md`）。
