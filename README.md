# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；
CLI、TUI、Web、Desktop 均作为协议消费者接入，任意语言可开发前端。

> 当前版本：**v0.25.2**
> · 变更记录：[`docs/changelog/`](docs/changelog/)
> · 用户指南：[`docs/guide/`](docs/guide/)
> · 协议规范：[`docs/specs/ipc-protocol.md`](docs/specs/ipc-protocol.md)

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [用户指南](docs/guide/) | 各端功能与使用方式（CLI / Web / TUI / Desktop / 配置 / HTTP） |
| [变更记录](docs/changelog/) | 逐版本变更与逐版本特性记录 |
| [协议规范](docs/specs/ipc-protocol.md) | 人类可读 IPC 协议；机器事实见 `schemas/*.json` + `protocol/methods.json` |
| [AGENTS.md](AGENTS.md) | 内部架构、约定与开发规则 |

## Features

**内核与协议**

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 21 方法
  （lifecycle / session / kernel / 通知 / 权限 / 提问 / 文件列举）
- [x] **JSON Schema**：52 Schema（schemars 生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json` + 跨语言类型绑定 `agentj-ui/packages/protocol`
- [x] **内核与模块注册表**：Kernel 混合总线 + `ServiceRegistry`（同步/异步）
- [x] **事件总线**：`EventBus` 订阅/发布 + 重入防护（`MAX_EVENT_DEPTH=16`）
- [x] **Agent 循环（流式）**：`TurnRunner` 总线化编排 + `AgentLoopModule` +
  `ToolExecutorModule` + `SubAgentService`
- [x] **内置工具**：bash / glob / grep / read / write + todo
- [x] **MCP 桥**：子进程 MCP 客户端 + `McpConnectionManager` 长驻连接
- [x] **模型提供方**：DeepSeek + opencode-go + Ollama + Mock（三线 wire-format）
- [x] **opencode-go 订阅**：`agentj auth` 凭据落盘 + live/overlay 动态目录 +
  合规请求头（`agentj/<ver>` UA、`x-opencode-session`）+ 模型可见性
  （见 [订阅指南](docs/guide/subscription.md)）
- [x] **权限审批系统**：`$/permissionRequest` + `permission/respond` + 60s 超时自动
  Deny + `allow_always` 会话级
- [x] **提问通道**：`$/askQuestion` + `question/respond` owner 定向往返
- [x] **会话**：生命周期 + `project_root` + 磁盘持久化 + 回放重建

**服务与接入**

- [x] **JSON-RPC IPC**：stdio + TCP 多客户端 + WebSocket 网关（同端口复用）
- [x] **CLI**：`agentj run` / `serve [stdio|--port]` / `dashboard`
- [x] **守护内核**：常驻多客户端 + `run --attach`
- [x] **内核托管 Web UI**：同端口静态托管 + 混合鉴权登录门 + WS cookie 通道
- [x] **可观测性**：`GET /version` / `GET /metrics`（Prometheus）+ 登录限流 + 资产完整性校验
- [x] **子代理 / Skills 加载器 / 插件系统 / 用量统计 / License（Ed25519）**

**前端（`agentj-ui` monorepo）**

- [x] **共享 SDK** `packages/sdk`：RpcClient + ResponseRouter + WebSocket/TCP 传输
- [x] **共享 tokens** `packages/tokens`：语义色 + LAYOUT 常量（深浅双主题）
- [x] **Web** `apps/web`：三栏工作台（会话 / 对话 / 运行详情）+ composer + 代码卡片 +
  显示设置 + 文件树 + 设置
- [x] **TUI** `apps/tui`：Bun + `@opentui/react` 全功能 + Files 视图与搜索
- [x] **Desktop** `apps/desktop`：Tauri v2 壳（deb / AppImage / dmg / msi）

> 各项功能的详细用法见 [用户指南](docs/guide/)；历史变更见 [变更记录](docs/changelog/)。

## Project Structure

```
crates/                    # 18 个 workspace crate（统一目录）
  agentj-protocol/         # IPC 协议契约（wire 类型 + schema 生成）
  agentj-core/             # 内核调度 + 事件总线（重入防护）+ TurnRunner + TurnArtifact（diff）
  agentj-config/           # TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）
  agentj-auth/             # 订阅凭据（auth.json 0600 原子写 + flag>env>file 优先级）
  agentj-session/          # 会话生命周期（FileBackedStore + 过期清理 + project_root）
  agentj-question/         # 提问通道（QuestionModule + AskTool）
  agentj-model/            # 模型提供方（DeepSeek/opencode-go/Ollama/Mock）
  agentj-tools/            # 内置工具（bash/glob/grep/read/write + todo）
  agentj-permission/       # 权限系统
  agentj-context/          # 上下文管线
  agentj-stats/            # 用量统计 UsageCollector
  agentj-license/          # Ed25519 许可证校验
  agentj-mcp/              # MCP 客户端 + McpConnectionManager
  agentj-skills/           # skills 加载器
  agentj-plugin/           # 插件系统 + libloading
  agentj-ipc/              # JSON-RPC IPC（stdio/TCP/WS + 连接分类器 + file/list + web 静态托管/登录门/指标）
  agentj-cli/              # agentj 二进制（run/serve/dashboard/auth）
  agentj-plugin-fixture/   # 插件 cdylib 测试夹具（dev-only）
