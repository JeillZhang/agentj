# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP，未来 WebSocket）对外暴露能力；CLI、TUI、Web Dashboard、IDE 插件均可作为协议消费者，**任意语言**可开发前端连接内核。

> 当前版本：**v0.7.0**——G4 会话持久化生产接线（16 crate，FileBackedStore 生产接入：`resolve_session_dir` XDG 默认 + `[session] dir` 覆盖 + 单点实例化共享于 run/serve/serve --port/dashboard + `SessionStore` trait 化 `scan_expired`/`spawn_expiry_loop` + `FileBackedStore::persist` 唯一 tmp `.<uuid>.json.tmp` + 目录 fsync；v0.6 全保留：per-session in-flight 路由（pending_runs 事实来源 + session_of 查表门控 + CancelParams.session_id 精准路由）；v0.5 全保留：内核调度全重构：KernelMessage 总线化、TurnRunner 总线化、ToolExecutorModule/AgentLoopModule、subagent/dispatch、IpcServer/CLI 全面接总线；v0.4 全保留：守护内核多客户端常驻、方法面 16、$/progress 广播+权限属主路由、stats/license/Web Dashboard；v0.3 全保留：事件总线 + 子代理调度 + skills + 插件 + MCP 长驻连接 + 会话过期 + FileBackedStore + Ollama；v0.2 全保留：IPC 协议契约 + 权限审批）。

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 16 个方法（lifecycle 4 / session 7（含 `session/subscribe` `session/unsubscribe` 广播订阅）/ kernel/status 1 / `$/cancelRequest` `$/progress` 通知 2 + `permission/respond` `$/permissionRequest` 2）
- [x] **权限审批系统**：`$/permissionRequest` 通知（S→C，`session_id` @since 0.6.0 多会话归类）+ `permission/respond` 请求（C→S，per-session 属主门控经 `module.session_of` 查表）+ 审批流（request_id 解耦、60s 超时自动 Deny、allow_always 会话级内存态、取消即拒绝）
- [x] **JSON Schema**：32 个机器可读 Schema（`schemars` 从 Rust 类型生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（语言无关，method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节，v0.2.0）
- [x] **跨语言验证**：TypeScript 绑定测试（`bindings/ts/`，json-schema-to-typescript + tsc）
- [x] **内核与模块注册表**：Kernel 混合总线（`UserRequest` + `ToolCallRequest` 目标路由 + `ServiceCall` 同步/异步 + `Shutdown`）+ `Kernel::modules()` + `ToolCallReply` + `AsyncKernelService` + `ServiceRegistry` trait 注册表（ModelProvider / SessionStore / ToolRegistry / ContextPipeline / ConfigProvider）
- [x] **Agent 循环（流式）**：`TurnRunner` 总线化编排（`spawn_turn`/`cancel`/`shutdown`，工具边界经 `ToolCallRequest` suspend 于 oneshot）+ `AgentLoopModule`（`UserRequest` 消费 + `agent/cancel`）+ `ToolExecutorModule`（`execute_async` 权限门禁）+ `SubAgentService` async `subagent/dispatch`
- [x] **内置工具**：bash / glob / grep / read / write + ToolExecutor
- [x] **MCP 桥**：子进程 MCP 客户端 + 工具发现注册
- [x] **JSON-RPC IPC（stdio + TCP 多客户端）**：16 方法服务循环 + 优雅退出 + per-session 路由（`$/cancelRequest` `session_id` 精准路由 + 缺省单会话兜底/多会话 no-op + `tracing::warn!`）+ 权限属主门控经 `session_of` 查表
- [x] **CLI run/serve/dashboard**：`agentj run [--attach tcp://…] "prompt"` / `agentj serve [stdio]` / `agentj serve --port`（常驻多客户端） / `agentj dashboard --port`
- [x] **模型提供方**：DeepSeek + opencode-go 统一订阅（Chat Completions / Responses / Anthropic 三线格式，ProviderFactory 按 `kind` 实例化）+ **Ollama provider**
- [x] **事件总线**：`EventBus` 订阅/发布，全生命周期事件（模块 / 会话 / MCP / 权限）
- [x] **子代理调度器**：Kernel 派发 + 任务状态跟踪
- [x] **Skills 加载器**：`.aj/.agents/.claude/skills` 优先级合并（同名高优先胜出）
- [x] **插件系统**：manifest 解析 + 生命周期状态机 + 依赖图 + libloading 动态加载
- [x] **MCP 长驻连接**：`McpConnectionManager` 引用计数共享 + 空闲回收 + 指数退避重连 + ghost-tool 注销
- [x] **会话过期 + 磁盘持久化**：`spawn_expiry_loop` 过期清理 + FileBackedStore 快照落盘
- [x] **守护内核**：统一 Connection 传输抽象（stdio 哨兵 / TCP；Unix socket 预留）+ 常驻 API 服务（`agentj serve --port` 多客户端共享、可接续任务，--token/--max-connections）
- [x] **Web Dashboard**：JSON-RPC 协议消费端（Vite + React + TS；kernel/status 面板 + node:net TCP 传输）
- [x] **用量统计**：agentj-stats UsageCollector（per-session/global totals + turns；经 AgentLoop on-turn-complete 接线）
- [x] **License 校验**：agentj-license Ed25519 签名密钥校验（entitlements + 过期策略；design-pending 商用门控）

