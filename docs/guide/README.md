# AgentJ 用户指南

AgentJ 各端的功能说明与使用方式。**版本变更记录**见
[`docs/changelog/`](../changelog/)；协议规范见
[`docs/specs/ipc-protocol.md`](../specs/ipc-protocol.md)。

## AgentJ 是什么

AgentJ 是一个 AI 编码代理工具（Rust），采用 Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；CLI、TUI、
Web、Desktop 均作为**协议消费者**接入，任意语言都可开发前端。内核亦可直接托管 Web UI，
支持局域网/远程访问与登录鉴权。

## 目录

| 文档 | 内容 |
| --- | --- |
| [快速上手](getting-started.md) | 环境要求、构建、启动内核、打开各端 |
| [命令行（CLI）](cli.md) | `run` / `serve` / `dashboard` 与参数 |
| [Web 工作台](web.md) | 三栏工作台、composer、右栏运行详情、设置 |
| [终端界面（TUI）](tui.md) | 会话视图、文件视图与搜索、键位 |
| [桌面应用（Desktop）](desktop.md) | Tauri 壳与安装包 |
| [配置](configuration.md) | `config.toml`（模型 / 权限 / 会话 / license / MCP） |
| [HTTP 面与鉴权](http-and-auth.md) | 内核托管 HTTP、登录、`/version`、`/metrics` |

## 文档导航

- 想了解**有哪些功能** → 本指南各章。
- 想了解**版本改了什么** → [`docs/changelog/`](../changelog/)。
- 想了解**协议契约** → [`docs/specs/ipc-protocol.md`](../specs/ipc-protocol.md)
  与 `protocol/methods.json`、`schemas/*.json`。
- 想了解**内部架构** → 仓库根 [AGENTS.md](../../AGENTS.md)。
