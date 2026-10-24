# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**健壮性与去重批次（前端侧）**（视觉空白判定上提共享包，wire 面零变更）。

> 当前功能：**健壮性与去重批次（前端侧）** —— 分支 `chore/hardening-dedup-batch`（自 `b1edc78` 起）。
> 本轮前端只落地「`isBlankText` 上提共享包 + TUI 空白帧」一项：过滤实现从 Web 应用移到
> `@agentj/sdk`，TUI 运行视图同样丢弃零宽保活帧。
> `PROTOCOL_VERSION` 仍 0.19.0，`VERSION` 仍 0.1.0。

## 健壮性与去重批次（前端）✅

### 交付

- **`isBlankText` 上提 `@agentj/sdk`**：新增
  `agentj-ui/packages/sdk/src/blankText.ts`（`BLANK_CHARS` 正则 + `isBlankText`），从
  `index.ts` 导出；Web 的 `Trajectory.tsx` / `RunPanel.tsx` 改为从 `@agentj/sdk` 引入并删除
  `apps/web/src/blankText.ts`（测试随实现迁到 `packages/sdk/src/__tests__/blankText.test.ts`）。
  `App.test.tsx` 的 `vi.mock("@agentj/sdk")` 同步补 `BLANK_CHARS` / `isBlankText`
  （带 DRIFT WARNING 注释）。
- **TUI 空白帧不再建行**：`agentj-ui/apps/tui/src/runview.tsx` 的 `rowsFromFrame` 在
  `case "text"` 先判 `isBlankText(frame.text)`，零宽保活增量不再产生空行。

### 验证

| 门禁 | 结果 |
|------|------|
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 59 passed（7 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/sdk` | `tsc --noEmit` 通过 |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 678 passed（54 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | 通过 |
| `bun --cwd agentj-ui/apps/tui test` | 193 pass / 0 fail（新增空白帧用例） |
| `npm --prefix agentj-ui run lint --workspace=@agentj/web` | 仅既有 warning（49 条，无 error） |

### 验收修复

- `1ab524f` BUG-002：`App.test.tsx` 的 `vi.mock("@agentj/sdk")` 改为 `await vi.importActual(...)`
  展开真源、只覆盖需要 stub 的导出（`RpcClient` / `ResponseRouter` / `WebSocketTransport` /
  `resolveWsUrl` / `parseProgress` / `METHODS` / `PROTOCOL_VERSION` / `VERSION`），手抄的
  `BLANK_CHARS` / `isBlankText` 正则删除，并新增身份 + 行为断言（重新手抄即 RED）。

### 遗留问题（本批次）

- 整包 mock 的教训仍在：以后新增 Web 会调用的 SDK 导出不需要再补 mock（`...actual` 已覆盖），
  但需要 stub 的导出仍要显式覆盖；㉔ 可考虑「只 mock transport 层」。

## 归档：工程治理批次（前端）✅

分支 `chore/engineering-governance-batch`。对应 `_se.md` 同批次；本侧落地待实施 ㉑ ㉒ ㉕ ㉝ ㉞
（㉒ 仅到「跨平台可编译」，打包链路遗留为 `_se.md` ㉟）。

### 交付

- **npm 单一 lockfile**：删除 `agentj-ui/bun.lock`（117,604 B）与 `agentj-ui/apps/web/
  package-lock.json`（58,110 B）、`agentj-ui/packages/protocol/package-lock.json`，只保留
  `agentj-ui/package-lock.json`；`agentj-ui/package.json` 补 `name: agentj-ui`、
  `version: 0.1.0`、`private: true`、`packageManager: npm@11.11.0`、`engines.node: >=24`；
  `apps/web` 与 `apps/tui` 的 `version` 由 `0.0.0` 对齐到 `0.1.0`；`apps/desktop` 由 `0.29.1`
  对齐到 `0.1.0`（lockfile 根条目同步）。
- **registry 固定**：新增 `agentj-ui/.npmrc` → `registry=https://mirrors.huaweicloud.com/repository/npm/`
  （lockfile 的 integrity 与 registry 无关，可随时改回官方源；`npm ci` 实测 `added 337 packages in 9s`，
  lockfile 内零 `registry.npmjs.org` URL）。
- **前端 CI 门禁**（`.github/workflows/ci.yml` 新增 `frontend` 作业，ubuntu-latest、timeout 20）：
  17 步 —— `npm --prefix agentj-ui ci` → protocol `generate` + `git diff --exit-code --`
  `agentj-ui/packages/protocol/types.d.ts`（漂移门禁）→ protocol `build` → protocol test →
  sdk `generate` + `git diff --exit-code --` `agentj-ui/packages/sdk/src/methods.ts` → sdk test → tokens test →
  web test → web build → `dist/version.json` 与 `protocol/methods.json` 的 `protocol_version`
  一致性（node -e）→ `npm run lint --workspace=@agentj/web`（oxlint，warnings 只显示）→
  `bun --cwd agentj-ui/apps/tui test` → `node agentj-ui/apps/desktop/check-desktop-ui.mjs` →
  `./scripts/check-version.sh` → `node scripts/check-relocate.mjs`；`actions/setup-node@v4`
  （node 24 + `cache-dependency-path: agentj-ui/package-lock.json`）与 `oven-sh/setup-bun@v2`
  （bun 1.4.0）各一步。`packages/protocol/package.json` 补 `"test": "vitest run"`（4 个测试）。
