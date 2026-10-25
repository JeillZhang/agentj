# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**结构债收敛与三端纯函数共享（前端侧）**——后端同批次见
`summary_and_next_se.md`；逐任务执行记录见 `.superpowers/sdd/progress.md`（本地文件，不入库）。

> 当前功能：**结构债收敛批次（前端侧）** —— 分支 `refactor/structure-debt-consistency-batch`，
> 前端侧为 T16–T30 + T32。`VERSION` 仍 **0.1.0**，`PROTOCOL_VERSION` **0.20.0**（wire 无变更）。
> `agentj-ui/packages/protocol` 与 `methods.ts` 未重新生成（协议未动）。

## 结构债收敛批次（前端侧） ✅

### 交付

**A. Web `FileTree.tsx` 拆分（T16–T19）**

- 796 行 → 415 行组件；纯逻辑与状态机各自独立：
  `components/fileTreeModel.ts`（normalize / toView / flatten / collectStats）、
  `components/fileTreeSearch.ts`（搜索状态机，与既有代际守卫同处一文件）、
  `hooks/useFileTree.ts`（懒加载 + 折叠保留子树 + 根/已展开刷新）、
  `hooks/useFileSearch.ts`（150ms 防抖的服务端搜索）。
- 行为不变：文件树相关 36 个用例原样通过；web 测试 706 → 723。
- 取舍：`NodeList` 改为接收 `UseFileTreeResult` 对象（原来是逐个回调 prop，拆分后 prop 爆炸）；
  `useFileSearch` 在渲染期复位搜索态以守住 oxlint 告警预算。

**B. TUI 测试收窄 + SDK 补齐（T20）**

- `agentj-ui/apps/tui/src/files.test.ts` 169 行 / 12 用例 → 21 行 / 2 用例：只留「barrel 表层 pin」+
  冒烟（十个 describe 原本与 SDK 重复）。
- 迁移缺失覆盖到 SDK：`MAX_INDENT_DEPTH` 常量 pin + 嵌套 `updateNode` patch 用例。
- tui 201 → 192，sdk 82 → 84（净减少是去重，不是丢覆盖）。

**C. 模型标记语义统一（T21–T24）**

- `agentj-ui/packages/sdk/src/modelMeta.ts`：导出 `modelKey`（`provider\0name`）与
  `resolveModelMarker(providers, selected, defaultModel) -> { index, match }`——显式带 provider 的
  选择优先；否则只有「唯一一个 provider 提供该默认模型名」才标记；都不成立返回 `-1`。
- Web `ComposerOptions.tsx` 删掉本地副本改为消费 SDK；选中模型不在列表内时显示「当前未提供」
  （`data-served=false`），不再假装第 0 行就是选中项。
- TUI `models.tsx` 同样改为 `resolveModelMarker`；`useModelConfig(client, refreshSeq)` 在
  `auth/setApiKey` 成功或 `clearCredential` 后经 `catalogSeq` 递增重取 `model/list`，
  不必重启就能看到新目录。

**D. 订阅状态常驻指示 + 趋势图交互（T25–T30）**

- `hooks/useSubscriptionStatus.ts`（238 行）：每个 shell 一份、**无任何定时器**；焦点只在距上次取数
  > 300s（`SUBSCRIPTION_FRESH_MS`）时补一次；`!authenticated` 归 `unknown` 且不发请求；
  取最差窗口分级 ok / warn(≥70) / error(≥90)；重取失败保留上次窗口并转 `error`，凭据失效单独归
  `invalid`（`errorKind: "unavailable"` 用于传输失败）。
- `components/SubscriptionStatus.tsx`（105 行）：顶栏指示灯，`data-state` 分级、陈旧加 `~` 与
  「N 分钟前」、未链接凭据显示「未链接凭据」并可跳到 设置 → 订阅；打开时才 `role="tooltip"`。
  打开时刻在 hover/focus 处理器里取（`openedAt`），渲染期不跑时钟。
- `components/SubscriptionPanel.tsx` 363 → 309 行：接受注入的 `status`（共享同一份快照，不再每次
  挂载都自称取数），只读用量块拆到 `SubscriptionUsage.tsx`(84) + `subscriptionModel.ts`(63)，
  窗口文案与错误表三处共用一份。
