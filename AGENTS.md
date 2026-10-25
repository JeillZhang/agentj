# AGENTS.md

> **当前版本 0.1.0** · `PROTOCOL_VERSION` **0.19.0**（28 方法 / 78 schemas / 15 crates）
> 本文件只记录**约束、入口与基线**（Setup / Commands / Architecture / Conventions / Rules）。
> **逐版本变更与历史决策不写在这里** —— 见 [`docs/changelog/`](docs/changelog/)、
> [`docs/guide/`](docs/guide/)、[`docs/specs/`](docs/specs/) 与 `.agents/summary/`。

## Project Overview

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。
后端 15 个 crate 位于 `crates/`；前端 monorepo 位于 `agentj-ui/`（npm workspaces
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
  > 内循环用 `cargo test -p <crate> --lib` 只跑单测；全量 e2e 现已秒级（`e2e_permission`
  > 曾因硬编码 session id 读阻塞而无限挂起，2026-10-16 修复并改用 `timeout_ms = 800`）。
  > e2e 子进程统一用 `serve --port 0` + 读回 stdout 的 `listening on`（避免先选端口再 bind 的竞态）。
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
- `cargo deny check advisories licenses bans sources -D unmatched-skip` — 依赖漏洞 / 许可 / 重复版本 / 源门禁
  （策略见 `deny.toml`；本机未装时先 `cargo install cargo-deny --locked`）
- `npm --prefix agentj-ui run lint --workspace=@agentj/web` — Web oxlint（errors 阻断，
  warnings 仅打印）
- `./scripts/check-version.sh` — 版本一致性（Cargo 工作区 + sdk `VERSION` + `PROTOCOL_VERSION`
  + `protocol/methods.json` + `agentj-ui/package.json` / `apps/web` / `apps/tui` / `apps/desktop`
  + `agentj-ui/package-lock.json` 根条目；失败打印出错文件与命中字面量。
  `AGENTJ_ROOT=<路径>` 可对任意目录跑同一套断言，`scripts/test-check-version.sh` 是其正/负样例）
- `python3 scripts/tests/fn-length.py [PATH…]` — 逐个 `fn` 计 body 行数，>50 行即失败并列出
  `file:line: fn name = N lines`（目录参数自动展开；`/tests/` 段跳过并打印
  `scanned N file(s), skipped M test file(s)`；空目标集 `rc=2` 拒绝空过）。
- `bash scripts/tests/assert-{ui,cargo,ci}.sh all` — 仓库不变量断言（18 个 check：UI 清单 /
  单一 lockfile / Cargo 工作区策略 / 15 crate 继承 / LICENSE / deny.toml / lockfile 一致性 /
  函数长度 / CI 作业契约 / 覆盖率棘轮 / 治理文件 / 卫生 / summary / 文档基线 —— 文档基线一项
  同时断言这里的 check 数与三个脚本实际注册的一致）。`<check>` 单跑、
  `--list` 列出、`--strict` 把未实现项（SKIP）视为失败 —— **收尾门禁用 `--strict`**

> 各门禁的当前测试数量与最近一次结果见 `.agents/summary/summary_and_next_{se,fe}.md`；
> 本文件不写死数字（会漂移）。

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试与全部门禁（PR + push main + 手动），作业：
  - `lint`：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`，
    再跑「Repo invariants」（`python3 -m pip install --user pyyaml` + `taiki-e/install-action@cargo-deny`
    + 三份 `scripts/tests/assert-*.sh --strict`；`--strict` 让 `SKIP` 直接失败，未实现的检查
    不能伪装成绿）
  - `test`：先 `cargo build -p agentj-plugin-fixture`，再 `cargo test --workspace --locked`
  - `coverage`：`cargo llvm-cov --workspace --fail-under-lines <COVERAGE_THRESHOLD> --locked`
    （**棘轮**：实测 2026-10-23 行覆盖 `93.67%` → 门限 `max(50, floor(X) - 1)` = **92**，
    只升不降，改基线须同时改注释里的日期与实测值）
  - `deny`：`cargo deny check advisories licenses bans sources -D unmatched-skip`（`deny.toml`）
  - `cross-platform`：matrix `ubuntu-latest` / `macos-latest` / `windows-latest`，仅
    `cargo build --workspace --locked`（验证可编译；**不含**打包）
  - `frontend`：17 步 —— `npm ci` → protocol 生成 + 漂移门禁 → protocol 构建 →
    protocol 测试 → SDK 生成 + 漂移门禁 → sdk / tokens / web 测试 → web 构建 →
    `dist/version.json` 与 `protocol/methods.json` 协议版本一致 → oxlint → TUI（bun）测试 →
    `check-desktop-ui.mjs` → `check-version.sh` → `check-relocate.mjs`
  - `desktop-package`：Linux 打包（唯一跑 `tauri build` 的作业）—— protocol / SDK 类型生成 →
    web 构建 → `--bundles deb,appimage` → `check-artifacts.sh linux` → 上传产物；
    `timeout-minutes: 45`，不含测试套件
- `.github/workflows/smoke.yml` — 冒烟（PR + push main + 手动）：
  `cargo build --release` + `agentj --version/--help` 断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E（main 合并后 + 手动）：
  `cargo test --release --test e2e_cli --test e2e_permission`
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`

