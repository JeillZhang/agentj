# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.23.0 右边栏运行详情 — run_id 维度 + FileChangeSource + total_bytes（七处加性 wire，PROTOCOL 0.14.0 → 0.15.0）——后端部分**
> ——来源：用户请求「右边栏运行详情中，任务与文件变更协议设计实现」+ 设计文档
> `docs/superpowers/specs/2026-09-25-agentj-right-panel-run-details-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-25-agentj-right-panel-run-details-plan.md`
> （T0–T11 全部落地，T7/T8 由前端侧落地，本文件仅后端）。
> 后端范围：`agentj-protocol`（FileChangeSource + 7 处加性字段）+ `agentj-core`（TurnRunner per-run UUID + artifact source/total_bytes + git_sample bash 标记）+ `agentj-ipc`（broadcast sanitize + trajectory run_id + history 过滤 + SessionRunResult.run_id）+ `docs/specs/ipc-protocol.md`（§7.3/8.7/8.19/9.1/10/11 同步）+ `schemas/`（52 schemas）+ `protocol/methods.json`（0.15.0）；`PROTOCOL_VERSION` 0.14.0 → **0.15.0**；工作区 0.22.0 → **0.23.0**。
> 前端部分见 `summary_and_next_fe.md`。

## v0.23.0（后端部分）✅（本次完成）

- ✅ **T0 协议 wire**：`streaming.rs` 新增 `FileChangeSource`（Write/Bash + Unknown）与 `FileChange {run_id, seq, source, total_bytes}`（`Option` + `skip_serializing_if`）+ `ProgressValue::Todo/FileChange {run_id?}` 外层路由提示 + `TrajectoryValue::Todo/FileChange {run_id?}` + `SessionHistoryParams {run_id?}` + `SessionRunResult {run_id?}`；`schema.rs` 注册 `FileChangeSource`；现有测试补齐 `run_id` 缺省构造，`gen_schemas` 生成 `FileChangeSource.json`。
- ✅ **T1 版本 bump**：`lib.rs:PROTOCOL_VERSION` 0.14.0 → **0.15.0**、`protocol/methods.json` 同步 0.15.0（21 方法不变），`registry` 测试同步；`check-version` 同步。
- ✅ **T3 TurnRunner per-run**：`types.rs` `AgentEvent::Todo {run_id, revision}` / `Done {run_id}`；`turn_runner.rs` 新增 `todo_revisions: HashMap<RunId,u64>` + `file_change_seqs: HashMap<RunId,u64>`（UUID 键全局唯一），`spawn_turn` 入口 `Uuid::new_v4()`，`forward_artifacts` 写入 `change.run_id/seq` 与 `Done.run_id`；`Cargo.toml` 新增 `uuid v4`。
- ✅ **T4 artifact 双源**：`artifacts.rs` 扩展 `FileChangeArtifact {run_id, seq, source, total_bytes}` + `pub use FileChangeSource`，`write_change` 按 `DIFF_MAX_BYTES` 截断时保留 64 KiB 前缀 + `total_bytes` 披露、`source=Write`；新增 `bash_file_changes` helper；`git_sample.rs:change_for` 补 `source=Bash`；T4 遗留单测 `write_change_diff_truncates_over_cap` 同步新语义。
- ✅ **T5 IPC 接线**：`lib.rs` `dispatch_session_run` 新增 `current_run_id` 跟踪 + `sanitize_run_id`（非法/超长→None，不抛错）+ `is_valid_run_id`（`Uuid::parse_str` + ≤128）、`file_change_into_wire` 透传 `run_id/seq/source/total_bytes`、`ProgressValue::Todo/FileChange` 外层 `run_id`、轨迹落盘 `run_id`、`SessionRunResult.run_id` 回显；`dispatch_session_history` 加性 `run_id` 参数校验（非法→`-32602` + `data{reason:"invalid_run_id"}`）+ `history_events_filtered`（先按 `run_id` 过滤 Todo/FileChange，再按 `cursor/limit` 分页，其余 kind 全量）。
- ✅ **T10 文档与 schema**：`docs/specs/ipc-protocol.md` 同步 0.15.0（§7.3 ProgressValue/FileChange、§8.7 SessionRunResult、§8.19 session/history run_id 过滤、§9.1 FileChangeSource、§10 52 schemas、§11.3 0.15.0 changelog）；`cargo gen_schemas` + `npm generate @agentj/protocol` 产出 `FileChangeSource.json` 与更新的 5 个 schema；`types.d.ts` 同步 `FileChangeSource`/`run_id`/`seq`/`source`/`total_bytes`。
- ✅ **T11 最终门禁**：`Cargo.toml` 工作区 0.22.0→**0.23.0**（三处桌面 manifest 同步），`scripts/check-version.sh` 同步 0.23.0/0.15.0；`cargo fmt` + `clippy -D warnings`（补 collapsible-if 与 `is_multiple_of`）全绿。

## 验证结果（后端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean（自动 `cargo fmt` 后校验） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test --workspace` | **648 passed / 0 failed**（含 T3 新增 per-run 覆盖，旧 `e2e_permission` 硬编码 `s1` 需预清理 `~/.local/share/agentj/sessions/s1.*`） |
| `cargo test -p agentj-protocol` | 85+4 passed；`drift` 2 passed（52 schemas） |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `cargo test -p agentj-ipc` | 167 passed |
| `./scripts/check-version.sh` | version-ok（**0.23.0**；`PROTOCOL_VERSION` **0.15.0**；`protocol/methods.json` 0.15.0） |
| `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` | `FileChangeSource` + 5 更新 schema 写入 `types.d.ts` |

> 环境预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`
> （e2e_permission 硬编码 `s1` 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> **验收状态：待验收（`jskill-test-acceptance` 未跑）**——后端验收项：多 run `run_id` 隔离广播、history 按 `run_id` 过滤 + 分页、非法 run_id `-32602`、旧数据 `null` 兼容回放、全量门禁独立复跑目标。

## 遗留问题 / 已知限制（后端）

- **per-run revision 迁移**：旧 `todo_revisions: HashMap<SessionId,u64>` 存量内存态直接切为 `HashMap<RunId,u64>`（重启即清空，无磁盘迁移；旧数据 `run_id=null` 走 legacy revision 兼容分支，不影响历史回放）。
- **run_id 校验口径**：live 帧非法 run_id 视为 `None`（静默降级为未分组，不抛错）；`session/history` 入参非法则严格 `-32602`（与前端 `parseProgress` 的 UUID 正则一致）。
- **TUI 元数据**：后端仅保证 wire 正确；TUI 侧未发 `project_root`（键仍为内核 cwd 回退）与 rank 三处镜像问题仍待实施 ④。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **v0.23.0 开发分支 `feature/right-panel-run-details` 全量落地，待 `jskill-test-acceptance` 验收后合并 main**；当前功能完结，下一特性从「待实施」表选择（`jskill-feature-next` 合并本表 + `_fe.md` 表）。
- 后端候选即上表 ①②④（前端候选见 `summary_and_next_fe.md`）。