- `components/UsageSparkline.tsx` 35 → 123 行：逐点命中带（`<title>` 显示百分比 + 本地时刻）、
  `role="tooltip"` 读数；键盘路径（`role="group"` + `tabIndex=0`，默认停最新样本，`←/→` 逐点、
  `Home/End` 首尾、`Esc`/失焦收起）+ `aria-live` 复述摘要；样本不足两条仍不画图。
- TUI `status.ts`(66) + `app.tsx`：状态行追加 `· sub 12%/40%`（最差窗口、陈旧加 `~`、≥70/≥90 变色），
  每个连接开启时读一次（无轮询）。

**E. 文档（T32）**

- `docs/guide/cli.md` / `configuration.md`：无审批器当场拒绝 + 那行 stderr 警告，`timeout_ms` 只管
  「已投递未回答」；`docs/guide/subscription.md`：新增「常驻状态指示灯」小节（无轮询 / 年龄可见 /
  300s 焦点补取）替换掉旧的「不做常驻徽标」非目标，并补趋势图的悬停 + 键盘读值。

### 门禁

web 750 测试 / 61 文件、sdk 89、tui 202 / 23 文件；`vite build` 通过；`oxlint` **0 error / 48 warning**
（与基线持平——`TopbarMenu` 改为「注册回调」而不是写穿父级 ref，消掉了一条 `react(immutability)`）。
`tokens` 未新增任何变量；`check-version.sh` / `check-desktop-ui.mjs` / `assert-ui.sh all` 全过。

## 归档：用量历史趋势 + 模型窗口标签（前端侧） ✅

SDK 增 `formatContextWindow` / 趋势格式化；Web 订阅面板每窗口行下加迷你曲线、模型列表显示真实窗口；
TUI 订阅浮层追加块字符曲线与首尾值；`check-artifacts.sh linux-deb` 档 + 与 `0.20.0` 同步的
`types.d.ts` / `methods.ts`。详见 `docs/changelog/v0.1.md`。

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

- **窗口标签只在有 `context_window` 时出现**：目录快照里仍有 7 个模型没有窗口（不猜），
  这些模型在下拉/列表里不显示容量标签——不是 bug，是刻意的「未知即留白」。
- **趋势依赖服务端取样密度**：`history_enabled = false`、或客户端与服务端保留期不一致时，
  趋势会变短或为空；Web 侧只提示「趋势需要至少两次取样」，不自行推断缺失点。

## 待实施（跨阶段保留：未实施不得删除）

> 本批次把「FileTree 拆分 / TUI 测试收窄 / 模型标记统一 / 订阅指示与趋势交互」实施完毕（本表无对应
> 条目）；当前表中暂无前端条目——下一特性由 `jskill-feature-next` 合并 `_se.md` 待实施表选取。

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本批次（结构债收敛，前端侧 T16–T30 + T32）已收尾：FileTree 拆分、TUI 测试收窄、模型标记语义统一、
  订阅状态常驻指示 + 趋势图交互、文档同步；门禁全绿（web 750 / sdk 89 / tui 202，oxlint 0 error / 48
  warning，vite build 过）；分支推送 `origin` 与 `gitcode`，PR 开在 `origin`（用户负责合并）。
- 表外可选改进：`SubscriptionPanel.tsx` 仍有 309 行（下一步可把「历史抽屉 + 凭据卡片」再分一层）；
  `SubscriptionStatus` 目前只在 shell 顶栏，尚无键盘直达设置页的快捷键；`UsageSparkline` 的命中带
  在极窄宽度下点距 < 8px 时只做钳制、未做密度降级。
- 后端候选见 `summary_and_next_se.md`（⑰⁺ / ㉟⁺ / ㊱⁺ / ㊲⁺）。

> 远端：`origin`（GitHub `JeillZhang/agentj`）是唯一权威远端，PR 只在此创建与合并；
> `gitcode` 是只读镜像（快进推送同步，不再开 MR）。feature 分支两远端同推同一 SHA。
