# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.25.0 opencode-go 订阅接入（一期）——前端部分**
> ——来源：设计 `docs/superpowers/specs/2026-10-04-agentj-opencode-go-subscription-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-04-agentj-opencode-go-subscription-plan.md`（T7/T8/T14/T20/T21）。
> 前端范围：`agentj-ui/apps/web`（ComposerOptions 模型列表）+ `agentj-ui/apps/tui`（models 视图）
> + `agentj-ui/packages/sdk`（版本）+ `agentj-ui/apps/desktop`（版本）；工作区 0.24.2 → **0.25.0**。
> 后端部分见 `summary_and_next_se.md`。

## v0.25.0（前端部分）✅（本次完成）

- ✅ **Web composer 消费服务端模型列表**：`ComposerOptions` 新增可选 `models` prop，四个使用点（选项计数、初始选中索引、`selectOption` 查找、弹层渲染）统一走 `modelOptions = models?.length ? models : MODEL_LIST`，无服务端数据时回退内置 `deepseek-chat`；`ModelEntry.cap` 改为可选；初始高亮索引 clamp 到 0（当前模型不在列表时不静默 no-op）。
- ✅ **Web 装配**：`App.tsx` 从 `kernel/status.providers[].models` 展平出模型列表 → `RunPanel` → `ComposerOptions`；`kernel/status` 无 `providers` 时保持原单条回退。
- ✅ **TUI 模型视图**：`providersFromStatus` 由「映射 provider id」改为「展平 `providers[].models`」，无 provider 上报模型时回退 `MODEL_CONFIG.providers`；`renderModelList` 保持 `▶` 默认高亮；既有 `app.test.tsx` 断言同步到新语义。
- ✅ **版本同步**：`@agentj/sdk` `VERSION` 0.24.2 → 0.25.0；desktop 三清单（`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml`）与 `scripts/check-version.sh` 同步 0.25.0。

## 门禁（2026-10-06）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **617 passed（52 files）**（611 → +4 T7 +1 T14 +1 T7 边界） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | **54 passed（6 files）**（版本串 0.24.2 → 0.25.0） |
| `bun --cwd agentj-ui/apps/tui test` | **150 passed（20 files）**（+4 models.test.tsx） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（`dist/version.json` product_version **0.25.0** / protocol 0.15.0） |
| `./scripts/check-version.sh` | version-ok（0.25.0；PROTOCOL_VERSION 0.15.0） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 遗留问题 / 已知限制（前端）

- **`ProjectEditDialog` 仍用静态 `MODEL_LIST`**：项目默认模型下拉只显示 `deepseek-chat`，未消费服务端列表（设计「新增/修改文件」未列入，属后续增强）。
- **服务端列表的陈旧保留**：`App.tsx` 仅在 `served.length > 0` 时更新，重连后若 `kernel/status` 省略 `providers` 会保留上一次列表而非回退单条（计划 `impl_code` 即如此，非 AC 违背）。
- **TUI 依赖需先安装**：`bun --cwd agentj-ui/apps/tui install` 已执行（本次新增 `@opentui/*` 解析，`bun.lock` 更新）；CI/新环境需先 install 才能跑 TUI 门禁。
- **lockfile 版本元数据漂移**：`agentj-ui/package-lock.json` / `bun.lock` / `src-tauri/Cargo.lock` 中的 desktop 版本仍为旧值，无门禁读取，下次 install 刷新。
- **`cap` 为可选**：`ModelEntry.cap` 现为可选，服务端条目可只带 `name`。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- opencode-go 订阅一期前端部分完成（web 617 / tui 150 / sdk 54）；待 `jskill-test-acceptance` 验收。
- 下一特性候选：③ Web 接入服务端搜索；后端候选（opencode-go 二期 / 待实施 ①②④）见 `summary_and_next_se.md`。
