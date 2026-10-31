# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **B10 收尾债批次（后端侧）**——`PROTOCOL_VERSION` 仍 **`0.21.0`**
（29 方法 / 81 schemas / 15 crate 全部不变，本批只更正文档与 schema 描述里的字段名），
工作区版本仍 **`0.1.0`**。

> 当前功能：**B10 收尾债批次（后端三项目）** —— 分支
> `feature/b10-debt-observability-batch`，后端侧为 T0–T6 / T10 / T11。
> 设计与计划是本地文档（`docs/superpowers/{specs,plans}/2026-10-30-b10-debt-observability-batch*`，
> 不入库）。

## B10 批次（后端侧） ✅

### 交付（每项一提交）

| 计划项 | 提交 | 内容 |
|--------|------|------|
| T0 | `7dbb9dc` | 新增 `agentj-core/src/process_group.rs`：`isolate(&mut Command)` + `ProcessGroup::{of,pid,kill,reap}` + `Drop`（kill → 预算内 reap，`/proc/<pid>/stat` 的 `Z` 状态判定）；`libc` 只在该模块 `#[cfg(unix)]` 内 |
| T1 | `bc8d352` | `ToolExecutor` 的 `ChildGuard` 委托 `ProcessGroup`，`libc` 从 `agentj-tools` 依赖消失 |
| T2 | `0f390ea` | `RunFailure::ProgramMissing { tool: Option<String>, .. }` + `ToolExecutor::with_tool`：启动失败报工具名而非程序名 |
| T3 | `f3675f0` | mcp 传输改走 `agentj_core::process_group`；`crates/agentj-mcp/src/process.rs` 保留为再导出 shim（`isolate_group` / `ProcessGroup` / `kill_group` + unix 回归测试），`libc` 从 `agentj-mcp` 依赖消失 |
| T4 | `0a71889` | `WebConfig::with_config_warnings` / `config_warnings()`；`/version` 新增 `config_warnings`（恒数组、`[]` 非 `null`、与横幅同一份 Vec） |
| T5 | `697896d` | `TcpBindOptions.config_warnings` → `build_web` → `WebConfig`；`agentj-cli serve_tcp` 接线；e2e `version_reports_the_same_config_warnings_the_banner_prints` 同时断言横幅与 JSON |
| T6 | `d5af175` | `/metrics` 新增 `agentj_web_config_warnings` gauge（只计数、不暴露文本），仍走 `push_metric` 唯一写出点且受 `auth_required` 门控（远程无 cookie → `401`，响应体无指标名） |
| T10 | `8524e71` | `agentj-protocol/src/permission.rs` 文档 + `schemas/PermissionRequestParams.json` 的 `cmd` → `command`（重新 `gen_schemas` 后仅该 schema 的一行描述变化）+ 两条防回退断言 |
| T11 | `4c3f84e` | `docs/specs/ipc-protocol.md` 的 bash 示例/字段表更正为 `command` / `timeout_ms`，并补记 `/version` 的 `config_warnings` 与 `/metrics` 的 `agentj_web_config_warnings`；`packages/sdk` 的 `progress.test.ts` 夹具同步 |
| — | `8f9a708` | `cargo fmt --all`（只含格式化，无逻辑变化） |

### 评审后修正（本次 review 发现并已修）

| 提交 | 内容 |
|------|------|
| `870b168` | **真回归修复**：`ProcessGroup::drop` 会在**成功**返回后杀整组——`ChildGuard` 字段在 `Drop::drop` 之后才被丢弃，`kill_on_drop = false` 拦不住它，于是「命令自己起的 dev server」在工具返回瞬间被 SIGKILL。新增 `ProcessGroup::disarm()`：成功路径先 disarm，只有失败 / 超时 / drop 才杀整组；`reap` 只把 `waitpid` 的 `ECHILD` 当已回收（`EINTR` 重试而非短路）；drop 内 250ms 内回收不了记 `tracing::warn!`（原先 `let _ =` 吞掉）。判别测试：`agentj-core` 的 `a_disarmed_group_survives_its_drop`、`reap_answers_true_for_a_pid_that_never_existed`，`agentj-tools` 的 `a_successful_run_leaves_background_children_alive`（真进程，不 mock） |
| `7476b8a` | sparkline 的「中位数顶点」改为**按值取中位**（原先取位置中点，名字与实现对不上），删掉 `vertices` 里恒真的区间过滤；新增判别用例 `locates the median by value, not by position`（`[10,90,20,30,95]` → 顶点 `[0,3,4]`，位置中点会给 `[0,2,4]`） |
| `5783a60` | 修掉 `integrity.rs` 的并行 flake：`build_router` 的抽检线程首 tick 立即触发，可能在 POST 之前就从一个 runtime 线程发出唯一那条告警（thread-local `capture_warnings()` 看不到）。改为断言契约——`warned` 已记录该路径、本轮 `offenses` 为空、捕获到的次数 `<= 1`；连跑 10 次 `cargo test -p agentj-ipc --lib web::tests` 全绿 |
| `768ebd0` | 删掉两条**不可能失败**的断言（protocol 与 sdk 里各自断言刚构造出来的 JSON 字面量）；schema 侧 `the_generated_schema_documents_command_not_cmd` 已覆盖文档注释与 wire 示例 |

### 实测门禁（本批收尾，2026-10-30）

