# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G4 会话持久化生产接线（FileBackedStore 生产接入）**——设计文档 `docs/superpowers/specs/2026-09-01-g4-session-persistence-wiring-design.md` + 实现计划 `docs/superpowers/plans/2026-09-01-g4-session-persistence-wiring-plan.md`（v0.7.0 已发布）。

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
| ✅ 完成 | **G4 会话持久化生产接线（FileBackedStore 生产接入 + 唯一 tmp + 目录 fsync）**（T0-T6，v0.7.0） | v0.6.0 | P0 | ✅ 已实现 v0.7.0 |

## Phase: G4 会话持久化生产接线（FileBackedStore 生产接入）✅ (本次完成)

### 进度与摘要

- ✅ **根因**：v0.3 宣称「FileBackedStore 磁盘持久化已落地」但 `agentj-cli/src/main.rs:182` 仍 `InMemoryStore::new()`——FileBackedStore 生产零引用（仅定义 + 单测）。守护内核「历史级接续」跨进程/跨重启恢复依赖落空；另两处磁盘缺陷：`persist()` 固定 `.<sid>.json.tmp` 并发互踩 + rename 后未 fsync 目录（崩溃一致性缺失）。

- ✅ **方案**（决策 D1-D3）：① `SessionStore` trait 化 `scan_expired`/`spawn_expiry_loop`（默认 `Ok(0)`/loop 包装），`InMemoryStore`/`FileBackedStore` 各自实现；② `agentj-config` 加性 `[session] dir: Option<PathBuf>`；③ `agentj-cli` 单点 `resolve_session_dir`（`[session] dir` 覆盖 → `$XDG_DATA_HOME/agentj/sessions` → `$HOME/.local/share/agentj/sessions` → `.agentj/sessions`），`Arc<FileBackedStore>` 共享于 `run`/`serve`/`serve --port`/`dashboard`，具体 `Arc<FileBackedStore>` 上驱动 `spawn_expiry_loop`（ttl 3600s interval 60s）；④ `FileBackedStore::persist` 唯一 tmp `.<uuid>.json.tmp` + 目录 `fsync`（防并发 + 保证 rename 持久性）；协议面零变化。

- ✅ **任务分解（T0-T6）已完成**：
  - T0 — `SessionStore` trait 新增 `scan_expired`/`spawn_expiry_loop` 默认实现（`InMemoryStore` 保留行为作参照，新增 `NoExpiryStore` 单测验证默认 `Ok(0)`）。
  - T1 — `InMemoryStore` 固有 `scan_expired`/`spawn_expiry_loop` 迁入 `impl SessionStore for InMemoryStore`（trait 覆盖），新增 `expiry_reachable_through_trait_object` 验证 trait 对象可达。
  - T2 — `FileBackedStore` 磁盘版 `scan_expired`（`updated` 账本 + `SessionStatus::Active` 过滤 + `SessionExpired` 发布），3 单测（stale/fresh/skips-archived）。
  - T3 — `persist` 改唯一 tmp + 目录 fsync（`uuid::Uuid::new_v4()` + `File::open(dir).sync_all()`），2 单测（无残留固定 tmp + 并发不互踩）。
  - T4 — `agentj-config` 加性 `[session] dir`（`SessionConfig` + `AgentJConfig.session` + `merge_config` 覆盖 + `ConfigProvider::get_str("session.dir")`），3 单测。
  - T5 — `agentj-cli` `resolve_session_dir` + `FileBackedStore::new(&dir)` 生产接线（`InMemoryStore` → `FileBackedStore`，`session_store.spawn_expiry_loop` 具体类型调用），3 环境隔离单测（config 优先/XDG 回退/HOME 回退）。
  - T6 — workspace version `0.6.0` → `0.7.0`（`Cargo.toml` + `Cargo.lock` via `cargo build`）。

- ✅ **验证结果**：
  - `cargo test --workspace` — 全量通过（~320，新增 FileBackedStore/Config/CLI 单测 + e2e_cli/e2e_permission/e2e_serve 回归，含 60s permission 超时）
  - `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
  - `cargo test -p agentj-config/session/cli` 专项通过；`cargo build` 更新 Cargo.lock 一致
  - `cargo fmt --all --check` — 通过

### 遗留问题 / 已知限制

- 跨进程同 session 并发写：唯一 tmp 避免互踩，最终 `fs::rename` 以最后写入者胜（last-rename-wins），不加 `fs2` flock（异常场景接受，D3 限定）。
- `FileBackedStore` 仍「每会话一 JSON + 全量重写」模型（大历史全 rewrite 成本未优化，G4 非目标）。
- `scan_expired` 单轮失败 `?` 短路返回错误，`spawn_expiry_loop` 默认实现吞错 `let _ =` 并下一轮继续（单轮非全扫描韧性）。
- 目录 fsync 失败返回 `PersistenceFailed`（保守失败——数据已写但持久性未保证）。
- G3/G5-G8 差距项未动（G4 范围外）。

### 下一步

- 按 gap 档案优先级推进下一差距项（G3、G5-G8），或根据产品规划启动新 feature 设计 → `/jskill-feature-design`。
- 后续可考虑 `FileBackedStore` 增量/压缩存储或 `fs2` 加固（需设计评审）。
