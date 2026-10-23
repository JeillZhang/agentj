# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**run 可靠性批次（前端）** —— 分支 `feature/run-reliability-batch`。Web 新增生效
> 模型 chip（`data-testid="run-model-chip"`，内核缺省时回退选择器），`@agentj/protocol` 类型
> 重新生成、`@agentj/sdk` `PROTOCOL_VERSION` 同步 **0.19.0**（`VERSION` 仍 0.1.0）。下方
> search-rank 单一真源化（TUI 侧）等前序章节为归档记录。

## run 可靠性批次（前端）✅

分支 `feature/run-reliability-batch`。前端只做生效模型回显 + 协议/版本同步，无独立逻辑重写。
对应 `_se.md` 待实施 ⑧（`session/run` 回显生效模型）的前端消费落地。

### 变更

- **Web 生效模型 chip**（`agentj-ui/apps/web/src/RunPanel.tsx`）：新增
  `data-testid="run-model-chip"` 的 chip，取值
  `chipModel = effectiveModel?.model ?? selected?.name ?? model`（`RunPanel.tsx:840`）——内核 run
  回复带上 `model` 时显示**生效**模型，缺省时回退模型选择器。
- **`@agentj/protocol` 类型重新生成**：`schemas/*.json`（含 `SessionRunResult` 新增可选
  `provider` / `model`）→ `types.d.ts` 再生成，`tsc --noEmit` 通过。
- **`@agentj/sdk` 协议版本同步**：`config.ts` `PROTOCOL_VERSION` → `0.19.0`（`VERSION` 仍 `0.1.0`）。

### 验证结果

| 门禁 | 结果 |
|------|------|
| `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` | 通过（types.d.ts 与 schemas 无漂移） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | `tsc --noEmit` 通过 |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 通过（RunPanel 生效模型 chip 相关断言） |
| `./scripts/check-version.sh` | **保持红**（0.19.0 协议基线待 T19 同步 SDK / desktop；本任务不修） |

## 已完成（前端：工具卡片可读性 + BUG-017 / BUG-018，详见 `docs/changelog/v0.1.md`）

- ✅ 工具调用卡片可读性：`toolSummary(entry)`（`bash`→`command`、`read`/`write`→`path`、
  `glob`/`grep`→`pattern`、`todo`→「N 项」）+ `.aj-tool-card` 铺满中栏 + `.aj-tool-summary` 截断；
  Web 666 passed。
- ✅ 验收修复 BUG-017 / BUG-018（每 Bug 一提交 `a2947f6` / `cf2b45e`）：视觉空白（U+200B 等）
  不再渲染成空助手气泡（内核 `flush_thinking` 改「总是 take + `is_visually_blank` 判定」+ 前端
  `blankText.ts`）；`session/new` 后左栏即时刷新（`agentj:sessions-updated` 事件 + refetch，不做
  乐观插入）。Web **679 passed**。

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
- **标签回退仍是「我们请求的」模型**：内核 `session/run` 现可回显生效模型（`_se.md` 待实施 ⑧ 已
  落地），Web run-model-chip 优先显示 `effectiveModel?.model`；仅当内核省略 `model` 时才回退
  选择器取值（`selected?.name ?? model`），此时仍只能保证「显示 == 我们发出的」。
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

- run 可靠性批次（前端）已提交到 `feature/run-reliability-batch`（提交见 `_se.md` 真实提交表）→
  推送**两个远端**（本机 `github` = GitHub 权威、`origin` = GitCode 镜像；与 AGENTS.md 的远端命名
  相反）→ 开 GitHub PR（用户负责合并）→ 进入测试验收。
- 前端候选：③ Web 接入服务端搜索，以及 ㉑–㉕、㉝–㉞ 工程债 / 修复（前端 CI、跨平台矩阵、SDK 共享、
  版本覆盖、lockfile 收敛、工作区卫生）；或订阅状态常驻指示（需先定义新鲜度语义）；
  或清理 `TopbarMenu` / `SettingsDialog` 测试里的重复 stub helper；
  或 TUI `/models` 与 Web 选择语义对齐（TUI 仍是内存态、不跨进程持久化）。
- 后端候选见 `summary_and_next_se.md`。
