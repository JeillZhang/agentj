# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.23.1 补齐 v0.23.0 计划遗漏（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）——后端部分**
> ——来源：v0.23.0 验收 PASS（BUG-013/014/015 verified per `.superpowers/acceptance/bugs.md`）后复核发现协议/IPC 计划测试缺漏与 `agentj-core` 死代码。
> + 实现计划 `docs/superpowers/plans/2026-09-25-agentj-right-panel-run-details-plan.md`（T0/T5 后端侧补测）。
> 后端范围：`agentj-protocol`（T0 补测 5）+ `agentj-ipc`（T5 补测 4）+ `agentj-core`（删除死代码 `bash_file_changes` + `git_sample` 生产路径断言）；`PROTOCOL_VERSION` 保持 **0.15.0**（52 schemas / 21 方法不变）；工作区 0.23.0 → **0.23.1**。
> 前端部分见 `summary_and_next_fe.md`。

## v0.23.1（后端部分）✅（本次完成）

- ✅ **T0 协议补测（5 测）**：`FileChangeSource::Unknown` 容忍、`FileChange` 新字段（`run_id`/`seq`/`source`/`total_bytes`）往返序列化、`ProgressValue::Todo/FileChange` 外层 `run_id` 加性、`SessionHistoryParams {run_id?}` 与 `SessionRunResult {run_id?}` 缺省省略（不破坏旧报文）。
- ✅ **T5 IPC 补测（4 测）**：broadcast `run_id` 透传（`file_change_into_wire`）、`session/history {run_id}` 过滤（先按 `run_id` 过滤 Todo/FileChange 再 `cursor/limit` 分页，其余 kind 全量）、非法 `run_id` → `-32602` + `data{reason:"invalid_run_id"}`、`sanitize_run_id`/`is_valid_run_id` 校验单元（非法/超长 → None 不抛错）。
- ✅ **死代码清理**：删除 `agentj-core` 未接线的 `bash_file_changes` helper 及其单测（−1 test）；`git_sample` 测试改为在生产路径断言 `source == Some(FileChangeSource::Bash)`（不再依赖 helper）。

## 验证结果（后端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test --workspace` | **660 passed / 0 failed**（v0.23.0 652 基线 +5 protocol +4 ipc −1 core 死测） |
| `cargo test -p agentj-protocol` | **90 passed**（+5，T0）；`drift` 2 passed（52 schemas） |
| `cargo test -p agentj-protocol -- drift` | 2 passed（52 schemas） |
| `cargo test -p agentj-ipc` | **171 passed**（+4，T5） |
| `./scripts/check-version.sh` | version-ok（**0.23.1**；`PROTOCOL_VERSION` **0.15.0**；`protocol/methods.json` 0.15.0） |
| `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` | 无漂移（52 schemas） |

> **v0.23.0 验收：PASS**（BUG-013/014/015 verified per `.superpowers/acceptance/bugs.md`；Rust 回归后 646 → 652）。本波 v0.23.1 为计划遗漏补齐，**待 `jskill-test-acceptance` 复验**——后端验收项：`cargo test --workspace` 660 独立复跑、T0/T5 补测覆盖、死代码删除后 `git_sample` 生产路径 `source == Some(Bash)` 断言、`check-version` 0.23.1/0.15.0。
> 环境预置清理（延续）：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`（e2e_permission 硬编码 `s1` 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。

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

- **v0.23.1 计划遗漏补齐已在开发分支落地，待 `jskill-test-acceptance` 复验后合并 main**；当前功能完结，下一特性从「待实施」表选择（`jskill-feature-next` 合并本表 + `_fe.md` 表）。
- 后端候选即上表 ①②④（前端候选见 `summary_and_next_fe.md`）。
