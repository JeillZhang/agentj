# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；
CLI、TUI、Web、Desktop 均通过 `agentj-ui` 协议消费者接入，任意语言可开发前端。

> 当前版本：**v0.14.5**——agentj-ui 三栏工作台（Sessions/Files 侧栏 + 会话中栏 + 运行详情右栏 + composer 模型/Agent/熵变选择器 + 模型提问卡片 + demo 模式）
> （v0.14.2 验收修复 3 bugs：短视口 composer 裁剪 / 右分栏折叠态锁定 / 提问卡折叠态摘要；
> 评审反馈修复：移除三栏间 standalone dashboard 提示行；
> v0.14.3 评审反馈修复 3 项：右栏始终满高 / 任务与文件变更标题点击折叠 / `.sec-head` 标题风格统一；
> v0.14.4 评审反馈修复 1 项：顶栏连接/继续弹窗 portal 至 body 窗口居中 + 遮罩点击关闭；
> v0.14.5 评审反馈修复 1 项：抽共享 ModalDialog，添加项目弹窗与连接设置/继续会话统一）
> （Web / TUI / Desktop + SDK + tokens，16 crate + npm workspaces monorepo，
> 共享 `packages/sdk` + `packages/protocol` + `packages/tokens`，后端零改造，frontend stays in-repo）

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 16 方法
  （lifecycle 4 / session 7 含 subscribe/unsubscribe / kernel/status 1 /
  `$/cancelRequest` `$/progress` 通知 2 + `permission/*` 2）
- [x] **权限审批系统**：`$/permissionRequest` + `permission/respond`
  + 60s 超时自动 Deny + allow_always 会话级 + 取消即拒绝
- [x] **JSON Schema**：33 Schema（schemars 生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节）
- [x] **跨语言验证**：TS 绑定 `agentj-ui/packages/protocol`（类型生成 + tsc）
- [x] **内核与模块注册表**：Kernel 混合总线 + `Kernel::modules()`
  + `ToolCallReply` + `AsyncKernelService` + `ServiceRegistry`
- [x] **Agent 循环（流式）**：`TurnRunner` 总线化编排 + `AgentLoopModule`
  + `ToolExecutorModule` + `SubAgentService`
- [x] **内置工具**：bash / glob / grep / read / write + ToolExecutor
- [x] **MCP 桥**：子进程 MCP 客户端 + McpConnectionManager 长驻连接
- [x] **JSON-RPC IPC**：stdio + TCP 多客户端 + WebSocket 网关
  + per-session 路由 + 权限属主门控
- [x] **CLI run/serve**：`agentj run` / `serve [stdio]` / `serve --port`
  + `dashboard --port`（常驻多客户端，`/ws` 同端口共存）
- [x] **模型提供方**：DeepSeek + opencode-go + Ollama + Mock
  （三线格式，ProviderFactory 按 kind 实例化）
- [x] **事件总线**：`EventBus` 订阅/发布 + 重入防护
  （EventGuard thread_local depth/stack + MAX_EVENT_DEPTH=16）
- [x] **子代理 / Skills / 插件 / 会话持久化 / 守护内核 / 用量统计 / License**：
  均已落地（见 AGENTS.md 架构节）
- [x] **WebSocket 网关**：`ws://127.0.0.1:<port>/ws`（axum + hyper-util
  + `?token=` 403 + `initialize.token` -32806 双通道，WS 不参与全局关闭）
- [x] **agentj-ui SDK**：`packages/sdk`（RpcClient + ResponseRouter
  + WebSocketTransport + TcpTransport + resolveWsUrl + METHODS + PROTOCOL_VERSION）
- [x] **agentj-ui Web**：`apps/web`（Vite + React 19，对齐项：常量化方法名、
  `?token=` 透传、`protocol_version` 同源、会话收藏、composer、
  `$/progress` 流式、审批对话框、usage 结算）
- [x] **agentj-ui TUI**：`apps/tui`（Bun + @opentui/react，全功能：
  连接管理、会话新建/切换/继续、composer 多行/@补全//palette、
  流式渲染、审批模态、模型只读展示、主题切换、usage 结算、优雅退出）
- [x] **agentj-ui Desktop**：`apps/desktop`（Tauri v2 壳，套 `apps/web` 产物，
  输出 deb / AppImage / dmg / msi 四件套，check-artifacts.sh 校验）
