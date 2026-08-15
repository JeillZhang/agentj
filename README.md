# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。内核通过统一的 JSON-RPC 2.0 协议（stdio，未来 TCP / WebSocket）对外暴露能力；CLI、TUI、Web Dashboard、IDE 插件均可作为协议消费者，**任意语言**可开发前端连接内核。

> 当前版本：**v0.1.0**——IPC 协议契约（12 方法）+ 10 crate 内核已落地：Agent 循环（流式）、内置工具、MCP 桥、JSON-RPC stdio IPC、CLI run/serve、模型提供方（DeepSeek + opencode-go）；守护内核（常驻 API 服务）设计已批准，待 v0.3。

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 12 个方法（lifecycle 4 / session 5 / kernel/status 1 / `$/cancelRequest` `$/progress` 通知 2；`session/subscribe` / `session/unsubscribe` / `permission/respond` / `$/permissionRequest` 订阅·审批系列为 v0.2 计划）
- [x] **JSON Schema**：31 个机器可读 Schema（`schemars` 从 Rust 类型生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（语言无关，method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节，v0.2.0）
- [x] **跨语言验证**：TypeScript 绑定测试（`bindings/ts/`，json-schema-to-typescript + tsc）
- [x] **内核与模块注册表**：Kernel 调度 + ModelProvider / SessionStore / ToolRegistry / ContextPipeline / ConfigProvider trait 注册表
- [x] **Agent 循环（流式）**：AgentLoop `run_turn_stream` 事件流 + `run_turn` 聚合
- [x] **内置工具**：bash / glob / grep / read / write + ToolExecutor
- [x] **MCP 桥**：子进程 MCP 客户端 + 工具发现注册
- [x] **JSON-RPC stdio IPC**：12 方法服务循环 + 优雅退出
- [x] **CLI run/serve**：`agentj run "prompt"` / `agentj serve`
- [x] **模型提供方**：DeepSeek + opencode-go 统一订阅（Chat Completions / Responses / Anthropic 三线格式，ProviderFactory 按 `kind` 实例化）
- [ ] **守护内核**：统一 Connection 传输抽象（stdio/TCP/Unix socket）+ 常驻 API 服务（`agentj serve --port` 多客户端共享、可接续任务）——设计已批准，能力 v0.3
- [ ] v0.2 生态：Hook System / Skills / 插件 / MCP 长驻连接
- [ ] v0.3 商业：Web Dashboard / 用量统计 / License

## Project Structure

```
agentj-protocol/     # IPC 协议契约 crate（wire 类型 + schema 生成，零内核耦合）
agentj-core/         # 内核：Kernel 调度 + 模块 trait 注册表 + AgentLoop 流式 API
agentj-config/       # TOML 配置（[model] provider/providers[]/kind/default_model）
agentj-session/      # 会话生命周期（状态机 SessionStatus + 快照恢复）
agentj-model/        # 模型提供方：三线格式 + OpenCodeGo 门面 + ProviderFactory + Mock
agentj-tools/        # 内置工具（bash/glob/grep/read/write）+ ToolExecutor
agentj-context/      # 上下文管线（纯消息构造器）
agentj-mcp/          # MCP 客户端（子进程连接 + 工具发现注册）
agentj-ipc/          # JSON-RPC 2.0 stdio IPC 服务
agentj-cli/          # agentj 二进制（run / serve）
models/              # 模型目录 models/opencode-go.json（opencode-go 路由事实来源）
schemas/             # 31 个 JSON Schema（schemars 生成，机器事实来源）
protocol/            # 方法注册表 methods.json
docs/specs/          # 人类可读协议规范 ipc-protocol.md
bindings/ts/         # TypeScript 绑定测试
docs/superpowers/    # 设计文档 + 实现计划（中文）
```

## Usage

```bash
agentj run "explain this codebase"   # 单轮运行（JSON-RPC 内核 + AgentLoop）
agentj serve                         # 常驻 JSON-RPC stdio 服务（EOF/信号优雅退出）
```

配置示例 `config.toml`（`kind` 区分 wire-format，`default_model` 供 AgentLoop 使用）：

```toml
[model]
provider = "deepseek"
default_model = "deepseek-chat"

[[model.providers]]
name = "deepseek-chat"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
kind = "openai-compat"   # openai-compat | responses | anthropic | opencode-go
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`（v0.2.0）
- Schema：`schemas/*.json`
- 方法注册表：`protocol/methods.json`
- 方法面（12）：`initialize` / `initialized` / `shutdown` / `exit` / `session/new` / `session/run` / `session/status` / `session/archive` / `session/delete` / `kernel/status` / `$/cancelRequest` / `$/progress`（`session/subscribe` / `session/unsubscribe` / `permission/respond` / `$/permissionRequest` 为 v0.2 计划）

## Development

```bash
cargo test --workspace            # 全量测试（118 个：protocol 31 单元 + 7 契约 + 3 注册表 + 内核 77）
cargo clippy --workspace -- -D warnings
cargo bench -p agentj-model       # 模型流式基准（criterion）
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cd bindings/ts && npm install && npm run generate && npm run build  # TS 绑定测试
```
