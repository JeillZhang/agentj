# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.23.0 右边栏运行详情 — run_id 维度 + FileChangeSource + total_bytes（七处加性 wire，PROTOCOL 0.14.0 → 0.15.0）——前端部分**
> ——来源：用户请求「右边栏运行详情中，任务与文件变更协议设计实现」+ 设计文档
> `docs/superpowers/specs/2026-09-25-agentj-right-panel-run-details-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-25-agentj-right-panel-run-details-plan.md`
> （T0–T11 全部落地，T7/T8 由前端侧落地，本文件仅前端）。
> 前端范围：`agentj-ui/packages/sdk`（parseProgress 容忍 + 0.23.0/0.15.0）+ `agentj-ui/packages/protocol`（types.d.ts 含 FileChangeSource）+ `agentj-ui/apps/web`（todoState/changes 归约 + RightPanel/ChangeList/TodoPanel/App/RunPanel/FileTree/统一侧栏）+ `agentj-ui/apps/tui`（todo.ts 兼容）；工作区 0.22.0 → **0.23.0**。后端部分见 `summary_and_next_se.md`。

## v0.23.0（前端部分）✅（本次完成）

- ✅ **T2 SDK 容忍**：`progress.ts` 扩展 `FileChangeFrame {run_id, seq, source, total_bytes}` + `TodoFrame {runId}` + `ProgressFrame file_change {runId}`，`parseFileChange` 读取 `run_id`（UUID 校验 + ≤128 + 正则过滤，非法丢弃不抛错）、`seq`/`source`/`total_bytes` 可选透传；`parseProgress` 对 `todo` 读取外层 `run_id`（非法→`undefined`）、对 `file_change` 合并內外层 `run_id`（`change.run_id` 优先）、非法外层丢弃；`config.ts` `VERSION 0.23.0 / PROTOCOL_VERSION 0.15.0`；`config.test.ts` 与 `init-default.test.ts` 同步。
- ✅ **T6 归约升级**：`todoState.ts` 按 `(run_id, revision)` 丢旧帧（`byRun: Map<string|null, ...>`，`runId`/`currentRunId` 跟踪，非法 run_id→`null` 未分组兼容，`applyTodoFrame` 同引用丢弃 stale，`todosFromHistory(events, runIdFilter?)` 按 `run_id` 过滤后取 `(runId, max revision)`）；`changes.ts` 扩展 `FileChangeRow {runId, seq, source, totalBytes, diffTruncated, eventSeq}`，`toRow` 解析 `run_id/seq/source/total_bytes`，`keyOf` 按 `(runId,path)`（legacy=`__legacy__`）、`upsert` 按 key、`sortBySeq`（seq→eventSeq→稳定序）、`applyFileChange` upsert 汇总 + 历史按 seq 排序、`changesFromHistory` 按 `(run_id,path)` 归并；两套归约均零破坏（web 51 文件 552 tests 全绿）。
- ✅ **T7 右栏分组与徽标**：`RightPanel.tsx` 新增 `RunGroup {runId, todos, changes}` + 导出 `groupByRunId`/`sortGroups`/`runLabel`（当前置顶、short 8-char、未分组（历史））、`RightPanelProps` 兼容 `groups`/`changes`/`todos`/`currentRunId` 双形态（children 透传保留，groups 优先走分组渲染，每组独立 header + Empty 占位 `暂无任务/暂无变更`，resizer 逻辑保留）；`ChangeList.tsx` 新增 `SourceBadge`（write→accent `write`，bash→muted `bash`，未知 legacy 无 badge + 计数文案）、`DiffPanel` 在 `diffTruncated && totalBytes` 时渲染 `已截断（原始 N KiB）` hint（Math.ceil(totalBytes/1024)）、`HistoryDrawer` 按 `seq`/`eventSeq` 排序（`sortBySeq`）、行不可展开（bash/无 diff 禁用 + tooltip `bash 变更无 diff，仅计数`）、`keyOf` 按 `(runId,path)` 去重、scrollIntoView 高亮 `data-path`。
- ✅ **T8 应用层联动**：`App.tsx` 新增 `activeRunId/ref + highlightedPath` 状态，`session/run` 结果 `run_id` 捕获（`setActiveRunId`）、`$/progress` `todo/file_change` 外层 `runId` 捕获并路由到对应 run 桶（`applyTodoFrame/applyFileChange` 已 per-run）、`session/history` 请求携带 `run_id: activeRunId ?? undefined`（缺省未分组回退），响应回放 `todosFromHistory(all, runFilter)`/`changesFromHistory(all, runFilter)`；`TodoPanel` 按 `byRun` 分组渲染（`activeRunId` 高亮 + `onSelectRun` 切换并回填 `todos` 前一条快照）、空态 `暂无任务`；`RightPanel` 接收 `currentRunId/groups/changes/todos`，`ChangeList` 接收 `highlightedPath/activeRunId/onSelectRun` 并对 `highlightedPath` 行高亮；`FileTree` `onFileSelect` 透传 `highlightedPath`，`UnifiedSidebar` Thread 回调同步 `activeSessionId`；`RunPanel.tsx` 补 `onRunId` 回调透传 `activeRunId`。
- ✅ **T9 TUI 兼容**：新建 `apps/tui/src/todo.ts`（纯函数 `applyTodoFrame`，per-run when `runId` else legacy；`EMPTY_TODO_STATE {revision:-1}` + UUID 校验），`runview.tsx` 委托 `todo.ts`（`TodoState = TodoStateFromHelper`，`EMPTY_TODO_STATE` 双别名兼容 `EMPTY_TODOS`），`rowsFromFrame` 新增 `todo`/`file_change` no-op 分支（保留只读 todo 区块，不新增 file_change 视图）。
- ✅ **T10 文档与生成**：`npm generate @agentj/protocol` 产出 `types.d.ts`（`FileChangeSource` + 5 处加性字段）、`npm generate @agentj/sdk` 方法注册表 21 方法 0.15.0；`docs/specs/ipc-protocol.md` 同步 0.15.0（§7.3/8.7/8.19/9.1/10/11.3 + 52 schemas）。

