# AGENTS.md

> **当前版本 0.24.2** · `PROTOCOL_VERSION` **0.15.0**（21 方法 / 52 schemas / 17 crates）
> 本文件只记录**约束、入口与基线**（Setup / Commands / Architecture / Conventions / Rules）。
> **逐版本变更与历史决策不写在这里** —— 见 [`docs/changelog/`](docs/changelog/)、
> [`docs/guide/`](docs/guide/)、[`docs/specs/`](docs/specs/) 与 `.agents/summary/`。

## Project Overview

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。
后端 17 个 crate 位于 `crates/`；前端 monorepo 位于 `agentj-ui/`（npm workspaces
`apps/*` + `packages/*`，三端共享 SDK / protocol / tokens）；WebSocket 网关同端口复用。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`agentj-ui/apps/web` + `agentj-ui/packages/protocol`
  + `agentj-ui/packages/sdk` 构建与测试需要）
- Bun ≥1.2（`agentj-ui/apps/tui` dev/build/test 需要）
- Rust 工具链（`agentj-ui/apps/desktop` Tauri 壳构建需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试（含 bus 密封 + FileBackedStore/Config/CLI 单测
  + e2e_cli/e2e_permission/e2e_serve/e2e_ws/e2e_http_static 密封测试）
- `cargo clippy --workspace --all-targets -- -D warnings` — lint 门禁（零警告）
- `cargo bench -p agentj-model` — 模型流式基准（criterion）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（schemas 无漂移）
- `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` — 生成 protocol 类型
  （`schemas/*.json` → `agentj-ui/packages/protocol/types.d.ts`）
- `npm --prefix agentj-ui run build --workspace=@agentj/protocol` — `tsc --noEmit` 校验
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — SDK 单测
  （client/router/transports/config）
- `npm --prefix agentj-ui run generate --workspace=@agentj/sdk` — 生成 `METHODS` 常量
  （`protocol/methods.json` → `agentj-ui/packages/sdk/src/methods.ts`）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — Web 单测（vitest）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — Web 构建（`tsc -b && vite build`）
  > 注意：Vite dev server 不监听 root 之外的 `packages/sdk` 源（alias 直连源码）；
  > **改动 SDK 后须重启 dev server**，否则运行陈旧的 `METHODS`（曾导致文件 tab 永久「加载中」）。
  > Web 依赖 `rehype-highlight`（hljs common 语言集），随 `npm --prefix agentj-ui install`
  > + lockfile 落地。
- `bun --cwd agentj-ui/apps/tui test` — TUI 单测（含 mock WS e2e）
- `./agentj-ui/apps/desktop/check-artifacts.sh linux` — Desktop 产物校验
- `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — Desktop 前端产物/配置一致性校验
- `./scripts/check-version.sh` — 版本一致性（Cargo + sdk `VERSION` + desktop 同步，
  `PROTOCOL_VERSION` 与 `protocol/methods.json` 版本一致）

> 各门禁的当前测试数量与最近一次结果见 `.agents/summary/summary_and_next_{se,fe}.md`；
> 本文件不写死数字（会漂移）。

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试（PR + push main + 手动）：
  `cargo fmt`、`cargo clippy`、`cargo test --workspace --locked`、
  `cargo llvm-cov --workspace --fail-under-lines 50`
- `.github/workflows/smoke.yml` — 冒烟（PR + push main + 手动）：
  `cargo build --release` + `agentj --version/--help` 断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E（main 合并后 + 手动）：
  `cargo test --release --test e2e_cli --test e2e_permission`
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`

## Architecture

> 全部 17 个 crate 位于 `crates/`（`crates/*`），前端 monorepo 位于
> `agentj-ui/`（npm workspaces，frontend stays in-repo，不拆独立仓），
> 下文 `agentj-*` 均为 `crates/agentj-*`，`@agentj/*` 为 `agentj-ui` 包。

- `agentj-protocol`（leaf）：IPC 协议权威 wire 类型（JSON-RPC 2.0、错误码、
  21 方法 params/result、流式/订阅/权限/提问/文件列举类型），serde + schemars，
  零内核耦合。协议版本 `0.15.0`，52 schemas。
