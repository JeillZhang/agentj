# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。内核通过统一的 JSON-RPC 2.0 协议（stdio，未来 TCP / WebSocket）对外暴露能力；CLI、TUI、Web Dashboard、IDE 插件均可作为协议消费者，**任意语言**可开发前端连接内核。

> 当前版本：**0.1.0**（开发中）——IPC 协议契约已建立，内核开发待 v0.1 计划执行。

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 12 个 v0.1 方法（lifecycle 4 / session 5 / kernel/status / `$/cancelRequest` / `$/progress`）
- [x] **JSON Schema**：27 个机器可读 Schema（`schemars` 从 Rust 类型生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（语言无关，method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节）
- [x] **跨语言验证**：TypeScript 绑定测试（`bindings/ts/`，json-schema-to-typescript + tsc）
- [ ] v0.1 内核：Agent 循环、模型管理、工具引擎、会话、上下文（计划就绪，待开发）
- [ ] v0.2 生态：Hook System / Skills / 插件 / MCP 长驻连接
- [ ] v0.3 商业：Web Dashboard / 用量统计 / License

## Project Structure

```
agentj-protocol/     # IPC 协议契约 crate（wire 类型 + schema 生成，零内核耦合）
schemas/             # 27 个 JSON Schema（schemars 生成，机器事实来源）
protocol/            # 方法注册表 methods.json
docs/specs/          # 人类可读协议规范 ipc-protocol.md
bindings/ts/         # TypeScript 绑定测试
docs/superpowers/    # 设计文档 + 实现计划（中文）
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`
- Schema：`schemas/*.json`
- 方法注册表：`protocol/methods.json`
- 方法面（12）：`initialize` / `initialized` / `shutdown` / `exit` / `session/new` / `session/run` / `session/status` / `session/archive` / `session/delete` / `kernel/status` / `$/cancelRequest` / `$/progress`

## Development

```bash
cargo test --workspace            # 全量测试（31 单元 + 7 契约 + 3 注册表）
cargo clippy --workspace -- -D warnings
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cd bindings/ts && npm install && npm run generate && npm run build  # TS 绑定测试
```