| 门禁 | 结果 |
|------|------|
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 warning |
| `cargo test --workspace --locked` | **1285 passed**, 0 failed（评审修正后重跑） |
| `cargo test -p agentj-protocol -- drift` | 2 passed（schemas 无漂移） |
| `python3 scripts/tests/fn-length.py crates/` | 130 文件扫描、跳过 35 测试文件、**0 个 fn > 50 行** |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh all` | 3 + 6 + 10 = **19 项全 PASS** |
| `cargo deny check advisories licenses bans sources -D unmatched-skip` | rc=0（仅 `deny.toml` 里 `Zlib` 未被遇到的既有 warning） |
| `./scripts/check-version.sh` | `version-ok` |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | `desktop-ui-ok` |

### 本批新增测试要点（防回退的判别力）

- `process_group`：`kill_takes_down_the_grandchild_in_the_group`（组内孙进程一起死）、
  `drop_reclaims_the_group_and_reports_done`（`Drop` 后 leader 不再存活）、
  `reap_is_true_once_the_leader_is_gone`；mcp 侧保留
  `a_group_survives_its_leader_and_reports_when_it_is_gone`。三例都在真进程上跑，不 mock。
- `/version`：`version_reports_an_empty_config_warning_array_when_none_were_injected`（默认是 `[]`
  而非缺省/`null`）与 `version_reports_config_warnings_verbatim`（逐字、按序）。
- `/metrics`：`config_warning_count_is_a_gauge_registered_from_zero`（0 时也注册，含 `# TYPE`）
  以及 `remote_peer_without_cookie_cannot_read_the_config_warning_count`（远程 → `401` 且响应体
  里连指标名都没有）。
- 端到端那条 e2e 必须同时断言 stderr 横幅与 `/version`：只断言 JSON 无法发现「两处各算一遍」的漂移。

## 遗留问题 / 已知限制（后端）

- **`agentj-mcp/src/process.rs` 仍是 shim**：`isolate_group` / `kill_group` 只是再导出，`send_signal`
  未改用 `ProcessGroup`；等 mcp 传输层重写时可整体删除（保留它只为不打断既有调用点与回归测试）。
- **非 unix 的 `ProcessGroup::kill`/`reap` 是空操作**：Windows 下没有进程组语义，也没有可移植的
  「不回收地探活」调用（`alive` 恒 `false`）；超时的子进程树可能留活口，是**既有平台缺口**。
  仍持有 `Child` 的调用方用 `start_kill()` 兜底，但 `wait_with_output` 取走句柄后已无处可兜——
  `process_group` 的模块文档按此如实描述，不再宣称调用方「必须」能满足。
- **成功路径「存活」≠「立刻返回」**：命令若让后台子进程继续持有 stdout/stderr 管道，
  `wait_with_output` 仍会等到 EOF 才收尾（与改动前一致）；要常驻必须自己重定向。
- **`config_warnings` 只有启动时算一次**：运行期不会因配置热更新而变化；同理 gauge 值恒定。
- **`/metrics` 的 `Zlib` 许可未被任何依赖遇到**（`deny.toml:27`）：`license-not-encountered`
  当前是 warning；若 `cargo deny` 将来把它升为 error，需删除该条目。
- **`integrity` 并行 flake 已修**（`5783a60`）：根因是抽检线程首 tick 立即触发，与 POST 竞争
  「谁先告警」，thread-local 捕获看不到另一线程那条；现改为断言去重契约（`warned` 含路径 +
  `offenses` 为空 + 捕获次数 `<= 1`）。**注意**：`capture_warnings()` 仍是 thread-local，写新用例时
  不要跨线程断言日志条数。
- **`check-artifacts.sh linux` 本地 `rc=1`**：需要 `src-tauri/target/release/bundle/{deb,appimage}`
  产物，本地未打包；该断言属 CI `desktop-package` 作业，不在本地收尾门禁内。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑰⁺b | 真 BPE 词表 | 可插拔 `TokenCounter` 接缝 + 字符类校准权重已交付，权重仍是启发式；下一步按 provider 家族嵌入真词表（`tiktoken` 类），只换 `TokenCounter` 实现 | 需引入分词器依赖 / 体积权衡 | 待实施 |
| ㉟⁺b | 产物签名与公证 | 三平台产物已由 CI 真实构建并校验（`deb` / `AppImage` / `dmg` / `msi`），仍缺签名与公证（macOS notarize / Windows signtool） | 需证书与 CI secret 策略 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。
> 本批实施完毕并移除的条目（原表外候选）：把 `Config::validate()` 告警接进 `/version` 与
> `/metrics`、合并 MCP 与 `ToolExecutor` 两套进程组实现。

## 下一步

- 本批次（B10 后端侧 T0–T6 / T10 / T11）已收尾、过门禁并完成评审修正；分支推 `origin` 与
  `gitcode`，PR 开在 `origin`（用户负责合并）。
- 下一批候选（见上表）：⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证。
- 表外可选改进：删掉 mcp 的 `process.rs` shim 并把 `send_signal` 也换成 `ProcessGroup`；
  attach 支持回答具体选项。

> 远端提示：本机 `git fetch gitcode` 若报 `fatal: couldn't find remote ref gitcode`，按纪律
> 不 `--force`；分支照常 `git push gitcode <branch>:<branch>`，失败即如实记录。
