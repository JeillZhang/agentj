# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。内核通过统一的 JSON-RPC 2.0 协议（stdio，未来 TCP / WebSocket）对外暴露能力；CLI、TUI、Web Dashboard、IDE 插件均可作为协议消费者，**任意语言**可开发前端连接内核。

> 当前版本：**v0.3.0**——生态扩展落地（14 crate）：事件总线 + 子代理调度 + skills 加载 + 插件加载（libloading）+ MCP 长驻连接 + 会话过期 + 磁盘持久化 + Ollama provider；v0.2 全保留：IPC 协议契约（14 方法）+ 权限审批系统 + JSON-RPC stdio IPC + CLI run/serve；守护内核（常驻 API 服务）设计已批准，能力 v0.4。

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 14 个方法（lifecycle 4 / session 5 / kernel/status 1 / `$/cancelRequest` `$/progress` 通知 2 + `permission/respond` `$/permissionRequest` 2；`session/subscribe` / `session/unsubscribe` 为守护内核 v0.4 计划）
- [x] **权限审批系统**：`$/permissionRequest` 通知（S→C）+ `permission/respond` 请求（C→S）+ 审批流（request_id 解耦、60s 超时自动 Deny、allow_always 会话级内存态、取消即拒绝）
- [x] **JSON Schema**：31 个机器可读 Schema（`schemars` 从 Rust 类型生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（语言无关，method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节，v0.2.0）
- [x] **跨语言验证**：TypeScript 绑定测试（`bindings/ts/`，json-schema-to-typescript + tsc）
- [x] **内核与模块注册表**：Kernel 调度 + ModelProvider / SessionStore / ToolRegistry / ContextPipeline / ConfigProvider trait 注册表
- [x] **Agent 循环（流式）**：AgentLoop `run_turn_stream` 事件流 + `run_turn` 聚合
- [x] **内置工具**：bash / glob / grep / read / write + ToolExecutor
- [x] **MCP 桥**：子进程 MCP 客户端 + 工具发现注册
- [x] **JSON-RPC stdio IPC**：14 方法服务循环 + 优雅退出
- [x] **CLI run/serve**：`agentj run "prompt"` / `agentj serve`
- [x] **模型提供方**：DeepSeek + opencode-go 统一订阅（Chat Completions / Responses / Anthropic 三线格式，ProviderFactory 按 `kind` 实例化）+ **Ollama provider**
- [x] **事件总线**：`EventBus` 订阅/发布，全生命周期事件（模块 / 会话 / MCP / 权限）
- [x] **子代理调度器**：Kernel 派发 + 任务状态跟踪
- [x] **Skills 加载器**：`.aj/.agents/.claude/skills` 优先级合并（同名高优先胜出）
- [x] **插件系统**：manifest 解析 + 生命周期状态机 + 依赖图 + libloading 动态加载
- [x] **MCP 长驻连接**：`McpConnectionManager` 引用计数共享 + 空闲回收 + 指数退避重连 + ghost-tool 注销
- [x] **会话过期 + 磁盘持久化**：`spawn_expiry_loop` 过期清理 + FileBackedStore 快照落盘
- [ ] **守护内核**：统一 Connection 传输抽象（stdio/TCP/Unix socket）+ 常驻 API 服务（`agentj serve --port` 多客户端共享、可接续任务）——设计已批准，能力 v0.4
- [ ] v0.4 商业：Web Dashboard / 用量统计 / License

## Project Structure

```
crates/              # 14 个 workspace crate（统一目录）
  agentj-protocol/   # IPC 协议契约 crate（wire 类型 + schema 生成，零内核耦合）
  agentj-core/       # 内核：Kernel 调度 + 模块 trait 注册表 + AgentLoop 流式 API + 事件总线 + 子代理调度
  agentj-config/     # TOML 配置（[model] provider/providers[]/kind/default_model）
  agentj-session/    # 会话生命周期（状态机 + 快照恢复 + 过期清理 + FileBackedStore 磁盘持久化）
  agentj-model/      # 模型提供方：三线格式 + OpenCodeGo 门面 + ProviderFactory + Mock + Ollama
  agentj-tools/      # 内置工具（bash/glob/grep/read/write）+ ToolExecutor
  agentj-permission/ # 权限系统：决策引擎 + 策略模型（[permission] 配置）+ PermissionModule
  agentj-context/    # 上下文管线（纯消息构造器）
  agentj-mcp/        # MCP 客户端（子进程连接 + 工具发现注册 + McpConnectionManager 长驻连接）
  agentj-skills/     # skills 加载器（.aj/.agents/.claude/skills 优先级合并）
  agentj-plugin/     # 插件系统（manifest + 生命周期状态机 + 依赖图 + libloading 动态加载）
  agentj-ipc/        # JSON-RPC 2.0 stdio IPC 服务
  agentj-cli/        # agentj 二进制（run / serve）
  agentj-plugin-fixture/  # 插件 cdylib 测试夹具（dev-only）
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

[permission]                       # 权限审批（可选；缺省为内置默认：bash/write=Ask，read/glob/grep=Allow）
[permission.tools.bash]
mode = "ask"                       # allow | ask | deny

[[permission.tools.bash.command_rules]]
prefix = "rm -rf"   # 字面前缀匹配（非正则）
effect = "deny"
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`（v0.2.0）
- Schema：`schemas/*.json`
- 方法注册表：`protocol/methods.json`
- 方法面（14）：`initialize` / `initialized` / `shutdown` / `exit` / `session/new` / `session/run` / `session/status` / `session/archive` / `session/delete` / `kernel/status` / `$/cancelRequest` / `$/progress` / `permission/respond` / `$/permissionRequest`（`session/subscribe` / `session/unsubscribe` 为守护内核 v0.4 计划）

## Development

```bash
cargo test --workspace            # 全量测试（233 个：protocol 47 + permission 40 + core 51 + cli 6 + 其余 9 crate 89）
cargo clippy --workspace -- -D warnings
cargo bench -p agentj-model       # 模型流式基准（criterion）
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cd bindings/ts && npm install && npm run generate && npm run build  # TS 绑定测试
```
