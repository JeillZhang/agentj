# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G7 EventBus 重入防护 + G8 WebSocket 网关 (v0.10.0)**——设计文档 `docs/superpowers/specs/2026-09-04-g7-eventbus-reentrancy-g8-websocket-gateway-design.md` + 实现计划 `docs/superpowers/plans/2026-09-04-g7-g8-plan.md`（G1-G8 全部闭环，v0.10.0 发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| ✅ 完成 | G2 修复：单全局 in-flight 会话槽 → per-session 路由（T1-T6，v0.6.0） | v0.5 | P0 | ✅ 已实现 v0.6.0 |
| ✅ 完成 | G4 会话持久化生产接线（FileBackedStore 生产接入 + 唯一 tmp + 目录 fsync）（T0-T6，v0.7.0） | v0.6.0 | P0 | ✅ 已实现 v0.7.0 |
| ✅ 完成 | G3 token 级用量统计（TokenUsage + 4 提供方 + SessionRunResult.usage + CLI 激活）（T0-T11，v0.8.0） | v0.7.0 | P0 | ✅ 已实现 v0.8.0 |
| ✅ 完成 | G5 License gating + G6 MCP wiring（[license] + McpServerSpec + close_all + per-command entitlement）（T0-T8，v0.9.0 + v0.9.1 验收补丁） | v0.8.0 | P0 | ✅ 已实现 v0.9.1 |
| ✅ 完成 | **G7 EventBus 重入防护 + G8 WebSocket 网关（v0.10.0）**（T0-T9，G1-G8 全闭环） | v0.9.1 | P0 | ✅ 已实现 v0.10.0 |

## Phase: G7 EventBus 重入防护 + G8 WebSocket 网关 (v0.10.0) ✅ (本次完成)

### 进度与摘要

- ✅ **根因**：G7 — `EventBus::publish` 同步串行执行 handler，handler 自触发同类型事件形成无限递归栈溢出，BUG-001 快照与 BUG-002 `catch_unwind` 已修复但无递归深度防护（`hooks.rs:50-67` 同步发布 14 处生产收口点均暴露）——设计债；G8 — Dashboard `RpcClient` 硬编码 `node:net` TCP，浏览器 `TypeError`，`Connection` 传输抽象已为 WS 留位，差距档案 G8 形态限制。

- ✅ **方案**（设计 D1-D8）：G7 在 `EventBus::publish` 入口单点加 `EventGuard`（thread_local `EVENT_DEPTH: Cell<u8>` + `EVENT_STACK: Vec<EventType>`，`MAX_EVENT_DEPTH=16`，同栈 `stack.contains(event)` 即时拦截 + 跨类型环深度兜底，超限/循环 `tracing::warn!` 丢弃，RAII Drop 回卷 + `catch_unwind` 叠加，调用点零改动）；G8 将 `TcpIpcServer` accept 循环升级为连接分类器（`MSG_PEEK` + `looks_like_http`，HTTP→`serve_http`/非 HTTP→`TcpConnection`），单一 `TcpListener` 同端口共存 raw TCP 与 HTTP/WS，新增 `WsConnection` 实现 `Connection`（`TransportKind::WebSocket`，原生 Text 帧，无 `\n` 拼接，mpsc writer 桥复用半连接写句柄，复用 `register_writer` + `serve_connection_requests` 零改动），服务端 `WsServerState`/`ws_router`/`ws_upgrade`/`serve_http`（axum 0.8 ws + hyper-util `serve_connection_with_upgrades`，鉴权双通道 `?token=` 403 + `initialize.token` -32806，`max_connections` 独立计数，WS 不参与 last-disconnect 全局关闭），前端 `WebSocketTransport`（原生 `WebSocket`，`VITE_AGENTJ_WS_URL` 可配，`onStatusChange` + `close`）+ `ResponseRouter` 通知分叉（BUG-005 修复，`id==null` 不再丢弃），删除 `node:net`，CLI `serve --port` 打印 `websocket on ws://…/ws`。