- `agentj-core`（内核）：Kernel 混合总线 + 模块 trait 注册表
  + ServiceRegistry（同步/异步） + TurnRunner 总线化 turn 编排
  + `ToolCallContext`（会话根上下文，工具相对路径重定基）
  + `TurnArtifact`（FileChange/Todo 产出 + 自研 unified diff，per-run `run_id`/`seq`）
  + 事件总线重入防护（EventGuard thread_local + MAX_EVENT_DEPTH=16）。
- `agentj-config`：TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）。
- `agentj-session`：会话生命周期（InMemoryStore/FileBackedStore + 过期清理
  + 会话级 `project_root` 持久化，serde default 兼容旧快照）。
- `agentj-permission`：权限系统（决策引擎 + 60s 超时 + allow_always）。
- `agentj-question`：提问通道（`QuestionModule` + `AskTool`，镜像 permission 的
  pending/owner 归属；`ask_blocking` 阻塞等待、`respond` 幂等 `accepted:false`、
  `timeout_s` 可空、`set_notifier`/`set_settle_hook` 统一结算一次）。
- `agentj-model`：模型提供方（三线 wire-format + opencode-go + Mock + Ollama）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write + `todo`，共 6 个）。
- `agentj-context`：上下文管线。
- `agentj-stats`：用量统计 UsageCollector。
- `agentj-license`：Ed25519 许可证校验（entitlements + free mode）。
- `agentj-mcp`：MCP 客户端 + McpConnectionManager 长驻连接
  （McpServerSpec + close_all）。
- `agentj-skills`：skills 加载器。
- `agentj-plugin`：插件系统（manifest + 状态机 + 依赖图 + libloading）。
- `agentj-ipc`：JSON-RPC IPC（stdio/TCP/WS + 同端口分类器 + WsConnection
  + WsServerState/ws_router/serve_http + per-session 路由
  + `file/list` dispatch（沙箱 + git 状态；目录聚合后代最高优先级字母；`query` 子树搜索
  独立访问预算 20000 + rank + Top-100 + `total`）+ question notifier/settle hook 接线
  + `error.data` 透传 + 非法结构请求回显可确定 id（可确定 id → `-32600`；语法错误
  → `-32700` + `id:null`）。**错误码表与鉴权通道契约以
  [`docs/specs/ipc-protocol.md`](docs/specs/ipc-protocol.md) 为准**，本文件不复述。
  - `web` 子模块（同端口静态托管 + 混合鉴权）：`WebConfig` + `build_router`
    （`/ws` + `/login`/`/logout` + 静态 `ServeDir`/SPA fallback；两静态服务各自
    包裹 cache 层与登录门，避免 `Router::layer` 波及 `/ws`）+ 不透明会话 cookie
    + 同源 Origin 校验 + `next` 白名单（防开放重定向）+ WS cookie 第三通道
    （`WsConnection::with_init_token` + `supply_initialize_token` 注入 server token）。
  - `login_limiter`：按来源 IP 失败计数与指数锁定（第 5 次失败起 30s、×2、封顶 900s；
    `429 + Retry-After`；idle_ttl 1800s / max_tracked 1024 淘汰；回环同样计数）
    + `snapshot()` 只读投影（供指标导出）。
  - 可观测性：`auth_required()` + 鉴权门控 `GET /metrics`（Prometheus 文本：
    聚合计数 + per-IP series Top-50 截断 + 截断指示 + integrity 摘要；
    回环/未配 token/有效 cookie 直出，远程无 cookie → `401` 不重定向）；
    未鉴权 `GET /version`（200 JSON + `no-store`，显式路由优先于 SPA fallback）
    暴露 `DistVersion` 与 `startup_warnings()`（dist 版本/清单缺失、不可解析、
    不一致三类启动警告，不阻断服务）。
  - 资产完整性：构建期 `asset-manifest.json`（`sha384` 覆盖 dist 全部部署文件，
    与 SRI `integrity` 逐字节一致）→ 启动逐文件校验 + 60s / 8 文件游标轮转运行期抽检
    （`SpotCheckConfig`，结果经 `/version` `integrity.runtime` 与 `/metrics` 双面暴露）。
  - 依赖：`tower-http`（fs/set-header）+ `uuid`（v4）。