- [x] **Visual 2.1 refine（v0.12.3）**：`packages/tokens` 扩展至 19 keys（shadowSoft/shadowRaised/radiusXl/textCaption + spacing/font/z/motion/icon vars）
  + Web 精细化：App Shell 居中列 + 抽屉 scrim / 悬浮胶囊 Composer（22px + shadow + sticky）/ 卡片化 ConnectionBar 与 SessionPanel / 气泡与 timeline 工具卡 / Trajectory timeline / Hero 空状态与 dashed trigger / Dialog 固定遮罩 + lucide 替换 emoji
- [x] **Visual 2.2 Opencode redesign（v0.13.0）**：UnifiedSidebar 256px（Threads + Project 文件夹 + 底栏）+ local Project（localStorage）+ Composer 增强（Project picker + model chip + status popover + @/slash）+ 密度收紧（conversation gap 8px）+ TUI 32char 侧栏 + a11y/CLS/虚拟化 + Desktop 同步
- [x] **三栏工作台（v0.14.5）**：`WorkspaceShell` 三栏布局（Sessions/Files 左栏 + 会话中栏 + 运行详情右栏 `TodoPanel`/`ChangeList` unified diff）+ `SplitPane` 拖拽/键盘/双击与宽度持久化 + `<1024` 右抽屉 + 右栏满高 + 任务/文件变更标题点击折叠 + composer 模型/Agent/熵变选择器（互斥 + 持久化）+ `AskQuestionCard` 模型提问卡片 + `TopbarMenu` 顶栏化（含 demo 开关 + `demoData` 驱动 TODO/文件树/变更/提问卡）+ tokens 24 keys + `LAYOUT` 常量 + 共享 `ModalDialog`（连接设置/继续会话/添加项目统一）

## Project Structure

```
crates/                    # 16 个 workspace crate（统一目录）
  agentj-protocol/         # IPC 协议契约（wire 类型 + schema 生成）
  agentj-core/             # 内核调度 + 事件总线（含重入防护）+ TurnRunner
  agentj-config/           # TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）
  agentj-session/          # 会话生命周期（FileBackedStore + 过期清理）
  agentj-model/            # 模型提供方（DeepSeek/opencode-go/Ollama/Mock）
  agentj-tools/            # 内置工具（bash/glob/grep/read/write）
  agentj-permission/       # 权限系统
  agentj-context/          # 上下文管线
  agentj-stats/            # 用量统计 UsageCollector
  agentj-license/          # Ed25519 许可证校验
  agentj-mcp/              # MCP 客户端 + McpConnectionManager
  agentj-skills/           # skills 加载器
  agentj-plugin/           # 插件系统 + libloading
  agentj-ipc/              # JSON-RPC IPC（stdio/TCP/WS + 连接分类器 + WsConnection）
  agentj-cli/              # agentj 二进制（run/serve/dashboard）
  agentj-plugin-fixture/   # 插件 cdylib 测试夹具（dev-only）
agentj-ui/                 # 前端 monorepo（npm workspaces，frontend stays in-repo）
  package.json             # 根 workspaces: ["apps/*", "packages/*"]
  packages/
    protocol/              # 协议类型生成（schemas → types.d.ts，复用 drift 门禁）
    sdk/                   # 共享 SDK（client/router/transports/config/methods）
      src/
        client.ts          # RpcClient
        router.ts          # ResponseRouter + RpcTransport
        config.ts          # VERSION="0.14.5" + PROTOCOL_VERSION="0.12.1" + resolveWsUrl
        methods.ts         # METHODS 常量（由 methods.json 生成）
        transports/
          websocket.ts     # WebSocketTransport（浏览器）
          tcp.ts           # TcpTransport（Node 直连预留）
    tokens/                # 共享 tokens（palette.ts 24 keys + LAYOUT 常量 + tokens.css/json/md 生成）
  apps/
    web/                   # Web 三栏工作台（Vite + React + TS，WorkspaceShell/RightPanel/ComposerOptions/AskQuestionCard；消费 @agentj/sdk + @agentj/tokens + lucide-react；components/*.css 模块化）
    tui/                   # TUI（Bun + @opentui/react，全功能）
    desktop/               # Tauri 壳（套 web/dist，输出安装包）
models/                    # models/opencode-go.json
schemas/                   # 33 JSON Schema（机器事实来源）
protocol/                  # methods.json 方法注册表
docs/superpowers/          # 设计文档 + 实现计划（中文）
```