- ✅ **任务分解（T0-T9）已完成**：
  - T0 — `hooks.rs` 新增 `MAX_EVENT_DEPTH=16` + `EventGuard` RAII（thread_local depth/stack + `enter` 循环/深度检查 + Drop 回卷）+ `publish` 入口 guard，含 3 单测（同类型自触发 1 次 / 跨类型 chain 深度 ≤16 / panic 后 guard 回卷）。
  - T1 — `TransportKind::WebSocket` + `agentj-ipc/Cargo.toml` 新增 axum 0.8(ws)/hyper/hyper-util/futures/tower + dev-dep tokio-tungstenite。
  - T2 — 前端 `RpcTransport` 扩展 `close?` + `ResponseRouter.setNotificationHandler` + `feed` 通知分叉（BUG-005）+ `RpcClient(transport, router?)` 可选 router。
  - T3 — `ws.rs` 新增 `WsConnection`（split + mpsc writer 桥，原生 Text 帧，close→None，ping/pong 跳过）+ `pub mod ws`，含 2 单测（roundtrip via writer / close EOF）。
  - T4 — `tcp.rs` 同端口连接分类器（`looks_like_http` + `MSG_PEEK` peek + `WsServerState`/`ws_router`/`ws_upgrade`/`serve_http`）+ `ws_classifier.rs` 4 集成（同端口 WS+TCP 共存 / `?token=` 403 / `initialize.token` -32806 / `max_connections` WS 限流）。
  - T5 — `web-dashboard/src/transport/websocket.ts` 新增 `WebSocketTransport`（native WebSocket，`onStatusChange`，`onmessage→feed`，`onerror/onclose→rejectAll`）+ `App.tsx` `VITE_AGENTJ_WS_URL` 接线 + 删除 `node:net`，含 3 单测（send/response / notification feed / error+status）。
  - T6 — `agentj-cli` `serve --port` 打印 `websocket on ws://…/ws` + `e2e_ws.rs` 双传输 e2e（WS+TCP 同端口 + `?token` 鉴权 + WS 不参与全局关闭）。
  - T7 — workspace version `0.9.1` → `0.10.0`（`Cargo.toml` + `Cargo.lock`）。
  - T8 — 文档 `docs/specs/ipc-protocol.md` §2.3 扩展为 v0.10.0 WS 传输节 + gap 档案 G7/G8 状态行 → ✅ 已实现。
  - T9 — 本次：doc sync `AGENTS.md`/`README.md`/summary（G7+G8 阶段内容整体替换）。

- ✅ **决策 D1-D8**：
  - D1 guard 放置 `EventBus::publish` 入口：14 生产发布点全收口，单点防护全覆盖、调用点零改动。
  - D2 超限行为丢弃 + `tracing::warn!`：不 panic，与 BUG-002 panic 隔离一致，插件 bug 不拖垮进程。
  - D3 防护粒度深度 16 + 同栈循环检测双保险：同类型即时拦截，跨类型环深度兜底。
  - D4 同端口 + axum：单一 `127.0.0.1:port` 分类器共存，`run --attach tcp://` 零破坏。
  - D5 hyper-util `serve_connection_with_upgrades` + axum Router service_fn 包装：分类器逐连接手动接管，官方 `serve-with-hyper` 模式。
  - D6 鉴权双通道：upgrade `?token=` 403 + 连接后 `initialize.token` -32806，`auth_gate` 传输无关零改动。
  - D7 WS 不参与 last-disconnect 全局关闭：浏览器会话短暂，不应决定常驻内核存亡。
  - D8 前端 `WebSocketTransport` + `RpcTransport` 扩展 + BUG-005 修复：`RpcClient`/`ResponseRouter` 既有解耦点直接复用，最小替换会让 Dashboard 看不到进度。

- ✅ **验证结果**：
  - `cargo test --workspace` — 全量通过（含 G7 hooks 3 + G8 ws 2 + ws_classifier 4 + websocket 3 + e2e_ws 双传输 + 既有 e2e_cli/e2e_permission/e2e_serve 回归）
  - `cargo test -p agentj-protocol -- drift` — 2 通过（33 schemas 无漂移，WS 为传输层，协议零变化）
  - `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
  - `cargo fmt --all --check` — 通过
  - `cd web-dashboard && npm test && npm run build` — 通过，`dist/assets/*.js` 无 `node:net`

### 遗留问题 / 已知限制

- G7 guard 为调用线程局部（`thread_local`）：跨线程并发发布不受深度约束（语义本为单线程调用栈保护，跨线程各栈独立）。
- G8 同端口分类器以首字节形似 HTTP 判定，极端 NDJSON 首行以 `GET ` 等开头理论误判（实际 JSON-RPC 首字节必为 `{`，无碰撞）。
- G8 浏览器 WSS/TLS、HTTP REST 面、CORS、每连接速率限流均明确非目标，随产品需求独立设计。
- WS 客户端重连为前端尽力而为（指数退避 + `onStatusChange`），服务端不做会话粘滞/断线续传（沿用 `session/subscribe` 重连观测语义）。
- gap 档案 G1-G8 全部 → ✅ 已实现，全闭环。

### 下一步

- G1-G8 全闭环，按产品规划启动新 feature 设计 → `/jskill-feature-design`，或进入发布流程（PR 合并 → `feature/g7-g8-eventbus-websocket-gateway` → `main`）。
- 可选增强：serve 周期性 license 复验、`close_all` 超时控制、`[[mcp.servers]]` 热重载、WS WSS/TLS、事件追溯日志（当前仅 warn 计数）。
