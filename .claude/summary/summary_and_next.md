# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License）**——实现计划 `docs/superpowers/plans/2026-08-09-agentj-v0.3-plan.md`（13 任务 T0-T12 全部完成并通过逐任务评审；版本 0.3.0 → 0.4.0）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布：事件总线 + ModuleResolver + 子代理调度 + skills + 插件 + Ollama + MCP 长驻 + 会话过期 + FileBackedStore） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成（本阶段） | **v0.4 商业扩展**：守护内核常驻 serve --port / run --attach / dashboard、Web Dashboard、agentj-stats 用量统计、agentj-license 许可证校验；方法面 14→16 | v0.1-v0.3 | P2 | 已完成 |

## Phase: v0.4 商业扩展 开发完成 ✅（本次完成）

### 进度与摘要

- ✅ **13 任务全部完成并逐任务评审**：
  - 协议层（T0-T4）：`-32805 SessionBusy` / `-32806 NotAuthorized` 错误码、`InitializeResult.connection_id` / `multi_client` 能力位、`SessionSubscribeParams` wire 类型、`PermissionRequestParams.origin_client` 属主字段、schema 注册与再生成（31→32 个）、方法注册表 14→16 + 契约 wire-shape 测试、TS 绑定重生成。
  - IPC 层（T5-T8）：ConnectionRegistry 多客户端路由（订阅广播 / 属主表 / run 会话互斥）、TcpConnection / TcpIpcServer 常驻服务（--token / --max-connections、末次断连优雅退出）、连接感知分发（initialize token 认证 / run 会话互斥 / permission 属主门控）、`$/progress` 广播 + 权限属主定向路由（`origin_client`）。
  - 商业件（T9-T11）：agentj-stats 用量统计（record_token_usage / record_turn，poison-tolerant）、agentj-license Ed25519 许可证校验（entitlements + 过期策略）、Web Dashboard JSON-RPC 消费端（Vite + React + vitest，kernel/status 面板 + node:net TCP 传输）。
  - 集成（T12）：CLI `serve --port [--token/--max-connections]` 常驻 / `run [--attach tcp://…]` 附着 / `dashboard --port`；UsageCollector 经 AgentLoop on-turn-complete 钩子接线；e2e_serve 密封测试（spawn → listening 行解析 → TCP session/new → 末次断连进程退出）；版本 0.3.0 → 0.4.0。
- ✅ **验证门禁全绿**：workspace 272 测试通过 / 0 失败（含 e2e_cli 2 + e2e_permission 1 + e2e_serve 1）；clippy --workspace --all-targets -D warnings 零警告；fmt 干净；drift 门禁绿（schemas 32 个）；bindings/ts 构建绿；web-dashboard vitest + vite 构建绿。
- ✅ 执行方式备注：Group A 四任务经并行子代理实现；其余任务因子代理基础设施反复静默失败改由控制器直接执行（用户批准），逐任务 TDD + 报告留档于 `.superpowers/sdd/2026-08-09-agentj-v0.3-plan/`。

### 遗留问题 / 已知限制

- **单全局 in-flight 会话槽**（继承自 v0.2 设计）：并发 run 于不同会话时会互相覆盖 inflight 记录，属主门控读取的是最近一次记录；按会话互斥已由 run-mutex 保证，inflight 槽改造留待后续设计。
- **TCP 常驻模式不等待内核 shutdown 句柄链**：IpcServer 持有于 Arc 内 await 需 &mut；进程在末次断连/关停标志后即刻退出，stdio 路径不受影响。
- **token 级用量计数待 provider 上报 usage 后接入**（当前 record_turn 仅记 turns）——计划内 [design-pending] 项。
- **agentj-license 为独立库能力**，商用特性门控接入点待后续设计。
- 继承自 v0.3 的已知 MEDIUM 项不变（MCP 空闲回收生产布线不可达、EventBus 重入、FileBackedStore tmp 竞争等，详见 v0.3 总结归档）。
- Web Dashboard 为协议消费端临时形态：浏览器 bundle 中 node:net 被外部化，运行时需 Node 环境或后续 WebSocket 网关。

### 下一步

- 运行 `/jcmd-test-acceptance` 启动测试验收流程（测试 agent 发现 Bug → Bug-fix agent 修复 → 回归验证），验收通过后创建 PR。
