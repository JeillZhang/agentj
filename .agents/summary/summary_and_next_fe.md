# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**对话工具调用卡片可读性**（工具名旁显示命令 / 文件 / 模式 + 卡片铺满中栏）
> 及其**验收修复**（BUG-017 空助手气泡 / BUG-018 会话列表不刷新）。均已交付
> （Web **679 passed** / Rust **911 passed**）。版本按约定保持 **0.1.0** 不递增；
> `PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas（**无协议变更**；BUG-017 触及
> `agentj-ipc` 的 thinking 落盘，但未改 wire）。

## 工具调用卡片可读性（工具名旁显示调用目标 + 铺满中栏）✅

### 需求

用户（m00002）：对话时模型执行 bash 的「结果卡片」，顶部 `bash` 命令旁把命令显示出来；
读取文件等也要显示读了什么文件；卡片宽度调整为中间栏宽度。

### 变更

- `agentj-ui/apps/web/src/RunPanel.tsx`：新增导出 `toolSummary(entry)` —— 按 `entry.name`
  从 `entry.args` 取一行目标：`bash`→`command`、`read` / `write`→`path`、`glob`→`pattern`、
  `grep`→`pattern`（`include` 存在且非 `*` 时拼 `"pattern · include"`）、`todo`→`"N 项"`；
  `args` 为 null / 非对象或工具未知时返回 `""`。`ToolCard` 在 `.aj-tool-name` 之后渲染
  `<span className="aj-tool-summary" title={summary}>`，仅当 summary 非空。
- `agentj-ui/apps/web/src/components/conversation.css`：`.aj-tool-card` 的
  `align-self: flex-start` → `align-self: stretch`（铺满 `.aj-conv` 中栏宽度；评审后移除了多余的
  `width: 100%`）；
  新增 `.aj-tool-summary`（`flex: 1 1 0` / `min-width: 0` / `white-space: nowrap` /
  `text-overflow: ellipsis` / `font-family: var(--aj-mono)` / `color: var(--aj-textMuted)`）。
- `agentj-ui/apps/web/src/components/displayPrefs.css`：`.aj-tool-summary` 字号跟随
  `var(--aj-font-code, 12px)`，与 `.aj-tool-output` 一致。

### 测试（RED → GREEN）

| 测试 | 文件 |
|---|---|
| `shows the bash command beside the tool name` | `agentj-ui/apps/web/src/RunPanel.test.tsx` |
| `shows the target path for read and write tools` | 同上 |
| `summarizes grep as pattern + include and glob as pattern` | 同上 |
| `omits the summary when args are absent or the tool is unknown` | 同上 |
| `stretches the tool card across the conversation column` | `agentj-ui/apps/web/src/components/toolCardCss.test.ts` |
| `truncates a long command summary instead of wrapping` | 同上 |

RED 证据：实现前 6 条断言全部失败（RunPanel 4 条 + CSS 2 条），实现后全绿。

### 验证结果（特性阶段）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **666 passed**（54 files；660 → +6） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（仅既有 chunk > 500 kB 提示） |
| `cargo fmt --all -- --check` | clean |
| `cargo test --workspace` | 全绿（本阶段未改后端） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `./scripts/check-version.sh` | **version-ok（0.1.0 / 0.18.0）** |

## 验收修复（BUG-017 / BUG-018）✅

用户（m00229）在同一会话报告：①「bash 命令卡片中间有不少空白消息没有显示，最左侧有蓝条，
但无文字消息」；②「新建会话后，左侧列表不更新新对话项」。按 `jskill-test-acceptance` 流程，
修复直接落在**同一特性分支** `feature/tool-card-command-summary`（PR #84），**每 Bug 一个提交**。

### BUG-017 — 零宽保活增量渲染成空助手气泡（commit `a2947f6`）

- **取证**：真实轨迹 `~/.local/share/agentj/sessions/4451942d-…​.trajectory`（170 事件）中
  12 个 `thinking` 有 **10 个 content 恰为 `"\u200b"`**（U+200B 零宽空格 = 网关在工具调用之间的
  保活增量）。U+200B **无 Unicode `White_Space` 属性**：`String.trim()` / `char::is_whitespace()`
  都不判空 → 每帧成为一个 `.aj-ai` 条目，即用户看到的「蓝条 + 无文字」。
- **修复**：新增 `agentj-ui/apps/web/src/blankText.ts`（`BLANK_CHARS = /[\s\u200b-\u200f\u2060\ufeff]/g`
  + `isBlankText()`；**不能用 `trim()`**）——`ConversationFeed` 的 `case "ai"` 直接 `return null`；
  `Trajectory` 的 `historyToEntries` / `historyToConversation` 两处 `case "thinking"` 同步跳过；
  后端 `crates/agentj-ipc/src/lib.rs` 的 `flush_thinking` 改为**总是 `mem::take`** 再由新关联函数
  `is_visually_blank` 判定「仅空白 / 零宽 / BOM」即不落盘（避免残留污染下一段 thinking）。

### BUG-018 — 新建会话后左侧列表不刷新（commit `cf2b45e`）

- **取证**：`UnifiedSidebar` 只在挂载时拉取一次 `session/list`；`handleNewSession` 成功后只
  `assignSession` + `onActiveChange`，turn 结算（preview / `updated_at` 变化）也不刷新。
- **修复**：抽出 `refreshList` 回调（挂载 / `session/new` 成功后 / 事件到达时调用，用 `aliveRef`
  替代一次性 `cancelled` 标志）；新增 `agentj-ui/apps/web/src/sessionEvents.ts`
  （`agentj:sessions-updated` + `notifySessionsUpdated` / `subscribeSessionsUpdated`）；
  `RunPanel.submit()` 的 `finally` 广播该事件 → 一轮跑完（成功或失败）左栏即刷新 preview 与时间。
  **刻意不做乐观插入**：refetch 返回的列表会冲掉本地乐观行。

### 测试（RED → GREEN）

| 测试 | 文件 |
|---|---|
| `isBlankText` 4 条（空 / 零宽 / 含可见字形 / 反 trim 守卫） | 新增 `agentj-ui/apps/web/src/blankText.test.ts` |
| `renders no assistant bubble for zero-width-only text`、`still renders an assistant bubble for zero-width-prefixed real text` | `agentj-ui/apps/web/src/RunPanel.test.tsx` |
| `visually blank thinking is dropped (BUG-017)` 3 条 | `agentj-ui/apps/web/src/Trajectory.test.tsx` |
| `dispatches agentj:sessions-updated after a turn settles`、`announces even when the turn fails` | `agentj-ui/apps/web/src/RunPanel.test.tsx` |
| `BUG-018 — a new session shows up in the list right away`、`BUG-018 — a sessions-updated event refetches the list` | `agentj-ui/apps/web/src/components/UnifiedSidebar.test.tsx` |
| `run_drops_zero_width_only_deltas_instead_of_recording_blank_thinking`、`run_keeps_thinking_that_contains_visible_glyphs`、`is_visually_blank_covers_zero_width_and_whitespace_only_text` | `crates/agentj-ipc/src/lib.rs` |

RED 证据：Web **7 failed / 662 passed**（+ `blankText.test.ts` 整文件 import 失败）；GREEN 后 **679 passed**。

### 验证结果（验收修复后 · HEAD `cf2b45e`）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **679 passed**（55 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（仅既有 chunk > 500 kB 提示） |
| `cargo test --workspace` | **911 passed / 0 failed**（`-p agentj-ipc --lib` 197） |
| `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` | FMT-OK / 0 warning |
| `./scripts/check-version.sh` | **version-ok（0.1.0 / 0.18.0）** |

## 遗留问题 / 已知限制（前端）

- **`packages/sdk` 版本断言的守护范围**：`config.test.ts` 现断言「`VERSION` == 仓库根
  `Cargo.toml` 版本」，可防 bump 遗漏；但 desktop 三清单 / `agentj-ui/package-lock.json`
  仍需 `scripts/check-version.sh` 覆盖（后者当前**未被覆盖**，其根条目仍记着陈旧的 `0.29.1`，
  因 `agentj-ui/package.json` 无 `version` 字段，npm 侧无实际影响）。
- **`sessionBusyMessage` 用文本匹配**：SDK 抛错只保留 `message` 文本（未附 `code`），故用
  `includes("-32805")`；若后续给 SDK `Error` 加 `code` 字段，应改为结构化判断。
- **无 `session/unsubscribe`**：会话切换后旧会话的订阅仍在（服务端按连接归属，重连即随连接
  释放）；当前不做前端侧退订。
- **Web 连接为手动重连**：「设置 · 连接」点 `Connect` 才重建 transport/client（`connectSeq`），
  没有自动重连/退避；只保证「重连后重新订阅」。
- **标签只能显示「我们请求的」模型**：内核 `session/run` 不回显生效模型（见 `_se.md` 待实施 ⑧），
  上游把请求静默改路由时 UI 无从得知；当前把标签与 request 参数绑在同一处取值，只能保证
  「显示 == 我们发出的」。
- **工具摘要只覆盖已知工具**：`toolSummary` 对未识别的 `name` 返回空串（不显示摘要），
  MCP / 插件工具若不带已知字段则不显示目标；如需通用回退（如 `path`/`pattern`/`command`
  任意键）可后续再议。
- **`cmd` / `command` 字段名不一致（既有）**：`toolSummary` 以规范键 `command` 取值
  （`crates/agentj-tools/src/builtins.rs` 的 JSON Schema 与内核文本工具调用还原均用 `command`）；
  但 `crates/agentj-protocol/src/permission.rs` / `schemas/PermissionRequestParams.json` 的文档与
  Web `ApprovalDialog.tsx:22` 标签仍写 `cmd`。若将来把权限原始 args 直接喂给工具卡片，摘要会静默为空；
  属独立遗留项，与本阶段无关。
- **TUI 仍会为视觉空白增量留一个空行（BUG-017 的同源盲区，本轮未修）**：内核仍照常广播
  `$/progress {kind:"text"}`（有意为之：保持 live 与落盘一致，且前端需覆盖旧轨迹回放），
  而 `agentj-ui/apps/tui/src/runview.tsx:126` 的 `case "text"` 无条件 `next.push({key, kind:"text", text: frame.text})`
  ——零宽帧会多出一个**空白文本行**（TUI 中几乎不可见，与 Web 的「蓝条空气泡」不同，故未计入本轮验收缺陷）。
  若要收敛，可二选一：①把 `isBlankText` 上提到 `@agentj/sdk`（三端共享，需同时改 Web import 与 SDK 测试）；
  ②在内核 `$/progress` 广播侧就丢弃视觉空白 delta（单点修复，但 live 与落盘由「都保留」变为「都丢弃」）。
- **防空白只落在「落盘 + Web 渲染层」**：本轮选择「后端 `flush_thinking` 只管落盘 + 前端渲染层过滤」，
  因此 live 帧仍带零宽字符；若将来新增第三端，需同样做渲染层过滤（或采用上面的方案 ②）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于服务端搜索） | 待实施 |
| ㉑ | 【债】前端 CI 门禁 | `.github/workflows/` 三个 workflow（`ci.yml` / `smoke.yml` / `e2e.yml`）**全部只跑 Rust、只跑 ubuntu-latest**（实测 `grep -rn "npm\|bun" .github/` 为空）→ 679 web 测试 / 55 文件、21 个 TUI 测试文件、`tsc -b`、`oxlint`、`check-desktop-ui.mjs` **均无门禁**，前端 30,722 行（占总代码 38%）靠本地自觉。新增 frontend job：`npm ci` → protocol `generate` → `tsc --noEmit` → web vitest → `bun --cwd agentj-ui/apps/tui test` → desktop UI 校验；并在 CI 中重建 `dist/version.json`（现产物需人工重建，否则触发 UI/内核版本不一致告警） | 无 | 待实施 |
| ㉒ | 【债】跨平台 CI 矩阵 | desktop 声称产 deb / AppImage / **dmg / msi**（`docs/guide/desktop.md`），但 CI 无 macOS / Windows 矩阵，跨平台从未被验证（Rust 侧亦只在 Linux 编译） | 依赖 ㉑ 的 workflow 结构 | 待实施 |
| ㉓ | 【修复】`isBlankText` 上提 SDK + TUI 盲区 | BUG-017 的视觉空白过滤现散落两处（内核 `flush_thinking` + Web `blankText.ts`），而 `agentj-ui/apps/tui/src/runview.tsx:126` 的 `case "text"` 仍无条件 `next.push(...)` → 零宽帧多一个空行（同源盲区未修）。把 `isBlankText` 上提 `@agentj/sdk`，三端共享（需同改 Web import 与 SDK 测试） | 无 | 待实施 |
| ㉔ | 【债】三端共享逻辑收敛 | Web 19,930 行 / TUI 7,939 行各自重写会话、运行视图、文件树、搜索（`toolSummary`、files 懒加载、空文本过滤等均为双份实现）；评估把无 UI 依赖的纯函数上提 `@agentj/sdk`，避免同一缺陷修两次 | 与 ㉓ 同向 | 待实施 |
| ㉕ | 【修复】版本元数据与依赖残留 | ① `scripts/check-version.sh` 未覆盖 `agentj-ui/package-lock.json`（根条目仍记陈旧的 `0.29.1`）与 `agentj-ui/apps/web/package.json`（`"version": "0.0.0"`）；② 清理 `agentj-ui/node_modules/.old_modules-245c1e1a995d5f43/` 等迁移残留 | 无 | 待实施 |
| ㉝ | 【债】包管理器与 lockfile 三份并存 | 实测 `agentj-ui/bun.lock` 117,604 B、`agentj-ui/package-lock.json` 184,696 B、`agentj-ui/apps/web/package-lock.json` 58,110 B **三份并存**，且所有 `package.json` **无 `packageManager` 字段** → npm 与 bun 解析结果可能漂移（`AGENTS.md` 现同时要求 npm 与 bun 命令，需一并收敛） | 无 | 待实施 |
| ㉞ | 【债】工作区与工具链卫生 | 仓库根残留空 `node_modules/`（0 项）与 `.playwright-mcp/`（**107 个文件**，已 gitignore）；AI 工具目录六个并存（`.agents` / `.claude` / `.omo` / `.opencode` / `.superpowers`）→ 清理 + 收敛，减少工具链碎片化 | 无 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本阶段改动已提交到 `feature/tool-card-command-summary`（特性 `c62ee6c` + 文档 `2190a0d`，
  验收修复 `a2947f6` / `cf2b45e`，均**每 Bug 一个提交**）→ 推送**两个远端**（本机
  `github` = GitHub 权威、`origin` = GitCode 镜像；与 AGENTS.md 的远端命名相反）→
  更新 PR #84 的 title / body 以覆盖验收修复（用户负责合并）。
- 本轮验收记录见本地 `.superpowers/acceptance/bugs.md`（gitignored；上轮已归档为
  `archive-bugs-20261018-103252.md`）。
- 前端候选：③ Web 接入服务端搜索，以及 ㉑–㉕、㉝–㉞ 工程债 / 修复（前端 CI、跨平台矩阵、SDK 共享、
  版本覆盖、lockfile 收敛、工作区卫生）；或订阅状态常驻指示（需先定义新鲜度语义）；
  或清理 `TopbarMenu` / `SettingsDialog` 测试里的重复 stub helper；
  或 TUI `/models` 与 Web 选择语义对齐（TUI 仍是内存态、不跨进程持久化）。
- 后端候选见 `summary_and_next_se.md`。