## Project Structure

```
crates/              # 16 个 workspace crate（统一目录）
  agentj-protocol/   # IPC 协议契约 crate（wire 类型 + schema 生成，零内核耦合；v0.6 加性 CancelParams/PermissionRequestParams session_id @since 0.6.0）
  agentj-core/       # 内核：Kernel 混合总线调度 + 模块 trait 注册表（SessionStore trait 化 scan_expired/spawn_expiry_loop） + TurnRunner + ToolExecutorModule + AgentLoopModule + 异步子代理
  agentj-config/     # TOML 配置（[model] provider/providers[]/kind/default_model；[permission] per-tool ToolPolicy；[session] dir）
  agentj-session/    # 会话生命周期（InMemoryStore/FileBackedStore + 统一过期清理 + FileBackedStore 唯一 tmp + 目录 fsync）
  agentj-model/      # 模型提供方：三线格式 + OpenCodeGo 门面 + ProviderFactory + Mock + Ollama
  agentj-tools/      # 内置工具（bash/glob/grep/read/write）+ ToolExecutor
  agentj-permission/ # 权限系统：决策引擎 + 策略模型（[permission] 配置）+ PermissionModule
  agentj-context/    # 上下文管线（纯消息构造器）
  agentj-stats/      # 用量统计 UsageCollector（per-session/global totals + turns）
  agentj-license/    # Ed25519 许可证校验（entitlements + 过期策略）
  agentj-mcp/        # MCP 客户端（子进程连接 + 工具发现注册 + McpConnectionManager 长驻连接）
  agentj-skills/     # skills 加载器（.aj/.agents/.claude/skills 优先级合并）
  agentj-plugin/     # 插件系统（manifest + 生命周期状态机 + 依赖图 + libloading 动态加载）
  agentj-ipc/        # JSON-RPC 2.0 IPC 服务（stdio + TCP 多客户端常驻，全面接总线：session/run→UserRequest + $/cancelRequest→agent/cancel per-session 精准路由 + permission/respond session_of 门控 + kernel/status→modules()）
  agentj-cli/        # agentj 二进制（run/serve/dashboard，resolve_session_dir XDG + [session] dir → FileBackedStore 单点实例化，spawn_expiry_loop 磁盘版）
  agentj-plugin-fixture/  # 插件 cdylib 测试夹具（dev-only）
models/              # 模型目录 models/opencode-go.json（opencode-go 路由事实来源）
schemas/             # 32 个 JSON Schema（schemars 生成，机器事实来源）
protocol/            # 方法注册表 methods.json
docs/specs/          # 人类可读协议规范 ipc-protocol.md
bindings/ts/         # TypeScript 绑定测试
web-dashboard/       # Web Dashboard 前端（Vite + React + TS，JSON-RPC 消费端）
docs/superpowers/    # 设计文档 + 实现计划（中文）
```

## Usage

```bash
agentj run "explain this codebase"              # 单轮运行（嵌入式 JSON-RPC 内核 + TurnRunner 总线化 turn）
agentj run --attach tcp://127.0.0.1:4096 "hi"   # 附着到常驻内核运行（不新建内核）
agentj serve                                    # 单客户端 stdio 服务（EOF/信号优雅退出）
agentj serve --port 4096                        # 常驻 TCP 内核（多客户端共享，可接续任务）
agentj serve --port 4096 --token SECRET         # 常驻 TCP 内核（initialize 需携带 token）
agentj dashboard --port 4096                    # 为 Web Dashboard 提供同款 TCP 协议入口
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

[session]                          # 会话持久化（可选；缺省 XDG_DATA_HOME/agentj/sessions）
dir = "/data/agentj/sessions"      # 覆盖默认会话目录
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`（v0.2.0）
- Schema：`schemas/*.json`
- 方法注册表：`protocol/methods.json`
- 方法面（16）：`initialize` / `initialized` / `shutdown` / `exit` / `session/new` / `session/run` / `session/status` / `session/archive` / `session/delete` / `session/subscribe` / `session/unsubscribe` / `kernel/status` / `$/cancelRequest` / `$/progress` / `permission/respond` / `$/permissionRequest`

## Development

```bash
cargo test --workspace            # 全量测试（~320 通过，含 bus 密封测试 + FileBackedStore/Config/CLI 新单测 + e2e_cli/e2e_permission/e2e_serve 密封测试）
cargo clippy --workspace -- -D warnings
cargo bench -p agentj-model       # 模型流式基准（criterion）
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cd bindings/ts && npm install && npm run generate && npm run build  # TS 绑定测试
cd web-dashboard && npm install && npm test && npm run build        # Web Dashboard 测试与构建
```