## Architecture

> 全部 15 个 crate 位于 `crates/`（`crates/*`），前端 monorepo 位于
> `agentj-ui/`（npm workspaces，frontend stays in-repo，不拆独立仓），
> 下文 `agentj-*` 均为 `crates/agentj-*`，`@agentj/*` 为 `agentj-ui` 包。

- `agentj-protocol`（leaf）：IPC 协议权威 wire 类型（JSON-RPC 2.0、错误码、
  28 方法 params/result、流式/订阅/权限/提问/文件列举/订阅服务类型），serde + schemars，
  零内核耦合。协议版本 `0.19.0`，78 schemas。
- `agentj-core`（内核）：Kernel 混合总线 + 模块 trait 注册表
  + ServiceRegistry（同步/异步） + TurnRunner 总线化 turn 编排
  + `budget`（请求 token 预算：`estimate_tokens` 估算 + `fit_messages` 裁剪
  + 上游超限时 `retry_budget` 收紧重试，`ContextPipeline::fit` 默认接入）
  + `ToolCallContext`（会话根上下文，工具相对路径重定基）
  + `text_tool_call`（把模型写成纯文本方言的工具调用还原成真正的调用；无法无歧义还原时
  纠偏重发并如实报错，不把标记当结论）
  + `TurnArtifact`（FileChange/Todo 产出 + 自研 unified diff，per-run `run_id`/`seq`）
  + 工具结果单点裁剪（`[agent] max_tool_result_chars`：超限只截一次并广播
  `ToolResultTruncated`；`AgentEvent::Done` 携带本轮生效的 `provider`/`model`）
  + 事件总线重入防护（EventGuard thread_local + MAX_EVENT_DEPTH=16）
  + `sync`（`lock_or_recover`/`read_or_recover`/`write_or_recover` 防中毒锁 helper
  + `panic_payload_message`：把 panic 负载渲染成可上报文本）
  + `pending`（`PendingRegistry`：待决条目存储 + TTL/归属回收，permission/question 共用）
  + `paths`/`tool_paths`（`resolve_within_root` 越界判定 + 工具相对路径重定基）
  + `context_pipeline`（`DefaultContextPipeline`：装配/回流纯函数；预算裁剪由 `budget` 统一下发）
  + `usage_stats`（`UsageCollector`/`SessionUsage` 用量统计）
  + `skills`（`.aj` > `.agents` > `.claude` skills 加载器）。
- `agentj-config`：TOML 配置（[model]/[agent]/[permission]/[session]/[license]/[[mcp.servers]]；
  `[permission] timeout_ms` 调整 `ask` 自动拒绝等待预算（缺省 60s，钳制 100..=600_000，
  越界钳制并告警；e2e fixture 调至 800ms）；
  `[model] max_context_tokens` 调整单次请求 token 预算（默认 128k，钳制 1k..=10M）；
  `[agent] max_tool_iterations` 调整单轮工具调用轮数（默认 50，钳制 1..=1000；
  用尽后内核改为无工具收尾要结论）；
  `[agent] max_tool_result_chars` 调整单个工具结果字符上限（默认 100_000，
  钳制 1_000..=1_000_000）；`[agent] question_timeout_s` 调整提问默认超时
  （默认 600s，钳制 1..=86_400）；
  `toml_edit` 定点写回 `[model]` 保留注释）。
- `agentj-auth`：订阅凭据（`Credential` api_key/oauth + `AuthStore`
  XDG 路径 / 目录 0700 / 文件 0600 / 原子写 / 权限与 JSON 校验拒绝降级
  + `resolve_credential` flag>env>file 优先级 + `redact` 仅末 4 位
  + `oauth` 模块（RFC 8628 设备码授权与 refresh 交换）
  + `refresh::ensure_access`（`auth.json.lock` 单飞刷新、单次 refresh token 轮换、
  60s 安全余量、失败不落盘））。
