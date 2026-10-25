# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**结构债收敛与三端纯函数共享（前端侧）**——files / toolSummary 上提 SDK +
Web 文件树接入服务端搜索，wire 面零变更。

> 当前功能：**结构债收敛 + Web 服务端搜索** —— 分支 `refactor/structure-debt-and-web-search`
> （自 `b7a3ae7` 起；前端为计划 Phase D 的 T42–T47）。后端同批次的 Rust 侧记录见
> `summary_and_next_se.md`。
> `PROTOCOL_VERSION` 仍 **0.19.0**，`VERSION` 仍 **0.1.0**；`packages/protocol/types.d.ts` /
> `schemas/*.json` 全程零 diff（`file/list` 的 `query` / `total` 是既有字段）。

## 结构债收敛 + Web 服务端搜索（前端侧） ✅

### 交付

**A. 纯函数上提 `@agentj/sdk`（同一语义不再两份实现）**

- `packages/sdk/src/files.ts`（164 行，从 `apps/tui/src/files.ts` 逐字搬迁）+ `__tests__/files.test.ts`
  （13 个用例）：`MAX_INDENT_DEPTH` / `toGitLetter` / `buildFileNode` / `updateNode` / `parentPath` /
  `flattenVisible` / `isEmptyExpandedDir` / `clampSelection` / `moveSelection` / `truncate` /
  `formatFileRowParts` / `buildSearchIndex` / `formatSearchRowParts` / `statusLetterColor`，
  含 `FileNode` / `GitLetter` / `RawFileEntry` / `RowParts` / `SearchEntry` / `VisibleRow` /
  `VisibleRowNote` 7 个类型，全部经 `index.ts` barrel 导出。
- `packages/sdk/src/toolSummary.ts`（27 行，从 `apps/web/src/RunPanel.tsx:186` 搬迁）。
- 消费者改接：TUI `apps/tui/src/files.ts` 161 → **32 行**（re-export；`moveSelection` 用 SDK 的
  clamp 变体，Web 的 wrap-aware 版本留在 `hooks/useRovingFocus.ts`）；Web `FileTree.tsx` 删本地
  `GIT_LETTERS` / `toGit`，`fileTreeNav.ts` 删 `parentDir`，`RunPanel.tsx` 删本地 `toolSummary`
  （1129 → 1102 行）。Web 的 `visiblePaths` 有意留在本地（Web 按父目录 Map 索引，TUI 按 children 嵌套）。

**B. Web 文件树服务端搜索（`apps/web/src/components/FileTree.tsx` 476 → 695 行）**

- 输入即检索：150ms 防抖后发 `file/list { session_id, path: null, query }`（**不发 `depth`**——
  规范 `docs/specs/ipc-protocol.md:790` 与内核 `file_list.rs:345-350` 都表明 query 模式忽略它），
  渲染服务端返回的顺序与 `total`，客户端不重排、不本地过滤。
- 模式判定：`typeof reply.total === "number"`（显式 `0` 也是合法结果）；旧内核（无 `total`）
  显示「不支持服务端搜索」并以空结果呈现，不降级为本地过滤。
- 代次守卫：`{seq, session}` 快照比对（`fileTreeSearch.ts` 的 `replyIsCurrent` 纯谓词，
  成功与错误路径共用），session 用 `useLayoutEffect` 刷新的 ref 做**绘制前**同步，
  乱序回复与跨会话回复都被丢弃。
- 状态行优先级：`搜索失败：<message>`（保留原有行与 git 字母，丢 `total`/`truncated`）→
  旧内核提示 → `搜索中…`（防抖等待中或请求在途）→ `输入关键字搜索文件名`（空串）→
  `搜索到 <rows> / <total> 项`，截断时追加 ` · 部分结果`。
- 键盘可达：结果容器可聚焦并处理 `↑` / `↓`（移动活动行）与 `Esc`（从输入框或结果行均可退出搜索态、
  恢复原树并取消未触发的尾部请求）；搜索态下图树快捷键（`j` / `k` / `r`）被置为惰性，
  不会改动被隐藏的树；目录命中用文件夹图标。
- `filetree.css` 146 → 181 行（搜索盒 / 状态行 / 结果行 / 空态）。

### 验证

