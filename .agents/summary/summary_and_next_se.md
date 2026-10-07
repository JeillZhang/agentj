# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.26.0 运行期切模型——后端部分**
> ——来源：设计 `docs/superpowers/specs/2026-10-07-agentj-runtime-model-switching-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-07-agentj-runtime-model-switching-plan.md`（T0–T23）。
> 后端范围：`agentj-protocol`（wire 字段 + 版本）+ `agentj-core`（`RunOverrides` /
> `ProviderRegistry::get` / `TurnRunner` 解析）+ `agentj-ipc`（`dispatch_session_run` 解析）；
> 加性字段，**无新方法（仍 21）/ 无新 schema（仍 52）**。
> `PROTOCOL_VERSION` 0.15.0 → **0.16.0**；工作区 0.25.2 → **0.26.0**。
> 前端部分见 `summary_and_next_fe.md`。

## v0.26.0（运行期切模型，后端部分）✅（本次完成）

- ✅ **协议加性字段**：`SessionRunParams` 新增可选 `provider` / `model`（`@since 0.16.0`、
  snake_case、缺省时 `skip_serializing_if` 不出现在 JSON）；`PROTOCOL_VERSION` 0.15.0 →
  **0.16.0**；`protocol/methods.json` 版本同步；`schemas/SessionRunParams.json` 内容重生成
  ——**无新方法（仍 21）、无新 schema（仍 52）**。`session/run` 仍只要求
  `[session_id, content]`，缺省路径与旧客户端逐字节相同。
- ✅ **`agentj-core`**：
  - 只读 `ProviderRegistry::get(name)`（按名查已注册 provider，不改动注册顺序/默认语义）。
  - 新 `RunOverrides { provider, model }`（`Default` = 全 `None`，即今天的行为）。
  - `KernelMessage::UserRequest.overrides` + `user_request_with_overrides` 构造器；
    3 参 `user_request` 保留为委托包装（既有调用点零改动）。
  - `TurnRunner::spawn_turn` 增加具名 `overrides` 参数；`run_turn` 解析：
    provider = `overrides.provider` → `get(name)`（**未命中即本轮失败**，文案
    `unknown provider: <name> (registered: ...)`，**绝不回落 `get_default()`**），
    否则 `get_default()`；model = `overrides.model` 否则 `config.model.default_model`
    否则 `deepseek-chat`；空白/纯空白视作「未给」。
- ✅ **`agentj-ipc`**：`dispatch_session_run` 用 `optional_trimmed` 解析两字段——
  缺失 / `null` / 空串 / 纯空白 → 视为缺省；非字符串 JSON 类型 → `invalid_params`
  （-32602）并在 `error.data.field` 指明字段。公开 2 参 `run_turn_stream` 保留
  （CLI `run` 不变），内部委托给带 overrides 的私有变体。
- ✅ **文档**：`docs/specs/ipc-protocol.md`（§8.1/§8.7/§9.3/§10/§11.1/§11.3）、
  新增 `docs/changelog/v0.26.md` + 索引、新增 `docs/guide/model-switching.md` + 索引，
  并订正 `docs/guide/subscription.md` / `docs/guide/tui.md` 中的陈旧表述。

## 验证结果（全量门禁清扫，2026-10-07）

> 本表由编排器在本阶段最终树上执行（T23 拥有 AC8/G6）。`cargo test --workspace` 以
> `-- --skip permission_request_notified_and_timeout_denies` 运行（见「遗留问题」）。

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo check --workspace --locked --quiet` | OK（lockfile fresh） |
| `cargo test --workspace -- --skip permission_request_notified_and_timeout_denies` | **740 passed / 0 failed（+1 skipped）** |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 626 passed（52 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 54 passed（6 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `bun --cwd agentj-ui/apps/tui test` | 157 passed |
| `./scripts/check-version.sh` | version-ok（**0.26.0** / `PROTOCOL_VERSION` **0.16.0**） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 遗留问题 / 已知限制（后端）

- **预存在的挂起 e2e（非本阶段回归）**：`crates/agentj-cli/tests/e2e_permission.rs` 的
  `permission_request_notified_and_timeout_denies` 在本环境会一直阻塞在 60s 权限超时上，
  未触碰的树上同样挂起——**不是本阶段引入的回归**，因此全量测试以
  `-- --skip permission_request_notified_and_timeout_denies` 运行（该 1 条已计入 skipped）。
- **CLI `agentj run` 不支持逐轮选模型**：按计划保留了 2 参 `run_turn_stream`，
  本地单轮 `run` 无 `--provider` / `--model` flag；只有走 IPC（`session/run` 带两个字段）
  的客户端（Web / TUI / 脚本）能覆盖。属加性范围之外，后续如需再补 CLI flag。
- **模型选择不进会话状态**：会话快照 / `SessionRunResult` / 历史消息 / 用量统计均不记录
  「本轮用的哪个模型」（设计如此，非缺陷）；选择只存在于客户端本地。
- **无 provider 目录新方法**：客户端仍从 `kernel/status.providers[].models` 取目录，
  本阶段未新增方法/schema（保持 21 方法 / 52 schemas）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **v0.26.0 运行期切模型（后端部分）已落地**，全量门禁清扫通过（740 passed / 0 failed，
  1 skipped 为预存在挂起 e2e）；待 PR 由用户合并 main。
- 后端候选：上表 ①②④；或 opencode-go 二期（`GET /zen/go/v1/usage` 用量面板 /
  device-code OAuth）；或为 CLI `run` 补 `--provider` / `--model` flag。
- 前端候选见 `summary_and_next_fe.md`。
