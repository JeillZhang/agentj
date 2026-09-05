# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；
CLI、TUI、Web、Desktop 均通过 `agentj-ui` 协议消费者接入，任意语言可开发前端。

> 当前版本：**v0.11.0**——agentj-ui 三端统一前端
> （Web / TUI / Desktop + SDK，16 crate + npm workspaces monorepo，
> 共享 `packages/sdk` + `packages/protocol`，后端零改造）

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
        config.ts          # PROTOCOL_VERSION="0.11.0" + resolveWsUrl
        methods.ts         # METHODS 常量（由 methods.json 生成）
        transports/
          websocket.ts     # WebSocketTransport（浏览器）
          tcp.ts           # TcpTransport（Node 直连预留）
  apps/
    web/                   # Web Dashboard（Vite + React + TS，消费 @agentj/sdk）
    tui/                   # TUI（Bun + @opentui/react，全功能）
    desktop/               # Tauri 壳（套 web/dist，输出安装包）
models/                    # models/opencode-go.json
schemas/                   # 33 JSON Schema（机器事实来源）
protocol/                  # methods.json 方法注册表
docs/superpowers/          # 设计文档 + 实现计划（中文）
```

> 前端不拆独立仓：`agentj-ui` 保留在 `crates/` 同级 monorepo 内，
> 与 Rust workspace 共享版本 `0.11.0`（单一 `PROTOCOL_VERSION` 来源 `sdk/config.ts`）。

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
npm install --prefix agentj-ui
npm run dev --workspace=@agentj/web        # Vite dev，VITE_AGENTJ_WS_URL 可配
npm run build --workspace=@agentj/web      # tsc -b && vite build
npm test --workspace=@agentj/web

# TUI 前端（Bun + OpenTUI）
cd agentj-ui/apps/tui && bun install
bun run dev                                # 直连 dashboard --port / serve --port
bun test                                   # @opentui/react testRender + mock WS e2e

# Desktop 安装包（Tauri，需 Rust 工具链）
npm run build --workspace=@agentj/web      # 先产出 frontendDist
npm run build --workspace=@agentj/desktop  # tauri build → target/release/bundle/
./agentj-ui/apps/desktop/check-artifacts.sh linux   # 校验 deb + AppImage

# SDK / Protocol
npm run generate --workspace=@agentj/protocol  # schemas → types.d.ts
npm run build --workspace=@agentj/protocol     # tsc --noEmit
npm run generate --workspace=@agentj/sdk       # methods.json → METHODS
npm test --workspace=@agentj/sdk
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
./scripts/check-version.sh        # 版本一致性：Cargo.toml + sdk/config.ts + desktop 均为 0.11.0

# 前端 monorepo（npm workspaces）
npm install                           # 根安装（agentj-ui workspaces）
npm run generate --workspace=@agentj/protocol   # 生成 protocol types
npm test --workspace=@agentj/sdk                # SDK 单测
npm test --workspace=@agentj/web                # Web 单测 + 构建
npm run build --workspace=@agentj/web
bun test --prefix agentj-ui/apps/tui           # TUI 单测（含 mock WS e2e）
./agentj-ui/apps/desktop/check-artifacts.sh linux  # Desktop 产物校验
```