- `agentj-session`：会话生命周期（InMemoryStore/FileBackedStore + 过期清理
  + 会话级 `project_root` 持久化，serde default 兼容旧快照）。
- `agentj-permission`：权限系统（决策引擎 + 60s 超时 + allow_always；
  `respond`/`cancel_session` 只投递决定、保留待决条目至工具消费，`PENDING_TTL` 300s 回收；
  重复/迟到答复返回 `false` 不覆盖首次决定）。
- `agentj-question`：提问通道（`QuestionModule` + `AskTool`，镜像 permission 的
  pending/owner 归属；`ask_blocking` 阻塞等待、`respond` 幂等 `accepted:false`、
  `timeout_s` 可空、`set_notifier`/`set_settle_hook` 统一结算一次）。
- `agentj-model`：模型提供方（三线 wire-format + opencode-go + Mock + Ollama）
  + `usage`（防御式 `GET /v1/usage`：三窗口、未知键忽略、`percent==0` 丢弃占位 reset）
  + `console_catalog`（`GET /api/v2/config` 账号级目录，失败即 Err 以便回退）
  + `catalog`（`data/opencode-go.json` 为 `gen_opencode_catalog` 由 live `/v1/models` 与
  models.dev `provider.npm` 生成的**路由快照**，经 `from_overlay_and_live` 与 live 合并；
  models.dev 未收录的 id 一律 `openai-compat` + `verified:false`，不猜路由）
  + `gateway_error`（三 wire 共用错误映射：429 → `RateLimited`、上下文超限 →
  `TokenLimitExceeded { used, max }`、其余 → `Api { status, message }`）
  + `retry`（`temperature` 被拒时去参数重试一次）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write + `todo`，共 6 个）；`grep` 优先
  `rg`、缺失时回退进程内检索（`regex` + `globset` + `walkdir`，行匹配 + `include` 通配 +
  跳目录/二进制/超大文件 + 2000 条上限）；`glob` 改为进程内匹配（`globset` + `walkdir`，
  无 `find` 子进程，复用 `GrepTool::SKIP_DIRS` 同一跳目录表，默认 60s 到点即停）；
  两个进程型工具（`bash` / `grep`）统一经 `ToolExecutor`（`executor.rs`）以独立进程组
  执行，超时杀整组并按工具默认值（`bash` 120s、`grep` 60s）生效，可经入参 `timeout_ms`
  覆盖（钳制 1s–600s）。
- `agentj-license`：Ed25519 许可证校验（entitlements + free mode）。
- `agentj-mcp`：MCP 客户端 + McpConnectionManager 长驻连接
  （McpServerSpec + close_all）。