| 门禁 | 结果 |
|------|------|
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 75 passed（9 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 701 passed（55 files；本批次前 681） |
| `bun --cwd agentj-ui/apps/tui test` | 193 pass / 0 fail（22 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | 通过 |
| `npm --prefix agentj-ui run build --workspace=@agentj/sdk` | 通过（`tsc --noEmit`） |
| `npm --prefix agentj-ui run lint --workspace=@agentj/web` | 0 error（警告均为既有） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` + `generate` | 通过，`git status --porcelain packages/protocol` 空 |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | `desktop-ui-ok` |
| `bash scripts/tests/assert-ui.sh all` | 3 passed |
| `./scripts/check-version.sh` | `version-ok` |

### 测试验收（`jskill-test-acceptance`，2 轮）

独立 tester 复验前端侧全部行为，**零前端 Bug**：旧内核 `file/list`（无 `total`）⇒ 提示「内核不支持服务端搜索」
且 0 行结果、不本地重排；错误保留既有行；`· 部分结果` 截断提示；Esc 恢复树（含结果行聚焦态）；
三次击键仅发 1 次请求且参数为 `{ session_id, path: null, query }`；`total:0` 不被误判为旧内核；
迟到回复代际守卫；SDK/TUI 共享纯函数行为一致。6 个验收 Bug 全在后端（见 `_se.md`），
前端只需在修复后复跑：sdk 75 / web 701 / tui 193 全绿。

### 决策与偏差（本批次）

- **Web 搜索逻辑留在 `FileTree.tsx`**（计划只列该文件）：内部已按函数拆分（`applySearchReply` /
  `searchHead` / `toSearchEntry` / `SearchBar` / `SearchResults`），~90 行无 JSX 的 `SearchState`
  适合后续提为 `hooks/useFileSearch.ts`。
- **`depth` 不发**：计划写了 `depth: 8`，但服务端在 query 模式忽略该参数，发它只会让测试固化了
  一个假机制——实测规范与内核两处一致后删除。
- **可观测性不可测的一处**：`session` 守卫从 passive effect 移到 `useLayoutEffect` 的 ref；
  jsdom + `act` 无法复现「passive effect 之前的一帧」，故改为把守卫提成纯谓词并钉住语义
  （session 不一致时即使 seq 与 query 都相同也丢弃），DOM 级模拟测试因对旧实现同样通过而被删除
  （不留假绿）。
- **TUI `files.test.ts` 未收窄**（~190 行仍在重测 SDK 已覆盖的行为）——不属于计划范围。

### 遗留问题 / 已知限制（本批次）

- `apps/web/src/components/FileTree.tsx` 695 行承载两套状态机（懒加载树 + 搜索），见上条的提取建议。
- `apps/web/src/RunPanel.test.tsx` 的字面量需写成 `{ name, args }`（SDK 签名不容 `kind/id/state`
  多余属性），另加一条 `RunEntry` 结构化断言保持兼容性可见。
- TUI 的 `files.test.ts` 与 SDK 测试重复（见上）。

## 归档：依赖刷新批次（前端）✅

分支 `chore/dependency-refresh-batch`（自 `c82c679`）。

### 交付

- **依赖升级**：`apps/web` react/react-dom 19.3、vite 8.3、vitest 5、jsdom 30、
  `@testing-library/jest-dom` 7、`lucide-react` 1.60、oxlint 1.89；`apps/tui` `@opentui/*` 0.6；
  `apps/desktop` `@tauri-apps/cli` 2.12。
- **vitest 版本统一**：`packages/{protocol,sdk,tokens}` 同步升到 vitest 5——否则 jest-dom 7 的
  `Assertion` 类型增强会绑到被提升的 vitest 4 类型上，`tsc -b` 报一批 `TS2339`。
- **TUI 测试改等帧**：`@opentui` 0.6 的 `captureCharFrame()` 在渲染进行中调用会抛
  `OutputBusy (status: -13)`，且 `flush()` 后抓帧可能落后一帧；`app.test.tsx` / `tui-flow.test.tsx`
  的 6 处断言改为 `await setup.waitForFrame(predicate)`，应用代码未改。
- **`scaffold.test.ts`**：`lucide-react` 断言从 `^0.` 改为 `^1.`。

### 验证

| 门禁 | 结果 |
|------|------|
| `bash -c 'bun --cwd agentj-ui/apps/tui test'` | 193 pass / 0 fail（22 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 678 passed（54 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 59 passed（7 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | 通过 |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | `desktop-ui-ok` |
| `./scripts/check-version.sh` | `version-ok` |

## 归档：健壮性与去重批次（前端）✅

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

> 本批次把「Web 服务端搜索」与「三端共享纯函数」两项实施完毕并移出本表；当前表中暂无前端条目——
> 下一特性由 `jskill-feature-next` 合并 `_se.md` 待实施表选取。

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 结构债收敛批次（前端侧）已收尾，分支推送 `origin` 与 `gitcode`，PR 开在 `origin`
  （用户负责合并）。③（Web 服务端搜索）与 ㉔（三端共享纯函数：files / toolSummary）已实施，
  从上表移除。
- 前端候选：`useFileSearch` 提取（FileTree 两套状态机分离）；TUI `files.test.ts` 收窄为
  表层断言 + 冒烟；订阅状态常驻指示；TUI `/models` 与 Web 选择语义对齐；
  Web 设置面板的项目/演示数据 tab 与 `project_root` 的进一步打通。
- 后端候选见 `summary_and_next_se.md`。