agentj-ui/                 # 前端 monorepo（npm workspaces，frontend stays in-repo）
  packages/
    protocol/              # 协议类型生成（schemas → types.d.ts）
    sdk/                   # 共享 SDK（client/router/transports/config/methods）
    tokens/                # 共享 tokens（palette + LAYOUT + 生成产物）
  apps/
    web/                   # Web 三栏工作台
    tui/                   # TUI（Bun + @opentui/react）
    desktop/               # Tauri 壳（套 web/dist）
models/                    # models/opencode-go.json
schemas/                   # 52 JSON Schema（机器事实来源）
protocol/                  # methods.json 方法注册表
docs/                      # 用户指南（guide/）+ 变更记录（changelog/）+ 规范（specs/）+ 设计（superpowers/）
```

> 前端不拆独立仓：`agentj-ui` 保留在 `crates/` 同级 monorepo 内，与 Rust workspace
> 共享版本（`VERSION` 与 `PROTOCOL_VERSION` 单一真源 `packages/sdk/src/config.ts`）。

## Quick Start

```bash
# 常驻内核（Web/TUI/Desktop 均连此端点）
agentj serve --port 4096
agentj serve --port 4096 --host 0.0.0.0 --token SECRET   # 局域网/远程（建议配 --token）
agentj dashboard --port 4096                              # 同款协议入口（无 token 最简）

# 内核托管 Web UI（浏览器打开 http://127.0.0.1:4096/）
agentj serve --port 4096 --web-dist path/to/agentj-ui/apps/web/dist
# 四级发现：--web-dist → AGENTJ_WEB_DIST → cwd agentj-ui/apps/web/dist → exe 相对

# 单轮 / 附着运行
agentj run "explain this codebase"
agentj run --attach tcp://127.0.0.1:4096 "hi"

# 可观测性
curl http://127.0.0.1:4096/version   # 版本事实 + 限流聚合 + 完整性摘要
curl http://127.0.0.1:4096/metrics   # Prometheus 文本（鉴权门控）
RUST_LOG=debug agentj serve --port 4096

# Web 前端（开发）
npm --prefix agentj-ui install
npm --prefix agentj-ui run dev --workspace=@agentj/web
npm --prefix agentj-ui run build --workspace=@agentj/web

# TUI 前端
bun --cwd agentj-ui/apps/tui install
bun --cwd agentj-ui/apps/tui run dev

# Desktop 安装包（Tauri）
npm --prefix agentj-ui run build --workspace=@agentj/web
npm --prefix agentj-ui run build --workspace=@agentj/desktop
./agentj-ui/apps/desktop/check-artifacts.sh linux
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

更多用法见 [用户指南](docs/guide/)（含 CLI 参数、Web / TUI 功能、HTTP 与鉴权、配置说明）。

## Protocol

- 规范：`docs/specs/ipc-protocol.md`
- Schema：`schemas/*.json`（52 个）
- 方法注册表：`protocol/methods.json`（21 方法）
- 方法面：`initialize` / `initialized` / `shutdown` / `exit` /
  `session/new` / `session/run` / `session/status` / `session/archive` /
  `session/delete` / `session/subscribe` / `session/unsubscribe` /
  `session/list` / `session/history` / `kernel/status` /
  `$/cancelRequest` / `$/progress` / `$/askQuestion` / `$/permissionRequest` /
  `permission/respond` / `file/list` / `question/respond`
- 传输：stdio（单客户端）+ TCP（NDJSON 多客户端）+ WebSocket `/ws`
  （同端口分类器；`?token=` + `initialize.token` + 登录 cookie 三通道鉴权）
- HTTP 面（内核托管）：`/`（Web UI）+ `/assets` + `/login` / `/logout` +
  `GET /version` + `GET /metrics`（详见 [HTTP 面与鉴权](docs/guide/http-and-auth.md)）
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；未知能力/枚举值容忍）

## Development

```bash
cargo test --workspace            # 全量 Rust 测试
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p agentj-model       # 模型流式基准
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cargo test -p agentj-protocol -- drift          # 漂移门禁
./scripts/check-version.sh        # 版本一致性（Cargo + sdk + desktop）

# 前端 monorepo（npm workspaces，根为 agentj-ui/）
npm --prefix agentj-ui install
npm --prefix agentj-ui run generate --workspace=@agentj/protocol   # schemas → types.d.ts
npm --prefix agentj-ui run build --workspace=@agentj/protocol      # tsc --noEmit
npm --prefix agentj-ui test --workspace=@agentj/sdk
npm --prefix agentj-ui test --workspace=@agentj/web
npm --prefix agentj-ui run build --workspace=@agentj/web
bun --cwd agentj-ui/apps/tui test
node agentj-ui/apps/desktop/check-desktop-ui.mjs
```

> 注意：Vite dev server 不监听 root 之外的 `packages/sdk` 源（alias 直连源码）；
> 改动 SDK 后须重启 dev server，否则运行陈旧的 `METHODS`。