> 前端不拆独立仓：`agentj-ui` 保留在 `crates/` 同级 monorepo 内，
> 与 Rust workspace 共享版本 `0.14.5`（`VERSION` + 单一 `PROTOCOL_VERSION 0.12.1` 来源 `sdk/config.ts`）。

## Usage

```bash
# 后端常驻内核（Web/TUI/Desktop 均连此端点）
agentj serve --port 4096
agentj serve --port 4096 --token SECRET   # 鉴权：?token= 或 initialize.token
agentj dashboard --port 4096              # 同款协议入口（无 token 最简）

# 单轮运行 / 附着运行
agentj run "explain this codebase"
agentj run --attach tcp://127.0.0.1:4096 "hi"

# Web 前端（开发）
npm --prefix agentj-ui install
npm --prefix agentj-ui run dev --workspace=@agentj/web        # Vite dev，VITE_AGENTJ_WS_URL 可配
npm --prefix agentj-ui run build --workspace=@agentj/web      # tsc -b && vite build
npm --prefix agentj-ui test --workspace=@agentj/web

# TUI 前端（Bun + OpenTUI）
bun --cwd agentj-ui/apps/tui install
bun --cwd agentj-ui/apps/tui run dev                                # 直连 dashboard --port / serve --port
bun --cwd agentj-ui/apps/tui test                                   # @opentui/react testRender + mock WS e2e

# Desktop 安装包（Tauri，需 Rust 工具链）
npm --prefix agentj-ui run build --workspace=@agentj/web      # 先产出 frontendDist
npm --prefix agentj-ui run build --workspace=@agentj/desktop  # tauri build → target/release/bundle/
./agentj-ui/apps/desktop/check-artifacts.sh linux   # 校验 deb + AppImage

# SDK / Protocol
npm --prefix agentj-ui run generate --workspace=@agentj/protocol  # schemas → types.d.ts
npm --prefix agentj-ui run build --workspace=@agentj/protocol     # tsc --noEmit
npm --prefix agentj-ui run generate --workspace=@agentj/sdk       # methods.json → METHODS
npm --prefix agentj-ui test --workspace=@agentj/sdk
```

配置示例 `config.toml`：

```toml
[model]
provider = "deepseek"
default_model = "deepseek-chat"

[[model.providers]]
name = "deepseek-chat"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
kind = "openai-compat"   # openai-compat | responses | anthropic | opencode-go

[permission]
[permission.tools.bash]
mode = "ask"

[[permission.tools.bash.command_rules]]
prefix = "rm -rf"
effect = "deny"

[session]
dir = "/data/agentj/sessions"

[license]
key = "base64(payload).base64(sig)"

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
env = { FOO = "bar" }
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`
- Schema：`schemas/*.json`（33 个）
- 方法注册表：`protocol/methods.json`（16 方法）
- 方法面：`initialize` / `initialized` / `shutdown` / `exit` /
  `session/new` / `session/run` / `session/status` / `session/archive` /
  `session/delete` / `session/subscribe` / `session/unsubscribe` /
  `kernel/status` / `$/cancelRequest` / `$/progress` /
  `permission/respond` / `$/permissionRequest`
- 传输：stdio（单客户端） + TCP（NDJSON 多客户端） + WebSocket `/ws`
  （同端口分类器，`?token=` + `initialize.token` 双通道鉴权）

## Development

```bash
cargo test --workspace            # 全量 Rust 测试（含 e2e_cli/e2e_permission/e2e_serve/e2e_ws）
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p agentj-model       # 模型流式基准
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cargo test -p agentj-protocol -- drift          # 漂移门禁（33 schemas 无漂移）
./scripts/check-version.sh        # 版本一致性：Cargo.toml + sdk/config.ts + desktop 均为 0.14.5（PROTOCOL_VERSION 0.12.1）

# 前端 monorepo（npm workspaces，根为 agentj-ui/）
npm --prefix agentj-ui install                           # 安装（agentj-ui workspaces）
npm --prefix agentj-ui run generate --workspace=@agentj/protocol   # 生成 protocol types
npm --prefix agentj-ui test --workspace=@agentj/sdk                # SDK 单测
npm --prefix agentj-ui test --workspace=@agentj/web                # Web 单测 + 构建
npm --prefix agentj-ui run build --workspace=@agentj/web
bun --cwd agentj-ui/apps/tui test           # TUI 单测（含 mock WS e2e）
./agentj-ui/apps/desktop/check-artifacts.sh linux  # Desktop 产物校验
```