- `agentj-cli`：`agentj` 二进制（run/serve/dashboard，模块注册进 Kernel，
  agentj-question 的 stdio 与 `serve --port` 两条装配路径）；`serve`/`dashboard`
  支持 `--host`（默认仍是 127.0.0.1 本机信任）、`--token`、`--web-dist <PATH>`
  （显式 flag → cwd `agentj-ui/apps/web/dist` → exe 相对两档四级发现），
  启动打印 `http://<host>:<port>/` 并输出 `web_config().startup_warnings()`。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具。
- `schemas/*.json`：52 JSON Schema（schemars 生成，提交件 + drift 门禁）。
- `protocol/methods.json`：方法注册表（method ↔ schema/方向/类型；21 方法，
  protocol_version 0.15.0）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范——**错误码表、WS/HTTP 鉴权
  通道、各方法参数/结果契约的权威来源**。

- `agentj-ui/packages/protocol`：协议包（原 `bindings` 迁入，
  `schemas/*.json` → `types.d.ts`，`json-schema-to-typescript` + `tsc --noEmit`，
  为 drift 门禁后第一门）。
- `agentj-ui/packages/sdk`：共享 SDK（`src/client.ts` RpcClient +
  `src/router.ts` ResponseRouter/RpcTransport + `src/config.ts`
  `VERSION` / `PROTOCOL_VERSION` / `resolveWsUrl` + `src/methods.ts` METHODS
  + `transports/websocket.ts` + `transports/tcp.ts`；`RpcClient.request` 拦截
  空/未定义 method；`index.ts` barrel 导出 `VERSION`），被 Web/TUI/Desktop 同源消费。
  - `resolveWsUrl` 页面地址派生：无 `VITE_AGENTJ_WS_URL`/显式 host 时取页面
    `location.hostname:4096`（https → wss；`tauri.localhost` 保持明文回环）。
  - 手动连接持久化到 `agentj.connection`（刷新自动恢复并重连）；显式设置优先于
    `VITE_AGENTJ_WS_URL`。
- `agentj-ui/packages/tokens`：共享语义 tokens（`palette.ts` 24 keys × dark/light
  + `SEMANTIC_KEYS` + `LAYOUT` 常量块（left/right 宽度/splitter/topbar/gap），
  `tokens.css/tokens.json/docs/ui-tokens.md` 生成，三端约束）。
- `agentj-ui/apps/web`：Web 前端（Vite + React 19，消费 `@agentj/sdk` +
  `@agentj/tokens` + `lucide-react` + `rehype-highlight`）。
  - 三栏工作台：`WorkspaceShell`（Sessions/Files 左栏 `SidebarTabs` + 会话中栏
    + 运行详情右栏）+ `SplitPane`（拖拽/键盘/双击）+ `RightPanel`
    （`TodoPanel` ← `$/progress` todo 全量快照 + revision 门控；`ChangeList`
    ← `file_change` upsert + unified diff + run 分组 + 历史抽屉）+ `FileTree`
    （`file/list` 懒加载 + git 状态字母，目录行聚合后代最高优先级字母）
    + `AskQuestionCard`（`$/askQuestion` → `question/respond`）
    + `ConnectionStatus`（状态灯 + hover 连接面板 + 版本行 UI/内核一致性告警）。
  - 对话与输入：`CodeBlockCard`（`pre` 覆盖 + `copyText` 降级链 + 语法高亮）
    + `RunPanel`（`composerCollapse.ts` 滚动自动收缩纯状态机：48/40 迟滞 +
    260ms 锁 + 程序化跟随排除；`data-collapsed` + 双职责发送按钮
    + `--aj-composer-h` 冻结）+ composer 75% 居中浮动
    + `OptionPopover`/`ComposerOptions`（模型/Agent/熵变/权限）
    + `tokenUsage.ts`（`Σ` 会话 token 输入/输出拆分读出）。
  - 设置与导航：`TopbarMenu`（项目/继续会话…/收藏会话/设置…/帮助 + 二级 hover 飞出）
    + `SessionListMenu`（清理/过滤/排序）+ `SettingsDialog`（连接/显示/项目/演示数据
    tab）+ `SettingsDisplayPanel`（逐区 px 字号）+ `SettingsProjectPanel`
    + `ProjectEditDialog` + `project_root` 随 `session/new` 打通
    + `ModalDialog`（模块级模态栈；`.aj-dialog-frame` 8 向缩放 + `dialogResize.ts`）
    + 共享 `useRovingFocus` + `useShortcutScope`（侧栏快捷键作用域）+ 快捷键提示行。
  - 状态与构建：`layoutStore` / `projectStore` / `displayPrefsStore` /
    `sidebarPrefsStore` + `todoState`/`changes`（`ChangesState {summary, history}`）
    reducer + `build/version-sri.ts` + `version-sri-plugin.ts`（生成
    `dist/version.json` + 入口 HTML `sha384` SRI 注入，本地引用缺失即构建失败）；
    vitest + vite build。