- `agentj-plugin`：插件系统（manifest + 状态机 + 依赖图 + libloading）。
- `agentj-ipc`：JSON-RPC IPC（stdio/TCP/WS + 同端口分类器 + WsConnection
  + 注入式 `ProviderService` handle（`model/list` / `auth/status` / `provider/usage` /
  `auth/deviceStart` / `auth/devicePoll` / `auth/setApiKey` / `auth/clearCredential`；
  未注入统一 `-32603 provider_service_unavailable`，ipc 不依赖 auth/model）
  + WsServerState/ws_router/serve_http + per-session 路由（`session/subscribe` 在会话
  无 owner 时认领归属，供断线重连后继续审批）
  + `file/list` dispatch（沙箱 + git 状态；目录聚合后代最高优先级字母；`query` 子树搜索
  独立访问预算 20000 + rank + Top-100 + `total`）+ question notifier/settle hook 接线
  + owner 断连结算（question 与 permission 待决项）
  + `error.data` 透传 + 非法结构请求回显可确定 id（可确定 id → `-32600`；语法错误
  → `-32700` + `id:null`）。**错误码表与鉴权通道契约以
  [`docs/specs/ipc-protocol.md`](docs/specs/ipc-protocol.md) 为准**，本文件不复述。
  - **模块布局**（`lib.rs` 仅剩装配 + 再导出，≤600 行）：`dispatch`（`handle_request_from` /
    `auth_gate` / `dispatch_request` / `DISPATCH_METHODS` 方法表）+ `handlers/{lifecycle,
    session,run,pending,provider,file}`（24 个方法处理器）+ `rpc`（JSON-RPC 信封 +
    `DispatchError`）+ `registry`（`Connection` / `ConnectionRegistry`）+ `frames`（帧构造与
    artifact wire 映射）+ `stdio` / `tcp` / `web` / `ws` + `tests/*`（按域分文件）。
    两道防漂移 pin：`DISPATCH_METHODS` ↔ `protocol/methods.json` 双向；`tests/pins.rs` 断言
    已搬走的符号不再回潮到 `lib.rs` / `web.rs` / `tcp.rs` / `ws.rs`。
  - `web` 子模块（同端口静态托管 + 混合鉴权）：`WebConfig` + `build_router`
    （`/ws` + `/login`/`/logout` + 静态 `ServeDir`/SPA fallback；两静态服务各自
    包裹 cache 层与登录门，避免 `Router::layer` 波及 `/ws`）+ 不透明会话 cookie
    + 同源 Origin 校验 + `next` 白名单（防开放重定向）+ WS cookie 第三通道
    （`WsConnection::with_init_token` + `supply_initialize_token` 注入 server token）。
  - `secure`：`ct_eq`（常量时间比较）+ 会话/清除 cookie 的单点构造
    （`HttpOnly; SameSite=Lax; Path=/` + 可选 `Secure`/`Max-Age=0`，序列化失败记日志）。
  - 请求入口 panic 隔离：`handle_request_from` → `dispatch_request`（24 方法）经 `isolate_panic`
    兜底，panic 转 `-32603` `InternalPanic`（`error.data { method, reason }`）。
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
- `agentj-cli`：`agentj` 二进制（run/serve/dashboard/auth，模块注册进 Kernel，
  agentj-question 的 stdio 与 `serve --port` 两条装配路径）；`auth login --device`
  （设备码登录，与 `--api-key` 互斥）+ `CliProviderService`（ProviderService 实现：
  usage 300s 缓存 / 三层 `model/list` / 设备流）+ `RefreshingProvider` 装饰器
  （OAuth 凭据每次 chat 前保鲜，仅密钥变化才重建）；`serve`/`dashboard`
  支持 `--host`（默认仍是 127.0.0.1 本机信任）、`--token`、`--web-dist <PATH>`
  （显式 flag → cwd `agentj-ui/apps/web/dist` → exe 相对两档四级发现），
  启动打印 `http://<host>:<port>/` 并输出 `web_config().startup_warnings()`。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具。
- `schemas/*.json`：78 JSON Schema（schemars 生成，提交件 + drift 门禁）。
- `protocol/methods.json`：方法注册表（method ↔ schema/方向/类型；28 方法，
  protocol_version 0.19.0）。
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
  - 三端共享纯逻辑：`src/files.ts`（文件树模型/导航/行格式化/搜检索引：`buildFileNode` /
    `updateNode` / `flattenVisible` / `moveSelection` / `formatFileRowParts` /
    `formatSearchRowParts` / `toGitLetter` / `parentPath` …）+ `src/toolSummary.ts`
    （工具调用摘要）：Web 与 TUI 都只消费这一份，不得在应用内重写。
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
  `file/list {query}`，内核 `file_list::search_rank` 是唯一排名真源；TUI 只渲染服务端返回的顺序与
  `total`，回复无 `total` 时提示「不支持服务端搜索」并置空行，不再本地降级）
  + 展开态经 `filetree.json` 按会话 `project_root` 持久化 + mock WS e2e + `testRender` 快照。
- `agentj-ui/apps/desktop`：Desktop 壳（Tauri v2，`frontendDist` → `../../web/dist`，
  与 workspace 版本同源，四件套 deb/AppImage/dmg/msi 打包）。
- `agentj-ui/` 根：`package.json` workspaces `["apps/*","packages/*"]`
  （frontend stays in-repo，前端不拆独立仓，与 Rust 共享同一版本号）。
- **守护内核**：统一 Connection 抽象 + `serve --port` 常驻多客户端 + `run --attach`
  + `dashboard`（见 daemon-kernel 设计文档）。

> Crate 边界：Rust 15 crate 保持 `crates/*` 边界纯净，
> 前端（TS/React/Bun/Tauri）全部收敛于 `agentj-ui/` 内，
> 通过 `protocol/methods.json` + `schemas/*.json` 与后端契约对齐，
> 不向 Rust workspace 注入前端构建产物。

## Conventions

- 协议 wire 字段一律 snake_case（`protocol_version`、`session_id` 等）。
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；
  未知能力/枚举值容忍）。
