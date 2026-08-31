# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G2 修复：移除单全局 in-flight 会话槽（per-session 路由）**——设计文档 `docs/superpowers/specs/2026-08-31-g2-per-session-inflight-routing-design.md` + 实现计划 `docs/superpowers/plans/2026-08-31-g2-per-session-inflight-routing-plan.md`（v0.6.0 已发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| ✅ 完成 | **G2 修复：单全局 in-flight 会话槽 → per-session 路由**（T1-T6，v0.6.0） | v0.5 | P0 | ✅ 已实现 v0.6.0 |

## Phase: G2 修复（移除单全局 in-flight 会话槽）✅ (本次完成)

### 进度与摘要

- ✅ **根因**：`IpcServer.session: Arc<Mutex<Option<String>>>` 单全局槽被 `dispatch_permission_respond`（属主门控）与 `dispatch_cancel_request`（取消路由）共享；并发跨会话 run 互相覆盖，cancel 打错会话、respond 属主误判。内核层（`TurnRunner.turns`）、run 互斥（`pending_runs`）、审批（`PermissionModule.pending: request_id→session_id`）、属主（`session_owner`）均已 per-session，污染仅源于此冗余槽。

- ✅ **方案**（决策 D1-D5）：① 删除全局槽 + `InflightSessionGuard`，`pending_runs` 为唯一 in-flight 事实来源；② `permission/respond` 经 `PermissionModule::session_of(request_id)` 查表再 `session_owner` 门控（None → `-32600`，非属主 → `-32806`）；③ `$/cancelRequest` 经加性 `CancelParams.session_id`（`Option<SessionId>` `@since 0.6.0`）精准路由，缺省 `pending_runs` 单会话兜底、多会话 no-op + `tracing::warn!`；④ `$/permissionRequest` 加性 `session_id` 供客户端多会话 UI 归类；`protocol_version` 保持 `0.2.0`。

- ✅ **任务分解（T1-T6）已完成**：
  - T1 — `CancelParams` + `PermissionRequestParams` 加性 `session_id`（`Option` + `skip_serializing_if` + `@since 0.6.0`），`schemas/*.json` + `bindings/ts/types.d.ts` 重生成，单测 + drift 门禁通过。
  - T2 — `PermissionModule::session_of` 只读 peek + `register_ask` 携带 `session_id`（`ctx.session_id.clone()`）透传至 notifier。
  - T3 — 移除全局槽；重写 `dispatch_permission_respond` / `dispatch_cancel_request`；`attach_permission_notifier` 透传 `session_id`；新增并发跨会话测试（属主隔离 / cancel 精准路由 / 兜底语义）+ 改造 L1282/L1374 既有测试。
  - T4 — `docs/specs/ipc-protocol.md` §6/§8.15/§8.16 标注 `@since 0.6.0` 与缺省行为 + gap 档案 G2 行 ✅ 已实现。
  - T5 — workspace version `0.5.1` → `0.6.0`（`Cargo.toml` + `Cargo.lock`）。
  - T6 — `AGENTS.md` / `README.md` / 本文件同步至 v0.6.0 per-session 路由。

- ✅ **验证结果**：
  - `cargo test --workspace` — 全量通过（含新增 per-session 路由测试 + e2e_cli/e2e_permission/e2e_serve 回归）
  - `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
  - `cargo test -p agentj-protocol -- drift` — 通过（schemas == 提交件）
  - `cd bindings/ts && npm run generate && npm run build` — 通过
  - `cargo fmt --all --check` — 通过

### 遗留问题 / 已知限制

- `permission/respond` 校验顺序变化：旧代码先读全局槽门控、后查 request，新代码先 `session_of` 查存在性（`-32600`）再门控（`-32806`）；**非属主 + 未知 request_id** 组合失败码从 `-32806` 变为 `-32600`（拒绝语义不变，测试锁定）。
- `$/cancelRequest` 仍无属主门控（与 v0.4 一致，控制面开放；authz 加固留待后续）。
- G3-G8 差距项未动（G2 范围外）。

### 下一步

- 按 gap 档案优先级推进下一差距项（G3-G8），或根据产品规划启动新 feature 设计 → `/jskill-feature-design`。
- 后续若需 cancel 属主加固，可在 `dispatch_cancel_request` 增加 `session_owner` 校验（需设计评审）。