- `agentj-ui/apps/tui`：TUI 前端（Bun + @opentui/react）：connection/session/composer/
  runview/permission/palette/models/completion/theme/usage + runview 只读 TODO 区块
  （revision 门控）+ `UnifiedSidebar`（文本版 32char）+ Threads/Files 切换（`f`）；
  Files 视图 = `files.ts` 纯函数 + `FilesView` + `useFileTree`（懒加载 `file/list`
  depth=1、stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取）
  + 键位 `j,k,↑,↓` / `Enter,l,→` / `h,←` / `r` / `s` 搜索（防抖 150ms 服务端
  `file/list {query}`，rank + `total`；旧内核无 `total` 自动降级 depth-8 本地过滤）
  + 展开态经 `filetree.json` 按会话 `project_root` 持久化 + mock WS e2e + `testRender` 快照。
- `agentj-ui/apps/desktop`：Desktop 壳（Tauri v2，`frontendDist` → `../../web/dist`，
  与 workspace 版本同源，四件套 deb/AppImage/dmg/msi 打包）。
- `agentj-ui/` 根：`package.json` workspaces `["apps/*","packages/*"]`
  （frontend stays in-repo，前端不拆独立仓，与 Rust 共享同一版本号）。
- **守护内核**：统一 Connection 抽象 + `serve --port` 常驻多客户端 + `run --attach`
  + `dashboard`（见 daemon-kernel 设计文档）。

> Crate 边界：Rust 17 crate 保持 `crates/*` 边界纯净，
> 前端（TS/React/Bun/Tauri）全部收敛于 `agentj-ui/` 内，
> 通过 `protocol/methods.json` + `schemas/*.json` 与后端契约对齐，
> 不向 Rust workspace 注入前端构建产物。

## Conventions

- 协议 wire 字段一律 snake_case（`protocol_version`、`session_id` 等）。
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；
  未知能力/枚举值容忍）。
- 值枚举带 `#[serde(other)]` + `#[schemars(skip)]` Unknown 变体（向前兼容）。
- Rust function ≤50 lines，≤120 chars per line。

## Rules

### 文档分工（写文档前先看这里）

- `AGENTS.md`（本文件）：只记录**约束 / 入口 / 基线**——Setup、Commands、Architecture、
  Conventions、Rules、当前版本与协议版本。**不记录逐版本变更、逐版本特性或阶段进度**；
  架构条目只描述「现在是什么」，不标注「哪个版本引入的」。
- `README.md`：只保留项目简介、高层特性概览、结构、快速上手、协议摘要与开发命令；
  **不记录详细版本变更与逐版本特性**。版本行只同步「当前版本 + 指向 `docs/changelog/` 的链接」。
- 版本变更记录 → `docs/changelog/`（每个次版本一个文件 `v0.<minor>.md`，补丁号作为同文件
  小节；Keep a Changelog 风格 `Added`/`Changed`/`Fixed`/`Removed`；新次版本另建文件并更新
  `docs/changelog/README.md` 索引）。
- 用户可见特性 → `docs/guide/`（按主题，去版本号叙事）。
- 当前正在开发功能的进度 / 遗留问题 / 待实施 / 下一步 → `.agents/summary/`（见下）。
- 协议与设计细节 → `docs/specs/`、`docs/superpowers/specs/`。
- `docs/` 下文档是跟踪交付物 —— 必须提交。