- 值枚举带 `#[serde(other)]` + `#[schemars(skip)]` Unknown 变体（向前兼容）。
- Rust function ≤50 lines，≤120 chars per line。
- 锁与 panic：锁一律走 `agentj_core::sync::{lock_or_recover, read_or_recover, write_or_recover}`
  （中毒记日志并取回内部状态，禁止 `.lock().unwrap()`）；长驻任务（内核消息循环 / 请求派发 /
  一轮 turn）必须 `catch_unwind` 隔离，且 panic 后仍完成协议契约（错误事件 / `-32603` /
  释放 turn 槽）；`let _ =` 只用于无结果语义的场景（通道发送、`abort()`、写入 `String`），
  其余必须记日志或传播 —— 细则见 `CONTRIBUTING.md`「Panics, locks and swallowed errors」。

## Rules

### 文档分工（写文档前先看这里）

- `AGENTS.md`（本文件）：只记录**约束 / 入口 / 基线**——Setup、Commands、Architecture、
  Conventions、Rules、当前版本与协议版本。**不记录逐版本变更、逐版本特性或阶段进度**；
  架构条目只描述「现在是什么」，不标注「哪个版本引入的」。
- `README.md`：只保留项目简介、高层特性概览、结构、快速上手、协议摘要与开发命令；
  **不记录详细版本变更与逐版本特性**。版本行只同步「当前版本 + 指向 `docs/changelog/` 的链接」。
- 版本变更记录 → `docs/changelog/`（每个次版本一个文件 `v0.<minor>.md`，补丁号作为同文件
  小节；Keep a Changelog 风格 `Added`/`Changed`/`Fixed`/`Removed`；新次版本另建文件并更新
  `docs/changelog/README.md` 索引）。全部特性合并于 `0.1.0`（`v0.1.md`），不保留历史版本演进。
- 用户可见特性 → `docs/guide/`（按主题，去版本号叙事）。
- 当前正在开发功能的进度 / 遗留问题 / 待实施 / 下一步 → `.agents/summary/`（见下）。
- 协议与设计细节 → `docs/specs/`。
- `docs/superpowers/`（design spec + plan）：**本地工作文档，不入版本控制**——不提交、不随 PR，
  由 `.gitignore` 忽略；设计与计划意图经 `.agents/summary/`、`docs/changelog/` 与 PR 描述传达。
- `docs/` 下其余文档（`docs/specs/`、`docs/changelog/`、`docs/guide/` 等）是跟踪交付物 —— 必须提交。

### 版本与提交

- Commit messages in English. `docs/superpowers/` 本地文档用中文（不入库）。
- **版本号不随特性/修复自动递增**：默认沿用当前版本号（现为 `0.1.0`），新特性与 bug 修复都不改动它。
  仅当用户**主动要求**变更版本号时才更新；届时须同步 `Cargo.toml`、`agentj-ui/packages/sdk/src/config.ts`
  （`VERSION`）、desktop 四件套（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`）、
  本文件与 `README.md` 的版本基线，以及 `scripts/check-version.sh` 的断言，并以
  `./scripts/check-version.sh` 通过为验收。
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
- 更新**本地** `docs/superpowers/specs` 目标 spec 文档的 TODO 列表（本地文件，不入库、不提交）。
- `docs/superpowers/specs/` 下所有图表必须使用 UML 或 Mermaid。
- 每阶段后清理临时文件与 Docker 容器。
- 命令挂起 >120s 时取消并重试。

### 协作

- PR creation: create PR on `origin` only and share link — user handles merge。
  **Never open a GitCode MR for the same branch**（见下方 Remote & Merge Policy）。
- **Feature 分支必须推送两个远端**：创建分支后 `git push -u origin <branch>` **且**
  `git push gitcode <branch>:<branch>`（同一 SHA，无害）。漏推 `gitcode` 会让镜像缺分支，
  下次同步 `main` 前无法核对 tree——**每个 feature 阶段收尾时必做**，不得遗漏。

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
- **feature 分支必须同时推送两个远端**（`origin` 与 `gitcode`，同一 SHA，无害）：
  创建分支后 `git push -u origin <branch>` **且** `git push gitcode <branch>:<branch>`。
  漏推 `gitcode` 会让镜像缺少该分支（下次同步 `main` 前无法核对 tree）；
  这是每个 feature 阶段的强制收尾步骤。**合并仍只能在一个远端（`origin`）进行**。
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
