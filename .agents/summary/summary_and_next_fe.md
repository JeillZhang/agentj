# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.23.1 补齐 v0.23.0 计划遗漏（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）——前端部分**
> ——来源：v0.23.0 验收 PASS（BUG-013/014/015 verified per `.superpowers/acceptance/bugs.md`）后复核发现前端状态拆分、右栏分组接线、样式与计划测试缺漏。
> + 实现计划 `docs/superpowers/plans/2026-09-25-agentj-right-panel-run-details-plan.md`（T2/T6–T9 前端侧补全）。
> 前端范围：`agentj-ui/packages/sdk`（T2 parseProgress 容忍补测 7）+ `agentj-ui/apps/web`（`changes.ts` 状态拆分 + `ChangeList`/`RightPanel`/`App` 接线 + `rightpanel.css` token 化样式 + 21 补测）+ `agentj-ui/apps/tui`（T9 per-run 门控 4 补测）；工作区 0.23.0 → **0.23.1**。后端部分见 `summary_and_next_se.md`。

## v0.23.1（前端部分）✅（本次完成）

- ✅ **(a) changes 状态拆分**：`changes.ts` 归约产物由单一数组升级为 `ChangesState {summary, history}`——`summary` 按 `(run_id, path)` upsert 覆盖为本次运行汇总；`history` 按设计决策 #3/#15 **append-only**（不去重、按 `seq`/`eventSeq` 排序、按 `tool_call_id` 分组），修复此前汇总覆盖导致历史抽屉无数据的缺陷（抽屉现已可达）。
- ✅ **(b) 右栏分组真正接线**：`ChangeList.tsx` 消费新 `history` prop（历史抽屉可达）；`RightPanel.tsx` run 分组（`RunGroup`）经 `App.tsx` 传入 `groups`/`history`/`highlightedPath`/`onSelectRun` 真正落地——run 组头可点击切换当前 run、每组独立空态（`暂无任务`/`暂无变更`）；无分组时保留 children/手风琴回退路径。
- ✅ **(c) token 化样式补齐**：`rightpanel.css` 新增 `run-group*`（组头/当前高亮/8-char short id）、`history-drawer`、`hist-*`（历史行）、`trunc-hint`（`已截断（原始 N KiB）`）、`src-badge*`（write/bash 徽标）——此前均为无样式裸节点，全部改用语义 token。
- ✅ **(d) 计划缺失测试补齐（前端侧）**：SDK T2 7 测（todo `run_id`、file_change `source`/`seq`/`total_bytes`、未知 source、旧数据缺 source、非法 `run_id` 丢弃、內外层 `run_id` 优先级、容忍回退）；web +21 测（per-run todo 门控、summary 隔离、append-only 历史序、抽屉 2 条、截断 hint `已截断（原始 69 KiB）`、bash 徽标/tooltip、run 分组头/点击、App `run_id` 捕获 + `session/history {run_id}` 请求、`data-highlight`）；TUI T9 4 测（todo per-run 门控/陈旧、新 run revision 1、非法 `run_id` → legacy 桶、file_change 忽略）。
- ✅ **(f) 用户直报修复（空态文案 + 分组丢失）**：右栏空态文案 `待协议支持` → `暂无任务`/`暂无变更`（`TodoPanel` 两处 + `ChangeList` 默认 `emptyText`）；`handleSelectRun` 由对象展开改为 `todoState.focusRun`（保留非枚举 `byRun`/`runId`/`currentRunId`），修复点击 run 组头后分组丢失、`runGroups` 为空回退旧空态；新增 `focusRun` 回归 2 测（切换桶 + 保留元数据、未知 run 同引用）。

## 验证结果（前端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | **54 passed（6 files）**（47 → +7） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **580 passed（51 files）**（557 → +23） |
| `bun --cwd agentj-ui/apps/tui test` | **146 passed（19 files）**（142 → +4） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（dist/version.json product_version 0.23.1 / protocol 0.15.0） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> **v0.23.0 验收：PASS**（BUG-013/014/015 verified per `.superpowers/acceptance/bugs.md`；回归后 web 552 → 557）。本波 v0.23.1 为计划遗漏补齐，**待 `jskill-test-acceptance` 复验**——前端验收项：历史抽屉可达（summary/history 拆分）、run 分组头点击切换、每组独立空态、无分组回退、`run-group*`/`history-drawer`/`src-badge*` 样式、`已截断（原始 N KiB）` hint；后端门禁与验收项见 `summary_and_next_se.md`。

## 遗留问题 / 已知限制（前端）

- **run 分组仅运行期**：`summary`/`history`/`activeRunId` 为内存态（切换会话重置），无 localStorage 持久化；`__legacy__` 桶与当前 run 桶并存，跨 run 不迁移。
- **history 无上限**：历史抽屉 append-only 无分页/截断，超长会话依赖 `session/history` 回放边界；`sortBySeq` 对无 `seq` 的旧数据回退 `eventSeq`（TrajectoryEvent.seq），无 `eventSeq` 时保持稳定序。
- **TUI 侧未发 project_root**：展开态键仍为内核 cwd 回退（与后端遗留一致）；rank 三处镜像问题待后端 ④。
- **Web 无搜索态**：设计非目标 ③，故 `sidebarHint` 搜索态文案组件存在但不可达（与 v0.22.0 遗留一致）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- **v0.23.1 计划遗漏补齐已在开发分支落地，待 `jskill-test-acceptance` 复验后合并 main**；当前功能完结，下一特性从「待实施」表选择（`jskill-feature-next` 合并本表 + `_se.md` 表）。
- 前端候选即上表 ③（后端候选见 `summary_and_next_se.md`）。
