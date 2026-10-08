# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.28.0 CLI `run` 的 per-run 模型/provider 覆盖（后端部分）**
> ——来源：设计 `docs/superpowers/specs/2026-10-08-agentj-cli-run-model-override-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-08-agentj-cli-run-model-override-plan.md`（T0–T10）。
> 后端范围：`agentj-ipc`（1 个 pub turn 入口）+ `agentj-cli`（2 个 run flag + 两条路径 + 失败退出码）
> + `docs/guide/` + `docs/changelog/` + 版本元数据同步。
> `PROTOCOL_VERSION` **不变**（0.17.0，26 方法 / 72 schemas）；工作区 0.27.1 → **0.28.0**。
> 前端 surface 零变更（仅版本元数据同步），见 `summary_and_next_fe.md`。

## v0.28.0（CLI run per-run 模型/provider 覆盖，后端部分）✅（本次完成）

- ✅ **`agentj-ipc`**：新增公开入口 `IpcServer::run_turn_stream_with(sid, input, RunOverrides)`
  （中性命名）；既有 `pub run_turn_stream` 退化为 `RunOverrides::default()` 薄封装；
  原私有 `run_turn_stream_with_overrides` 收拢为公开实现，`dispatch_session_run` 改用新名。
  **无新依赖、无 dispatch/ProviderService 变更**；3 条新单测钉住显式 provider 路由、
  默认路径不变、空白 override 视为缺省。
- ✅ **`agentj-cli`（flag 表面）**：`Commands::Run` 新增 `--provider <NAME>` / `--model <MODEL_ID>`
  （均 `Option<String>`、长选项、相互独立、无 clap 交叉约束）。CLI **不做** trim/归一，
  空白值原样透传由内核判定（设计 D6）。
- ✅ **`agentj-cli`（embedded 路径）**：构造 `RunOverrides` 调 `run_turn_stream_with`；
  失败时先 `signal_shutdown()` + `mcp_manager.close_all()` 清理，再 stderr 一行
  `error: <文本>` + 退出码 `1`（此前为打印空行 + 退出码 `0`）；成功路径 `println!` 是最后输出。
- ✅ **`agentj-cli`（`--attach` 路径）**：`run_params_json` 仅在 `Some` 时写入 `provider`/`model`
  键（不写 `null`/`""`），`content` 转义交给 serde_json；新增 `run_error_line` 分支——
  `-32603` → `error: <message>`、其它错误码 → `error: <message> (code <code>)`，
  **不再吞掉 JSON-RPC `error` 而永久挂起**；两条路径退出码对齐为 `1`。
- ✅ **文档**：`docs/guide/cli.md`（`agentj run` flag 表 + embedded/`--attach` 示例 + 空白/失败语义）、
  `docs/guide/model-switching.md`（新增「CLI」per-run 覆盖小节，保持「未知 provider 失败不回落」口径）、
  新建 `docs/changelog/v0.28.md` + 索引行。
- ✅ **版本元数据**：`Cargo.toml`/`Cargo.lock`、SDK `VERSION`、desktop 三清单、
  `package-lock.json`/`bun.lock`/`src-tauri/Cargo.lock`、`scripts/check-version.sh` 全部同步 0.28.0；
  `PROTOCOL_VERSION` 保持 0.17.0。

## 验证结果（全量门禁清扫，2026-10-08）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo test --workspace -- --skip permission_request_notified_and_timeout_denies` | 812 passed / 0 failed |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `git diff --exit-code -- schemas protocol/methods.json docs/specs/ipc-protocol.md` | 空（零 wire 变更） |
| `./scripts/check-version.sh` | version-ok（**0.28.0** / `PROTOCOL_VERSION` **0.17.0**） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 54 passed |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 代码评审（Phase 4，`ajteam-task-reviewer` 逐任务只读审查）

- T0–T10 全部 **VERDICT PASS**（T0 ipc 入口、T1 embedded flag、T2 attach overrides + error 分支、
  T3 指南、T4 changelog、T5 工作区版本、T6 SDK、T7 desktop 清单、T8 lockfile、
  T9 版本门禁、T10 基线/摘要同步）。
- 无阻塞性偏差；实现与设计文档 D1–D8、G1–G6 一致。
- Phase 5 全分支审查：无 CRITICAL / HIGH；1 项 MEDIUM（`--attach` 连接失败经 `?` 提前返回、
  未先 `close_all()`）经确认为**分支前既有形态**且设计已明确划出范围，仅记录不改。

## 验收（`jskill-test-acceptance`，2026-10-08）

- 单轮 **零 Bug**（critical 0 / high 0 / medium 0 / low 0）——无修复提交、无回归轮次。
- tester 门禁全绿，探索式 QA 覆盖 embedded / `--attach` 两条路径、未知 provider 不回落、
  空白=缺省、clap 用法错误（exit 2）、连接失败不挂起、破坏性输入无 panic、`--attach` wire
  键省略、错误码映射（`-32603` vs `(code X)`）、文档-行为一致性；证据见本地
  `.superpowers/acceptance/bugs.md`。
- 零修复故未触发产品 patch bump；版本保持 **0.28.0**。PR 由验收流程在 `origin` 创建（用户负责合并）。

## 遗留问题 / 已知限制（后端）

- **预存在的 e2e 挂起（非本阶段回归）**：`crates/agentj-cli/tests/e2e_permission.rs`
  的 `permission_request_notified_and_timeout_denies` 在无外部交互时会挂起，
  全量测试统一以 `-- --skip permission_request_notified_and_timeout_denies` 运行。
- **CLI 级无「模型被使用」的直接证据**（设计已记录）：唯一 key-free 的脚本化 `MockStateful`
  忽略 `ModelOptions`，故 CLI e2e 只证明 flag 贯通与 turn 正常收尾（配置须带
  `[permission.tools.bash] mode = "allow"`）；路由正确性由 `agentj-ipc` / `agentj-core` 层单测钉住。
- **`--attach` 连接失败路径文案未统一**（设计已记录）：仍是 `anyhow` 默认 `Error: …` 前缀，
  仅新增 JSON-RPC `error` 分支；非目标。
- **退出码只区分 0/1/2**：不细分运行失败 / 连接失败 / 协议错误。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |
| ⑥ | 用量历史（服务端持久化 + 趋势） | `provider/usage` 由按需 TTL 300s 扩为服务端持久化采样 + 历史/趋势查询（协议加性）、Web/TUI 趋势视图与保留策略 | 无（基于 v0.27.0 用量面板；设计需先定采样归属/保留/加性 wire） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **v0.28.0 CLI run per-run 覆盖已落地**，全量门禁清扫通过；待 PR 由用户合并 main。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