- **跨平台构建**：新增 `cross-platform` 作业（matrix ubuntu / macos / windows +
  `cargo build --workspace --locked`），仅验证可编译，不含打包。
- **版本一致性**：`scripts/check-version.sh` 现覆盖 `agentj-ui/package.json`、`apps/web`、
  `apps/tui`、`apps/desktop` 与 lockfile 根条目（原 ㉕ ① 的缺口），失败时打印文件名与期望值。
- **卫生**：删除 `agentj-ui/node_modules/.old_modules-245c1e1a995d5f43/`、仓库根空
  `node_modules/`、`.playwright-mcp/`（107 文件）；`.gitignore` 补 `.opencode/`。
- **`check-relocate.mjs` 修复**：原先硬断言已改名的 `apps/web/src/jsonrpc.test.ts` → 必红；
  改为断言 `agentj-ui/apps/web/src` 下至少存在一个 `*.test.ts(x)`，并接入 CI。
- **远端命名**：`origin` = GitHub 唯一权威、`gitcode` = 只读镜像（与 `AGENTS.md` 一致）。

### 验证

| 命令 | 结果 |
|------|------|
| `npm --prefix agentj-ui ci` | `added 337 packages in 9s`（锁文件无 npmjs.org URL） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 55 passed（6 文件） |
| `npm --prefix agentj-ui test --workspace=@agentj/protocol` | 4 passed（1 文件） |
| `npm --prefix agentj-ui test --workspace=@agentj/tokens` | 通过 |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 681 passed |
| `npm --prefix agentj-ui run lint --workspace=@agentj/web` | exit 0（仅 warnings） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | 构建成功 |
| `bun --cwd agentj-ui/apps/tui test` | 192 pass / 0 fail |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` / `node scripts/check-relocate.mjs` | `desktop-ui-ok` / `relocate-ok` |

### 测试验收（前端相关部分）

两轮回归轮在前端侧的产出（逐条已修，详见 `_se.md` 同小节）：

- `@agentj/protocol` 的 4 个类型断言此前**从未在 CI 执行**（`frontend` 作业只跑 sdk/tokens/web，
  而断言脚本又要求其 `test` 脚本存在）→ 作业现为 17 步，protocol 测试在 protocol 构建后执行。
- oxlint 门禁（`npm run lint --workspace=@agentj/web`）未进 `CONTRIBUTING.md` 的本地清单，
  且不被「文档 ↔ CI 命令」派生断言识别 → `is_gate` 现认 `run lint`，清单补齐。
- 覆盖率 / 断言类「恒真断言」「检查未执行却算绿」缺陷（含 `--strict` 化）见 `_se.md`；
  `assert-*.sh --strict` 现为本侧本地自查入口（3 + 5 + 8 = 16 项）。
- 前端测试基线（验收轮实测）：protocol 4、sdk 55、tokens 11、web 681、TUI 192（22 文件）。

### 遗留问题 / 已知限制（前端）（本批次）

- CI 上的 `frontend` 作业尚未实跑（本地逐步跑通同一序列）；首次绿灯后需观察 oxlint warnings 噪声。
- `cross-platform` 只做 `cargo build`，desktop 打包（deb/AppImage/dmg/msi）仍未在 CI 验证
  → 记录为 `_se.md` 待实施 ㉟。
- `agentj-ui/node_modules` 仍约 470MB（迁移残留已清，体积属正常依赖）；`npm ci` 需要网络。
- `.npmrc` 指向华为云镜像；换源只需改该文件（integrity 不变）。

## 归档：run 可靠性批次（前端）✅

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
| ㉔ | 【债】三端共享逻辑收敛 | Web 19,930 行 / TUI 7,939 行各自重写会话、运行视图、文件树、搜索（`toolSummary`、files 懒加载、空文本过滤等均为双份实现）；评估把无 UI 依赖的纯函数上提 `@agentj/sdk`，避免同一缺陷修两次 | 与本轮空白过滤上提同向 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 健壮性与去重批次（前端侧）已收尾，分支推送 `origin` 与 `gitcode`，PR 开在 `origin`
  （用户负责合并）。
- 前端候选：③ Web 接入服务端搜索（消费服务端 `query` / `total`，唯一带协议消费面的待实施项）；
  ㉔ 三端共享逻辑收敛（建议与 ③ 同批：files 懒加载 / `toolSummary` / 空文本过滤都在两侧重复）；
  或订阅状态常驻指示；或 TUI `/models` 与 Web 选择语义对齐。
- 后端候选见 `summary_and_next_se.md`。
