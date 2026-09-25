# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.22.0 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口
> （四项，三处加性 wire）——前端部分**
> ——来源：用户请求（2026-09-24，四项）；设计文档
> `docs/superpowers/specs/2026-09-24-agentj-metrics-manifest-search-and-keys-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-24-agentj-metrics-manifest-search-and-keys-plan.md`
> （T0–T31 全部落地）。
> 前端范围：`agentj-ui/apps/web`（构建期清单 `public/` 覆盖 + 共享 `useRovingFocus` 收口 +
> 侧栏快捷键提示行）+ `agentj-ui/apps/tui`（服务端搜索 + `project_root` 展开态键）；
> 工作区 0.21.1 → **0.22.0**。后端部分见 `summary_and_next_se.md`。

## v0.22.0（前端部分）✅（本次完成）

- ✅ **② 资产清单覆盖扩展（构建期）**：`apps/web/build/version-sri*` 生成的
  `dist/asset-manifest.json` 覆盖 dist 实目录全部部署文件（含 `public/` 透传，bundle 键优先，
  排除 `version.json`/自身/隐藏文件）；单遍哈希与 SRI `integrity` 逐字节一致。
- ✅ **③ TUI 服务端搜索**：搜索改防抖 150ms 服务端查询（`file/list
  {path:null, depth:8, query}`，服务端 rank + `total`；旧内核无 `total` → 自动降级 depth-8
  本地过滤）；展开态键改为会话有效 `project_root`（`session/new`/`session/status` 捕获，
  未知回退 `__default__`）。
- ✅ **④ Web 键位收口**：共享 `useRovingFocus`（`wrap`/Home-End）扩展，TopbarMenu /
  SessionListMenu / SidebarTabs / SettingsDialog 四处迁移（ComposerOptions listbox 保留
  独立模式）+ 侧栏底部快捷键提示行（单一常量源、`aria-hidden`；三态文案组件，Web 当前
  两态可达，搜索态待 ③）。
- ✅ **Phase 5 全分支复核（Oracle）+ 修复波（前端侧）**：恢复 `SessionListMenu` 过滤/排序
  飞出层的键盘进入路径（`cd24cb2`）；修正提示行宣传未绑定键位（从 Files 浏览态提示去除
  Web 不存在的 `s 搜索`，`784ed8b`）。

## 验证结果（前端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 47 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **552 passed（51 files）** |
| `bun --cwd agentj-ui/apps/tui test` | **142 passed（19 files）** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> **验收状态：PASS（2026-09-25，`jskill-test-acceptance`）**——0 个新 Bug；前端验收项：
> Playwright 22/22（菜单 roving / tablist / 提示行两态 / composer 零误触）。
> 后端门禁与验收项见 `summary_and_next_se.md`。

## 遗留问题 / 已知限制（前端）

- **② 浏览器端动态 import 仍无 SRI**（定位：部署完整性）。
- **③ TUI 搜索受预算/降级限制**：防抖引入 ≤150ms 输入延迟；旧内核降级为 depth≤8/≤2000
  索引（不完整）；TUI 仍不发送 `project_root`（键为内核 cwd 回退）；`r` 在 150ms 防抖窗口内
  按下时，该次搜索会话回退为本地索引模式。
- **③ rank 三处镜像（TUI / mock 侧）**：TUI `files.ts::searchRank` 与 mock kernel 镜像
  Rust 规则，跨语言漂移风险（消除 → 后端待实施 ④，见 `_se.md`）。
- **④ Web 展开态键仅 Web-safe**：既非 `__default__` 也不以 `/` 开头的键（如 Windows
  `C:\...`）在保存时被剪除，仅保留当前活动键。
- **④ 菜单 Tab 行为变更**：迁移后 Tab 不再逐项循环（ARIA 对齐，显式记录）；ComposerOptions
  保持独立模式；提示行不覆盖菜单/对话框键位（「帮助」项仍为 no-op）。Web 无搜索模式
  （设计非目标 ③），故 Files 搜索态提示文案组件支持但不可达。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- **v0.22.0 已合并 main（`553473f`，!39）**；当前功能完结，下一特性从「待实施」表选择
  （`jskill-feature-next` 合并本表 + `_se.md` 表）。
- 前端候选即上表 ③（后端候选见 `summary_and_next_se.md`）。
