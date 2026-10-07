# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.26.0 运行期切模型——前端部分**
> ——来源：设计 `docs/superpowers/specs/2026-10-07-agentj-runtime-model-switching-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-07-agentj-runtime-model-switching-plan.md`（T10–T14/T16/T18/T19）。
> 前端范围：`agentj-ui/apps/web`（`ComposerOptions` / `App` / `RunPanel`）+
> `agentj-ui/apps/tui`（models 选择器 / `app.tsx`）+ `agentj-ui/packages/sdk`（版本）+
> `agentj-ui/apps/desktop`（版本清单）；工作区 0.25.2 → **0.26.0**。
> 后端部分见 `summary_and_next_se.md`。

## v0.26.0（运行期切模型，前端部分）✅（本次完成）

- ✅ **Web（`apps/web`）**：
  - `ComposerOptions` 改为**受控**：新增 `selected` / `onSelect`，`ModelEntry` 导出并带可选
    `provider` 标签；修掉「`model ?? prefs.model` 标签覆盖」缺陷——chip 显示所选模型后
    不再被内核默认值盖回。
  - `App` 持有 `selectedModel`，把 `kernel/status.providers[].models` 展平时给每条打上
    **它自己 provider 的 id**（不看列表位置，`providers[0]` 不等于默认 provider）。
  - `RunPanel.submit` 携带 `model`，且**仅当该条目带 provider 标签时**才携带 `provider`；
    未挑选时参数**逐字**为 `{session_id, content}`（零回归）。
  - 同名模型跨 provider 用 **provider 限定 key** 去重（`modelKey`），标签/选中判定不串味。
- ✅ **TUI（`apps/tui`）**：`ModelSelector` 变为可选中（`j` / `k` / 方向键移动、`Enter` 确认、
  `Esc` 关闭），条目带 provider 标签；`app.tsx` 发送所选 `model`（带标签时附带 `provider`）
  并在确认后关闭选择器；移除「重启内核生效」旧文案。
- ✅ **版本同步**：`@agentj/sdk` `VERSION` 0.25.2 → 0.26.0；desktop 三清单
  （`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml`）与 `scripts/check-version.sh`
  同步 0.26.0（`PROTOCOL_VERSION` **0.16.0**）；`packages/protocol` 类型由 schemas 重生成。
- ✅ **文档（前端相关）**：新增 `docs/guide/model-switching.md`（Web/TUI 选择入口、生效范围、
  本地保存位置）+ 指南索引；订正 `docs/guide/subscription.md` / `docs/guide/tui.md` 陈旧表述。

## 验证结果（全量门禁清扫，2026-10-07）

> 本表由编排器在本阶段最终树上执行；**全量清扫覆盖前后端两侧并全部通过**，
> 前端门禁如下（本文件只列前端门禁），后端门禁见 `summary_and_next_se.md`。

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 626 passed（52 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 54 passed（6 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `bun --cwd agentj-ui/apps/tui test` | 157 passed |
| `./scripts/check-version.sh` | version-ok（**0.26.0** / `PROTOCOL_VERSION` **0.16.0**） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 遗留问题 / 已知限制（前端）

- **Web `selectedModel` 不从 `localStorage` 回灌（已知、符合零回归）**：选择持久化到
  `agentj.composer.model`，但页面加载时**不**从中恢复 `selectedModel`，
  因此刷新后版本前一次挑选不生效——发送的下一轮走内核默认，直到用户在弹层里重新确认。
  这是有意的零回归行为（保证「没挑过」与旧客户端逐字一致）；`docs/guide/model-switching.md`
  已如实描述该可观察行为。二期可做「加载即回灌本地选择」。
- **预存在的 TUI 工具链问题（非本阶段回归）**：TUI `tsconfig` 的 `types: ["bun-types"]`
  与 oxlint 的 native-binding 在本环境无法直接跑，故 TUI 门禁以
  `bun --cwd agentj-ui/apps/tui test` 为准（157 passed），未纳入 `tsc` / oxlint。
- **TUI 选择仅存内存**：退出 TUI 即丢失，下次启动回到内核默认（设计如此，非缺陷）。
- **`ProjectEditDialog` 仍用静态 `MODEL_LIST`**：项目默认模型下拉只显示 `deepseek-chat`，
  未消费服务端目录（设计「新增/修改文件」未列入，属后续增强）。
- **服务端目录的陈旧保留**：`App.tsx` 仅在 `served.length > 0` 时更新；重连后若
  `kernel/status` 省略 `providers` 会保留上一次列表而非回退单条（既有行为，非本阶段 AC 违背）。
- **lockfile 版本元数据已同步（本阶段修复）**：`agentj-ui/package-lock.json` /
  `bun.lock` / `src-tauri/Cargo.lock` 的 desktop 版本此前分别停留在 0.25.2 / 0.24.2 /
  0.24.1（均无门禁读取），本阶段已统一刷到 **0.26.0**，遗留项关闭。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- v0.26.0 运行期切模型（前端部分）完成：Web 626 / SDK 54 / TUI 157 全绿，
  Web + protocol 构建 clean，check-version / desktop-ui 通过；待 PR 由用户合并 main。
- 前端候选：③ Web 接入服务端搜索；或在弹层重新打开时从 `agentj.composer.model` 回灌选择；
  或让 `ProjectEditDialog` 消费服务端模型目录。
- 后端候选（opencode-go 二期 / 待实施 ①②④）见 `summary_and_next_se.md`。