## 验证结果（前端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 47 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **552 passed（51 files）** |
| `bun --cwd agentj-ui/apps/tui test` | **142 passed（19 files）** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> **验收状态：待验收（`jskill-test-acceptance` 未跑）**——前端验收项：RightPanel 按 run 分组（当前置顶、未分组历史）、bash 徽标不可展开、write 截断 hint、历史抽屉按 seq、FileTree→ChangeList 高亮跳转、非法 run_id 降级未分组、旧数据兼容；后端门禁与验收项见 `summary_and_next_se.md`。

## 遗留问题 / 已知限制（前端）

- **run 分组仅运行期**：`byRun` 与 `activeRunId` 为内存态（切换会话重置），无 localStorage 持久化；`__legacy__` 桶与当前 run 桶并存，跨 run 不迁移。
- **抽屉与分组为派生**：`sortBySeq` 对无 `seq` 的旧数据回退 `eventSeq`（TrajectoryEvent.seq），无 `eventSeq` 时保持稳定序；无分页懒加载。
- **TUI 侧未发 project_root**：展开态键仍为内核 cwd 回退（与后端遗留一致）；rank 三处镜像问题待后端 ④。
- **Web 无搜索态**：设计非目标 ③，故 `sidebarHint` 搜索态文案组件存在但不可达（与 v0.22.0 遗留一致）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- **v0.23.0 开发分支 `feature/right-panel-run-details` 全量落地，待 `jskill-test-acceptance` 验收后合并 main**；当前功能完结，下一特性从「待实施」表选择（`jskill-feature-next` 合并本表 + `_se.md` 表）。
- 前端候选即上表 ③（后端候选见 `summary_and_next_se.md`）。
