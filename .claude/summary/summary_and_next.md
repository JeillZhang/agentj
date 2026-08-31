# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G2 修复：移除单全局 in-flight 会话槽（per-session 路由）**——设计文档 `docs/superpowers/specs/2026-08-31-g2-per-session-inflight-routing-design.md`（设计完成，待用户审阅 → plan）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| 🔄 设计中 | **G2 修复：单全局 in-flight 会话槽 → per-session 路由**（设计文档已完成，待用户审阅批准 → `/jskill-feature-plan`） | v0.5 | P0 | 已设计 |

## Phase: G2 修复（移除单全局 in-flight 会话槽）设计完成 ✅（本次完成）

### 进度与摘要

- ✅ **设计文档已写入** `docs/superpowers/specs/2026-08-31-g2-per-session-inflight-routing-design.md`（中文，Mermaid 建模）：
  - **根因**：`IpcServer.session: Arc<Mutex<Option<String>>>` 单全局槽（`agentj-ipc/src/lib.rs:286`），被 `dispatch_permission_respond`（L703 属主门控）与 `dispatch_cancel_request`（L753 路由）读取；并发跨会话 run 互相覆盖；`CancelParams.id` 被解析后丢弃。内核层（`TurnRunner.turns`）、run 互斥（`pending_runs`）、审批（`PermissionModule.pending: request_id→session_id`）、属主（`session_owner`）**均已 per-session**，污染仅源于此冗余槽。
  - **方案**（用户已确认两项决策）：① wire 范围 = `CancelParams.session_id` + `$/permissionRequest.session_id`（双加性，`@since 0.6.0`），`permission/respond` wire 不变；② cancel 缺省兜底 = `pending_runs` 单会话兜底、多会话 no-op + warn（歧义安全）。
  - **实施要点**：删除全局槽 + `InflightSessionGuard`；respond 重写为 `session_of(rid)` 查表 → `session_owner` 门控（None → -32600）；cancel 重写为 wire session_id 精准路由；`register_ask` 构造 params 携带 session_id；`attach_permission_notifier` 透传。
  - **测试**：L1282/L1374 两既有测试改造 + 新增并发跨会话测试（属主隔离 / cancel 精准路由 / 兜底语义）+ e2e 回归。
- ✅ 差距档案 G2 行状态更新为「✅ 已设计」并附链接。

### 遗留问题 / 已知限制

- respond 校验顺序变化：非属主 + 未知 request_id 组合失败码从 `-32806` 变为 `-32600`（拒绝语义不变，测试锁定）。
- cancel 仍无属主门控（与 v0.4 一致，authz 加固留待后续）。
- G3-G8 差距项未动（G2 范围外）。

### 下一步

- 用户审阅设计文档 → 批准后运行 `/jskill-feature-plan docs/superpowers/specs/2026-08-31-g2-per-session-inflight-routing-design.md` 编写实现计划（T1-T5 TDD 任务分解）。