### 版本与提交

- Commit messages in English. `docs/superpowers/specs/` 文档用中文。
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`)。
- 涉及协议 wire 的变更必须同步 `PROTOCOL_VERSION` + `protocol/methods.json` + schemas。

### 总结文件（前后端隔离，禁止交叉写入）

- 后端 → `.agents/summary/summary_and_next_se.md`（crates/ · schemas/ · protocol/），
  前端 → `.agents/summary/summary_and_next_fe.md`（agentj-ui/）。
- 每个文件只保留该侧**当前正在开发**的功能：进度、遗留问题、待实施、下一步。
  **替换而非追加**——但 `## 待实施` 表中尚未实现的条目必须跨阶段保留，不得删除，
  直到真正实施完成。
- 开发前必读：先读这两个 summary 与 `docs/superpowers/`，再开始规划/开发。

### 阶段收尾（每个功能阶段完成后、`cargo test` + `clippy` 通过后自动执行）

1. 变更记录追加到对应 `docs/changelog/v0.<minor>.md`（新次版本另建文件并更新索引）；
   用户特性写入 `docs/guide/`。
2. 更新 summary 文件（**.agents/summary/**，按阶段触及侧更新，可两文件同更）——**必须**。
3. `AGENTS.md` / `README.md` **仅在该阶段改变了以下内容时才更新**：架构边界、
   命令、规则、当前版本/协议版本基线。**纯功能/修复阶段不得向这两个文件追加版本叙事。**
4. Commit as `docs: auto-sync AGENTS.md, README.md, summary (fe/se) after {phase}`
   （未触及 `AGENTS.md` / `README.md` 时按实际触及文件命名）。
5. 不等待用户 —— 这是强制步骤。
- 更新 `docs/superpowers/specs` 目标 spec 文档的 TODO 列表。
- `docs/superpowers/specs/` 下所有图表必须使用 UML 或 Mermaid。
- 每阶段后清理临时文件与 Docker 容器。
- 命令挂起 >120s 时取消并重试。

### 协作

- PR creation: create PR on `origin` only and share link — user handles merge。
  **Never open a GitCode MR for the same branch**（见下方 Remote & Merge Policy）。

## Remote & Merge Policy

> 背景：2026-09-30 发现 `origin/main` 与 `gitcode/main` 提交数不一致，根因是**同一条
> feature 分支在两个远端各被合并一次**，各生成一个 merge commit——两侧 tree 完全相同，
> **纯记账差异，非内容问题**（已 double-merge 的分支：`fix/right-panel-occlusion`、
> `chore/agent-model-deepseek-v4-1-flash`、`docs/readme-changelog-guide-split`）。
> 以下规则用于防止再次发生。

- **单一权威远端**：`origin`（GitHub `JeillZhang/agentj`）是 `main` 的**唯一真源**；
  `gitcode`（GitCode `jeill/agentj`）是**只读镜像**，不接受 MR。
- **只合并一次**：PR 只在 `origin` 创建与合并。合并后**不得**再在 `gitcode` 建同名 MR。
- **镜像用快进推送同步，禁止再次合并**：

  ```bash
  git pull --ff-only origin main
  git push gitcode main:main        # 复制 commit，不新增 merge commit
  ```

  若 `gitcode/main` 落后，这是快进；若已分叉，`push` 会被拒绝——**不要用 `--force`**。
- **feature 分支可推两个远端**（同一 SHA，无害）；**合并只能在一个远端进行**。
- **健康检查看 tree，不看提交数**：

  ```bash
  git fetch origin gitcode
  git diff --stat origin/main gitcode/main              # 期望为空
  git rev-parse origin/main^{tree} gitcode/main^{tree}   # 期望相等
  ```

  提交数不同但 tree 相同 = 旧的双合并遗留记账差异，**不是内容问题**；
  tree 不同才是真分歧，需立即排查。
- **分叉后不要自动 force-push 收敛**（改写已发布 `main` 属高风险操作）——
  报告给用户，由用户决定是否重置镜像。
