# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**工程治理批次**（配置 / 门禁 / 元数据 / 卫生，wire 面零变更）。

> 当前功能：**工程治理批次** —— 分支 `chore/engineering-governance-batch`（自 `5062736` 起）。
> 仓库断言 harness + CI 补全（deny / cross-platform / frontend）+ 覆盖率棘轮 + 依赖与许可治理 +
> 社区治理文件 + Cargo 工作区策略 + 版本元数据 + 工作区卫生 + 远端命名对齐。
> `crates/*` 运行期逻辑零改动，`PROTOCOL_VERSION` 仍 **0.19.0**、工作区仍 **0.1.0**。

## 工程治理批次 ✅

分支 `chore/engineering-governance-batch`。对应待实施 ⑮ ⑳ ㉛ ㉜ 的实施落地（本侧）；
㉑ ㉒ ㉕ ㉝ ㉞ 属前端侧，见 `summary_and_next_fe.md`。

### 交付

- **仓库断言 harness**（`scripts/tests/assert-ui.sh` / `assert-cargo.sh` / `assert-ci.sh`）：
  `--list` / `<check>` / `all` / `--strict` 四种入口，未实现项以 `SKIP`（exit 3）显式暴露、
  `--strict` 视为失败。16 个检查覆盖 UI 清单与单一 lockfile、workspace Cargo 策略与 18 crate
  继承、LICENSE 三件套、`deny.toml`、lockfile 一致性（`cargo metadata --locked`）、CI 作业契约、
  覆盖率棘轮、frontend 作业、治理文件、工作区卫生、两份 summary 与文档基线。
- **CI 作业补全**（`.github/workflows/ci.yml`）：新增 `deny`（cargo-deny 四类检查）、
  `cross-platform`（ubuntu / macOS / Windows，仅 `cargo build --workspace --locked`）、
  `frontend`（16 步：`npm ci` → protocol/sdk 生成漂移 → sdk/tokens/web 测试 → web 构建 →
  `dist/version.json` ↔ `protocol/methods.json` 协议版本一致性 → oxlint → TUI 测试 →
  desktop UI 校验 → `check-version.sh` → `check-relocate.mjs`）；`lint` 追加「Repo invariants」
  （`python3 -m pip install --user pyyaml` + 三份断言脚本 `all`）。
- **覆盖率棘轮**：实测行覆盖 **93.67%**（`cargo llvm-cov report`，2026-10-23：lines 36671/2321、
  regions 93.45%、functions 86.95%），`COVERAGE_THRESHOLD` 由 `50` → **92**
  （`max(50, floor(X) - 1)`，注释记录日期与基线）；`cargo llvm-cov` 与
  `cargo build -p agentj-plugin-fixture` 均加 `--locked`。
- **依赖 / 许可治理**：`deny.toml` —— advisories / licences / bans / sources 四类；licence 白名单
  14 项（MIT、Apache-2.0、BSD、ISC、Zlib、BSL-1.0、CC0-1.0、Unicode-3.0、Unlicense、MPL-2.0 等）；
  `multiple-versions = "deny"` + 6 条带来源注释的 `skip`（`getrandom` 0.2/0.3、`r-efi`、
  `rand_core` 0.6、`core-foundation` 0.9、`syn` 3）；`[sources]` 用 `allow-registry` 只放
  crates.io。`LICENSE` / `LICENSE-MIT` / `LICENSE-APACHE` 落地 **MIT OR Apache-2.0** 双许可。
- **社区治理**：`CONTRIBUTING.md`（本地门禁清单 + 不可提交项）、`SECURITY.md`
  （GitHub Security Advisories 上报通道、7 天响应）、`.github/dependabot.yml`
  （cargo / npm / github-actions，minor+patch 各聚合成一个 PR）。
- **Cargo 工作区策略**：`[workspace.dependencies]` 收敛 12 个三方依赖、`[workspace.lints.clippy]
  all = { level = "warn", priority = -1 }`、`[profile.release]` `lto = "thin"` +
  `codegen-units = 1`（**不设** `panic = "abort"`，保留 `catch_unwind`）、
  `[workspace.package] license = "MIT OR Apache-2.0"`、`clippy.toml` `msrv = "1.96"`；
  18 个 crate 全部改为继承式清单（依赖 `workspace = true`、`license.workspace = true`、
  `[lints] workspace = true`），42 处内部 path 依赖补 `version = "0.1.0"`（消除 wildcard 告警）。
- **版本元数据**：`scripts/check-version.sh` 新增 `AGENTJ_ROOT` 覆盖、`agentj-ui/package.json` /
  `apps/web` / `apps/tui` / lockfile 根条目断言、通用 `0.2x` 字面量守卫；失败打印出错文件、
  期望值与命中字面量（不再静默 `exit 1`）。`scripts/test-check-version.sh` 用临时目录夹具覆盖
  正/负样例。
- **工作区卫生**：删除 `agentj-ui/node_modules/.old_modules-*` 迁移残留、仓库根空 `node_modules/`、
  `.playwright-mcp/`；`.gitignore` 补 `.opencode/`（AI 工具运行时状态，机器本地）。
- **远端命名**：`origin` → GitHub（唯一权威）、`gitcode` → 只读镜像，与 `AGENTS.md`
  「Remote & Merge Policy」一致；分支按纪律推送两个远端，只在 `origin` 合并。

### 验证

| 命令 | 结果 |
|------|------|
| `cargo fmt --all --check` | FMT_OK |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 警告 |
| `cargo test --workspace --locked` | 48 组 `test result: ok`，无失败 |
| `cargo deny check advisories licenses bans sources` | advisories / bans / licenses / sources 全 ok |
| `cargo metadata --locked --format-version 1` | 通过（307 packages，`Cargo.lock` 无漂移） |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh --strict` | 全部 PASS，0 SKIP |
| `sh scripts/test-check-version.sh` / `./scripts/check-version.sh` | `check-version-tests-ok` / `version-ok` |
| 前端：sdk 55、protocol 4、tokens、web 681、TUI 192 + oxlint + `check-desktop-ui.mjs` + `check-relocate.mjs` | 全绿 |

### 遗留问题 / 已知限制（本批次）

- `cargo-deny` 不在开发机默认工具链（需 `cargo install cargo-deny --locked`，实测 0.20.2）；
  CI 走 `taiki-e/install-action@cargo-deny`。
- `frontend` 作业的 CI 首次运行尚未观测；本地已逐步跑通同一 16 步序列。
- 覆盖率基线 93.67% 为单次实测——后续若测试减少，棘轮会立即转红（预期行为，需重新基线化）。
- `cross-platform` 只做 `cargo build`，不做打包（dmg/msi 见待实施 ㉟）。
- **T22 远端收敛 BLOCKED**：`git push origin main` 被 GitHub 拒绝（非快进）——远端 `main` @
  `d883a79`（PR #90 合并）与本地 `main` @ `5062736`（!67 合并）是同一份内容的**两个 merge
  commit**，三端 tree 完全相同（`4e72b5a6b47df679d70489244abe70a3c59b5ed1`）。按纪律**未使用
  `--force`**、未改写任何远端历史。重命名已生效：`origin` = GitHub（权威）、`gitcode` = GitCode
  （只读镜像），`main` 已 `--set-upstream-to=origin/main`，`gitcode/main` 与本地 `main` 一致。
  收敛方式待用户决定：① 在 `main` 上 `git merge origin/main`（生成一个内容零变化的合并提交，
  之后 `git push origin main` 即为快进，不改写历史）；② 重置镜像或权威端的 `main`。

## 归档：run 可靠性批次 ✅

分支 `feature/run-reliability-batch`（自 `e924473` merge 后起）。六项特性 + 协议加性变更一次收尾，
对应待实施 ⑦⑧⑨⑭⑱⑲ 的实施落地；`PROTOCOL_VERSION` 升 **0.19.0**、工作区仍 0.1.0。

### 交付

- **工具结果落库上限**（待实施 ⑦）：`crates/agentj-core/src/budget.rs:147 fn truncate_chars` 按字符
  预算保留头部并追加 `…[已截断 {dropped} 字符，原文 {total} 字符；请缩小查询范围]…`
  （`budget.rs:24-25 TOOL_RESULT_TRUNCATION_NOTICE`）；唯一截断点 `turn_runner.rs:468`（构造
  `ToolResult` 处）；`[agent] max_tool_result_chars` 默认 `100_000`、钳制 `1_000..=1_000_000`
  （`crates/agentj-config/src/lib.rs`）；截断时发内核内事件
  `AgentEvent::ToolResultTruncated { tool_call_id, dropped_chars }`（`crates/agentj-core/src/types.rs:120`，不上 wire）。
- **`session/run` 回显生效 provider / model**（待实施 ⑧）：`AgentEvent::Done` 携带 `provider` /
  `model`（`crates/agentj-core/src/types.rs:135-136`）；wire `SessionRunResult` 加性新增
  `provider` / `model`（`crates/agentj-protocol/src/session.rs:60-65`，
  `#[serde(default, skip_serializing_if = "Option::is_none")]` → 未知时整键省略、绝不为 `null`）；
  轨迹新增 `TrajectoryValue::RunModel`（每轮至多一条）（`crates/agentj-protocol/src/trajectory.rs`，
  `crates/agentj-ipc/src/lib.rs` 记录）；`ProviderRegistry` 补 `set_default` / `default_name` /
  `get_default_with_name`（`crates/agentj-core/src/module.rs:487-507`）。
- **`glob` 路径语义（进程内执行）**（待实施 ⑨）：删 `find` 子进程，`crates/agentj-tools/src/glob.rs`
  进程内 `globset` + `walkdir`；pattern 不含 `/` 按路径分量 basename、含 `/` 按根相对全路径
  （`literal_separator(true)`：`*` 不跨 `/`、`**` 跨）；跳过目录沿用 `GrepTool::SKIP_DIRS`；输出
  `./<相对路径>` 逐行排序、上限 `2000`（超出追加 `# 已截断 N 条命中（上限 2000），请缩小 pattern`）；
  默认超时 60s、`timeout_ms` 可覆盖（钳制 1s–600s）；超时以 `glob timed out after Ns` 文本结果回报而非错误。
- **`ask` 默认超时**（待实施 ⑱）：`[agent] question_timeout_s` 默认 `600`、钳制 `1..=86_400`
  （`crates/agentj-config/src/lib.rs`）；CLI 注入 `QuestionModule::set_default_timeout_s`，未显式给
  `timeout_s` 的提问超时后按 **skipped** 结算（`crates/agentj-question/src/module.rs`），不再永久挂起。
- **CLI 审批与配置健壮性**（待实施 ⑲）：`run --yes` 自动批准并在 stderr 打印
  `auto-approved <tool> (--yes)`；TTY 下 `approve <tool>? <精简参数>` / `  reason: <原因>` /
  `[y/N] ` 打到 stderr、另起线程读 stdin（`y`/`yes` 批准，其余含 EOF 拒绝 → 拒绝理由
  `denied by user`）；非 TTY 且未加 `--yes` 不读 stdin、不挂审批器，待决请求按
  `[permission] timeout_ms` 预算自动拒绝；`timeout_ms` 钳制 `100..=600_000`（越界 stderr 警告、
  省略 = `60_000`）。
- **e2e 测试环境隔离**（待实施 ⑭）：共享 `TempEnv`（`crates/agentj-cli/tests/common/mod.rs`）把
  `HOME` / `XDG_DATA_HOME` / `XDG_CONFIG_HOME` 指向临时目录并移除凭据类环境变量，
  `e2e_cli` / `e2e_permission` / `e2e_auth` / `e2e_serve` 全部改用——测试不再读写真实 `~`。
- **协议版本**：`PROTOCOL_VERSION` **0.19.0**（仍 28 方法 / 78 schemas）；schemas 重新生成、
  `@agentj/sdk` `PROTOCOL_VERSION` 同步 0.19.0（`VERSION` 仍 0.1.0）。

### 验证（最终门禁）

| 门禁 | 结果 |
|------|------|
| `cargo test --workspace` | 全绿（`failed = 0`；新增 `glob` / 截断 / `session/run` 回显 / 提问超时 / CLI 审批 / e2e 隔离 测试） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `cargo fmt --all --check` | 干净 |
| `cargo test -p agentj-protocol -- drift` | 全绿（78 schemas 无漂移） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 55 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 681 passed（55 files） |
| `bun --cwd agentj-ui/apps/tui test` | 192 pass / 0 fail（22 files） |
| `./scripts/check-version.sh` | `version-ok`（T19 已把 `AGENTS.md` 与脚本断言同步到 0.19.0） |

### 真实提交（`e924473..HEAD`，最旧 → 最新）

- `fdf5421` test(e2e): add shared TempEnv helper and adopt it in e2e_cli
- `6e2685d` refactor(e2e): adopt shared TempEnv helper in permission, auth and serve suites
- `2817385` feat(tools): add in-process glob matcher and walker
- `51594ab` refactor(tools): run glob in-process and drop the find subprocess
- `f293f8e` feat(core): add char-budget truncation helper
- `06f0c4a` feat(config): add [agent] max_tool_result_chars
- `6d10033` feat(core): truncate oversized tool results once
- `9f0ae54` feat(config): add [agent] question_timeout_s
- `c8ef0cd` feat(protocol): add provider and model to SessionRunResult and a RunModel trajectory value
- `bc603e7` feat(question): make the default ask timeout configurable
- `15d5cd4` feat(core): report the effective provider and model on run completion
- `082b07d` chore(protocol): bump PROTOCOL_VERSION to 0.19.0
- `4bca0a9` chore(protocol): regenerate schemas and sync generated client types
- `0da5465` chore(sdk): sync PROTOCOL_VERSION with the 0.19.0 wire bump
- `effd538` feat(cli): add --yes and a stdin approver for interactive runs
- `a1f8125` test(ipc): restore partial-window paging coverage
- `f508471` feat(ipc): surface the effective provider and model on run replies
- `69074f2` feat(web): show the effective model reported by the kernel
- `f0b510e` docs: cover the run-reliability behaviour in specs, changelog and guides
- `306922c` docs: update the CLI guide and the (se/fe) summaries after the batch
- `f604697` docs: sync the AGENTS.md baseline and the version gate to protocol 0.19.0
- `b661b80` polish: address the accumulated review minors from the run-reliability batch
- `03dcc9e` fix: BUG-001 inject the question timeout default on every assembly path
- `bb52cd7` fix: BUG-002 clamp a negative permission timeout instead of failing the parse
- 本提交 docs: sync the config guide and the se summary after the acceptance fixes

### 遗留问题（非阻塞）

- **非 TTY 未加 `--yes` 仍等到 `timeout_ms` 才拒绝**：本次只消除「永久挂起」，未做「立即拒绝」——
  计划 AC 措辞「auto-denied instead of waiting」与实装「等 `[permission] timeout_ms` 预算耗尽再拒」
  不符，已按实装记录（guide 与 changelog 均为「按预算自动拒绝」）。
- **EOF 无独立「no input」文案**：计划/AC 写作 `Deny { reason: "no input" }`，实装里 EOF 只是映射为
  普通 `Deny`，最终拒绝理由是权限引擎的 `denied by user`（`crates/agentj-permission/src/module.rs:150`）。
- **`glob` 超时以文本结果回报**：`glob timed out after Ns` 是 Ok 文本而非 Err，模型看不到「工具超时」
  错误语义（与 grep/bash 的超时 Err 不一致）；属既有取舍，可选跟进。
- **`[permission] timeout_ms` 上界收窄为 `i64`**：验收 BUG-002 为让负值不被 serde 拒绝而把字段改为
  `Option<i64>`（`crates/agentj-permission/src/policy.rs:82`），因此 `> i64::MAX` 的 TOML 整数字面量在
  解析阶段报 `number too large to fit in target type`（修复前 `Option<u64>` 可接受再钳制）。量级约
  3.2 亿年，验收判 low 并**接受现状**；`docs/guide/configuration.md` 已写明取值须在 64 位有符号整数范围内。
- **`e2e_mcp` 会写真实数据目录**：偶发在真实 `~/.local/share/agentj` 留下 `metrics-state-<port>.json`，
  不在本次 ⑭ 改造的 4 个 e2e 文件范围（范围外观察项，未处理）。
- **TUI 测试改写真实配置**：`bun --cwd agentj-ui/apps/tui test` 会改写真实 `~/.config/agentj/theme.json`，
  属前端既有问题（本批次明确不触碰 TUI），未处理。

## 已完成（0.29.0 – 0.29.12 + BUG-017/018 + 抽检扩展 + search-rank 单一真源化 + run 可靠性批次，详见 `docs/changelog/v0.1.md`）

- ✅ run 可靠性批次（本阶段，见上「run 可靠性批次 ✅」）：工具结果落库上限 + `session/run` 回显
  生效 provider/model + `glob` 路径语义（进程内）+ `ask` 默认超时 + CLI 审批/配置健壮性 + e2e
  隔离；`PROTOCOL_VERSION` 0.19.0。
- ✅ search-rank 单一真源化（已合并 PR #89）：文件搜索排名收敛到 Rust `file_list::search_rank`
  唯一真源 + 跨语言契约夹具 `protocol/search-rank-cases.json`（Rust 契约测试 2 条 + TUI 镜像退役 +
  mock kernel 夹具回放）；TUI 192 passed、Web 679 passed（未触碰）、check-version ok。
- ✅ 抽检扩展（逐请求校验 + 手动全量端点）：`[web]` 4 键（`spot_interval_secs` / `spot_sample` /
  `verify_on_request` / `verify_cache_ttl_secs`）+ `IntegrityOptions` + 逐请求资产校验中间件
  （memoize + 绝对 TTL + 上限 1024、fail-open 计数 / 去重告警、未鉴权零哈希）+ `POST /integrity/verify`
  手动全量 + `/version` 加性 `integrity.on_demand` + 8 个加性指标；`serve` / `dashboard` 两路装配。
- ✅ 0.29.8：工具往返落库（每轮 `assistant{tool_calls}` + `tool` 结果写入会话，「继续」不再失忆）
  + 截断流（无 `Done` 的纯文本轮）重放 ≤2 次后如实报错；前端输入框 chip 跟随实际选择。
- ✅ 0.29.9：工具轮次用尽后追加一趟「无工具收尾」（`conclude_without_tools`），
  `[agent] max_tool_iterations` 从死配置变为可配（钳制 `1..=1000`）。
- ✅ 0.29.10：`text_tool_call` 把模型写成纯文本方言（`<rfl:parameter>` / `<tool_call>`）的
  工具调用还原成真正的调用；无法还原时纠偏 2 次后如实报错，而不是当结论。
- ✅ 0.29.11：Web 模型选择器不再提供虚构 `deepseek-chat` 占位（前端，详见 `_fe.md`）。

- ✅ 0.29.7：请求 token 预算（`budget.rs` + `ContextPipeline::fit` + 超限收紧重试）——长会话
  不再把整段历史原样发给上游后才被 400 拒绝；`[model] max_context_tokens` 可配（默认 128k）。

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。
- ✅ 0.29.4：真实 key 端到端调测适配（4 个上游缺陷 + 1 个测试挂起）——`catalog_source::bare_id()`
  去 `opencode-go/` 前缀；`responses` 文本/工具 delta 改「字符串优先、旧对象形状容忍」；
  新增 `retry::send()` 温度类 400 去参数重试一次；`gateway_error::api_error` 收敛三份私有副本
  + `429` → `RateLimited`；`[permission] timeout_ms` 使 e2e 审批超时降到 800ms，
  `e2e_permission.rs` 改用真实 `session_id` + 20s 截止通道（修复前**无限挂起**，现 0.85s）。
  真实 key 实测 9 模型 × 3 路由全绿（含 `glob` 命中唯一标记文件 `./UNIQUE_MARKER_7f3a.txt`）。
- ✅ 0.29.5：`serve` / `dashboard` 不再强制预先配置密钥（`providers::NO_USABLE_PROVIDER`
  只作为警告，`main.rs` 仅对无可录入客户端凭据的命令 fatal；`run` 行为不变）。
- ✅ 0.29.6：断连重连后的 `SessionBusy`——`settle_owner_pending` 在**每条**连接收尾路径
  （EOF / 传输错误 / shutdown）结算该 owner 的 question 与 permission 待决项（reason
  `disconnected`），`ConnectionRegistry::clear_owner` 在排空**前**撤销归属（排空期间新审批
  立即被拒），`attach_permission_notifier` 无 owner 可达即刻拒绝；Web 重连即重新
  `session/subscribe` + `SessionBusy` 友好文案；`pump_requests` 借用 `&mut Box<dyn Connection>`
  使连接活过排空（否则写方向被 shutdown，响应丢失）。

## 遗留问题 / 已知限制（后端）

- **抽检扩展 E4 的判别力**：`tokio::time::interval` 首 tick 立即触发，故 e2e 只能断言「15s 内出现
  **第二**轮」才真正证明 `[web] spot_interval_secs` 到达 `spawn_spot_check`（已按此实现）；若将来
  把抽检改成 `sleep` 语义需重新校准该断言。
- **`temp_dist` 固定 `/tmp` 目录**：`agentj-cli-e2e-web-{tag}` 仅按 tag 区分，同 tag 并发跑同一测试
  目标（或残留服务）会互踩；既有模式，未在本阶段改动。
- **fixture 基线重复**：`web_verify_off.toml` / `web_fast_spot.toml` 各自复制了
  `permission_deny.toml` 的完整基线（含 `timeout_ms = 800`），基线变更时需同步三处。
- **计划文本笔误（抽检扩展 T2）**：计划 AC4 写 `mismatched is 2`，正确值为 1（代码与测试是对的）。
- **远端命名修正（本工作副本实测）**：`github` = `git@github.com:JeillZhang/agentj.git`（**权威**）、
  `origin` = `git@gitcode.com:jeill/agentj.git`（GitCode **镜像**）——与 `AGENTS.md`「Remote & Merge
  Policy」的命名**相反**；feature 分支须推 `github` + `origin` 两个远端，PR 开在 `github`（待实施 ⑳
  的主题，本阶段不改文档，仅记录实测事实）。
- **文档滞后（本阶段有意未动 `AGENTS.md`）**：`AGENTS.md` 的 `agentj-config` 条目尚未列新增 `[web]`
  配置节，`agentj-ipc` 条目尚未提 `client_ip` / `metrics_state` 两个新模块（本阶段文档落在
  `docs/specs/ipc-protocol.md` + `docs/guide/http-and-auth.md` + `docs/changelog/v0.1.md`）；下次
  结构性触碰时补齐。
- **状态目录默认值文档不完整（实测）**：`docs/specs/ipc-protocol.md:58` 与
  `docs/guide/http-and-auth.md:63,82` 只写默认 `$XDG_DATA_HOME/agentj`，而
  `crates/agentj-cli/src/main.rs:146 fn metrics_state_dir_from` 的实装回退链为 `[web] state_dir`
  → `$XDG_DATA_HOME/agentj` → `$HOME/.local/share/agentj`（代码自身的启动警告亦写「set
  XDG_DATA_HOME, HOME or [web] state_dir」）；下次触碰该文档时补 HOME 回退。
- **错误路径收尾的测试只覆盖到 helper 内部**（终审 MEDIUM）：`crates/agentj-cli/src/main.rs` 的
  新测试 `shutdown_flushes_metrics_state_even_when_the_serve_loop_failed` 直接调用私有
  `shutdown_tcp_server`，因此**能**钉住「`run_result?` 不得早于 `flush_metrics_state()`」这一
  顺序回归（把 `?` 前移即 RED）；但它**不**驱动 `serve_tcp`，故若回归重新在调用点写回
  `tcp.run().await?`（`main.rs:447` 一类），该测试仍会通过。要覆盖这一层需让 `serve_tcp` 以
  可失败的 `run()` 端到端跑通（当前无该夹具），故仅记录、不再加测。
- **`[web] state_dir = ""` 未做配置校验**（验收实测）：`metrics_state_dir_from`
  （`crates/agentj-cli/src/main.rs:146`）对空 `$XDG_DATA_HOME` / `$HOME` 有过滤，但**显式空**
  `state_dir` 会返回 `Some(PathBuf::from(""))`；后果为每次 debounce flush 均失败（
  `warning: cannot persist login metrics state to metrics-state-<port>.json (io error: No such
  file or directory)` + `agentj_web_metrics_state_write_failures_total` 增长），不写文件、
  不改 cwd 权限、不崩溃。字面满足验收 AC（`<dir>` 即 `state_dir`），故未计 Bug；跟进项：
  加载配置时把空串归为 `None` 并复用「无状态目录」启动警告。
- **flush 与变更之间存在理论竞态（未复现）**：`flush_metrics_state`
  （`crates/agentj-ipc/src/web.rs:412-424`）先快照、后**无条件** `mark_clean()`；若某次限流变更
  恰好落在两者之间，该变更会被当作「已持久化」。可由下一次变更或关机时的无条件 flush 补回，
  仅「在该窗口内 SIGKILL」才真正丢一次增量；因无法确定性复现，故仅记录。
- **预算为近似估算**：`estimate_tokens` 用「ASCII / 4 + 非 ASCII × 1」的字符启发式，非真实
  tokenizer；目录里**没有 per-model 上下文窗口元数据**（`schemas/ModelEntry.json` 仅
  format/id/verified），因此默认 128k 是保守假设，真窗口更小的模型仍可能被上游拒绝一次
  （此时由 `retry_budget` 按上游窗口收紧重试，最多 2 次）。
- **裁剪即丢失**：超预算时最老的会话历史与（同轮内）较早的工具结果会被丢弃，正文超长时中段
  被截断；仅在日志/行为上可观测，UI 无「已裁剪」提示。
- **文本方言还原只按参数名推断**（0.29.10）：泄漏模型已确认为 **`beam-free`**（用户
  2026-10-17），但泄漏随机、无法稳定复现（前缀回放 / 完整 harness run 都返回原生
  `tool_calls`），因此还原能力靠**参数名**推断工具（`command`→bash 等）；多参数调用（如
  write 的 path+content）与未知参数名仍走「纠偏重发 → 如实报错」。
- **grep 回退不读 `.gitignore`**（0.29.12）：无 `rg` 时的进程内检索只跳过固定目录
  （`.git`/`target`/`node_modules` 等），不解析 `.gitignore`；命中结果仍受 2000 条上限约束。
- ~~`e2e_ws` 端口竞态~~：0.29.7 已修复（`serve --port 0` + 读回绑定地址；并行复现 1/3 → 3/3）。
- **2 条 legacy 路由**：`qwen3.6-plus` / `qwen3.7-max` 按 models.dev 判为 `openai-compat`
  （旧手写表标 `anthropic`），二者均不在官方在售列表内；如实测不符，加回一条种子即可。
- **温度降级只重试一次**：`retry::send` 仅处理「仅 temperature 不支持」的 400；若模型同时拒绝
  其他参数，仍按原错误上抛（不盲目剥参）。
- **凭据时序**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间进行中的那一轮 run
  可能仍使用旧凭据，新凭据从下一轮生效。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 即远端主机）；
  非回环 `serve --host` 必须配 `--token`。
- **进程型工具报错里的 `name` 变成程序名**：`NotFound` / spawn 失败时 executor 把
  `ProgramMissing.name` 填成**程序**（`crates/agentj-tools/src/executor.rs:27-30`：bash → `sh`、
  grep → `rg`），而改动前填的是工具名（`bash` / `grep`）；管道读失败则是 `"child"`
  （`executor.rs:71,78`）。错误码与变体未变（无 wire 影响），但与设计 G4「错误面不变」有偏差，
  属可选跟进。（`glob` 已进程内化，不再 spawn `find`，故不再有 `glob → find` 映射。）
- **`ToolExecutor::run_blocking` 只能在阻塞线程里调**：它自建 runtime 后 `block_on`
  （`crates/agentj-tools/src/executor.rs:220-237`），当前唯一生产调用方是 `spawn_blocking`
  （`crates/agentj-core/src/module.rs:451`，线程未设 `EnterRuntime::Entered` 故安全）；若将来在
  异步上下文（多线程 worker / `block_in_place`）直接调 `Tool::execute` 会 panic。
- **MCP 子进程链路仍未接线**：`crates/agentj-mcp/src/connection_manager.rs:222` 仍是裸
  `tokio::spawn` 管理子进程，未走 `ToolExecutor` 的进程组 / 超时 / drop 回收；MCP server 挂起
  同样不会被杀整组（后续跟进）。
- **bash 默认 120s 对冷编译偏紧**：Rust 全量 `cargo build` / 大测试套件可能超过 120s；模型可用
  入参 `timeout_ms`（上限 600s）覆盖，默认值是否上调待观察。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中；两个 auth 方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑥ | 用量历史（服务端持久化 + 趋势） | `provider/usage` 由按需 TTL 300s 扩为服务端持久化采样 + 历史/趋势查询（协议加性）、Web/TUI 趋势视图与保留策略 | 无（基于 v0.27.0 用量面板；设计需先定采样归属/保留/加性 wire） | 待实施 |
| ⑩ | 【债】IPC 协议层拆分 | `crates/agentj-ipc/src/lib.rs`（实测 **7,339 行 / 295,749 B**）按 dispatch 域拆为 `dispatch_session` / `dispatch_permission` / `dispatch_question` / `dispatch_file` / `dispatch_provider` 等子模块；`impl IpcServer`（行 486–1931，约 **1,445 行**、30+ 方法）切分为多个 impl 块或独立 handler | 无（纯重构，wire 面不变） | 待实施 |
| ⑪ | 【债】超长函数治理 | 实测 lib 代码 831 个函数中 **26 个 >50 行**（违反 `AGENTS.md` 「function ≤50 lines」）：`crates/agentj-cli/src/main.rs:466 fn main`（**379 行**）、`crates/agentj-core/src/turn_runner.rs:189 fn run_turn`（**362 行**）、`crates/agentj-ipc/src/lib.rs:861 fn dispatch_session_run`（**252 行**）、`crates/agentj-ipc/src/lib.rs:1420 fn dispatch_permission_respond`（80 行）——按域抽子函数 | 与 ⑩ 同批执行 | 待实施 |
| ⑫ | 【债】锁中毒与 panic 隔离 | 非测试代码 **170 处 `.lock().unwrap()`**：内核是常驻多客户端守护进程，任一线程持锁 panic → mutex 中毒 → 后续每次取锁全部 panic（单会话 bug 可升级为整内核不可用）。改 `unwrap_or_else(|e| e.into_inner())` 或统一封装；请求入口（`agentj-ipc` dispatch / `turn_runner`）补 `catch_unwind` + 错误响应（现全仓仅 `agentj-core/src/hooks.rs:117`、`agentj-question/src/module.rs:356` 两处隔离；另 `crates/agentj-ipc/src/web.rs:257` 的 `std::sync::RwLock` 用 `web.rs:453/457/460` 的 `.read()/.write().expect("sessions")`，是同类中毒路径） | 无 | 待实施 |
| ⑬ | 【债】permission / question 待决机制收敛 | `crates/agentj-permission/src/module.rs` 与 `crates/agentj-question/src/module.rs` 是同一套 pending / owner / settle 的双份镜像（`new` / `set_notifier` / `respond` / `session_of` / `cancel_session` / `clear_all` + `PENDING_TTL` 一一对应）；抽出共用 `PendingRegistry`，消除「修一处漏一处」（BUG-017 已证明代价） | 无 | 待实施 |
| ⑯ | 【债】crate 粒度收敛 | `agentj-context` 87 行 / `agentj-stats` 133 / `agentj-skills` 132 / `agentj-license` 149 均为 <150 行的独立 crate，而 `agentj-ipc` 单 crate 12,514 行；评估合并或降级为模块（触及 `agentj-cli` 装配与 CI） | 无 | 待实施 |
| ⑰ | 模型元数据与 token 预算 | `schemas/ModelEntry.json` 补 `context_window`（加性 wire），把 `estimate_tokens` 的「ASCII/4 + 非 ASCII×1」字符启发式换成真 tokenizer，或至少按模型真实窗口裁剪（现默认 128k 是保守假设） | 取决于上游目录是否提供窗口字段 | 待实施 |
| ㉗ | 【债】错误处理约定与生产 `expect` | lib 代码 `let _ =` **58 处**（已剔除 `#[cfg(test)]`；报告所称 222 含测试），多数是合理「尽力而为」却无统一约定与注释，审计成本高；生产路径 `crates/agentj-ipc/src/web.rs:595/642 .expect("valid cookie header")`、`web.rs:627 .expect("retry-after")` 本可 `if let` 处理 | 无 | 待实施 |
| ㉘ | 【修复】Web 登录门与 cookie 安全细节 | ① `crates/agentj-ipc/src/web.rs:587 if !expected.is_empty() && form.token == expected` 非常量时间比较（低成本可换 `subtle::ConstantTimeEq`，限流已缓解但非等价）；② `web.rs:593/640` 会话 cookie 仅 `HttpOnly; SameSite=Lax; Path=/`，**缺 `Secure`**（https 部署下明文回放） | 无 | 待实施 |
| ㉙ | 【债】工具侧路径校验弱于 IPC 侧 | `crates/agentj-ipc/src/file_list.rs:71/126/331` 用 `canonicalize` + 前缀比对（真沙箱），而 `crates/agentj-permission/src/engine.rs:76 escapes_root` = `lexical_normalize` + `starts_with`、`crates/agentj-core/src/tool_paths.rs:16 rebase_tool_args` 亦纯词法归一化 → **符号链接可绕过**；bash 只设 `current_dir`，模型可 `cd /` 后任意操作（无 seccomp/namespace 硬隔离，安全性完全押在权限系统 + 用户审批上）→ 统一到 canonicalize 或明确文档化该边界 | 无 | 待实施 |
| ㉚ | 【债】SSE 事件分派三份重复 | `crates/agentj-model/src/responses.rs:141`、`anthropic.rs:140`、`openai_compat.rs:188` 各一份 ~96–101 行 `push_line`（字节流→行的组装已抽到 `sse.rs`，**事件分派层未抽**）→ 新增 provider 需再抄一遍 | 无 | 待实施 |
| ㉟ | 【债】Desktop 安装包构建验证 | `cross-platform` 作业目前只做 `cargo build --workspace --locked`；`agentj-ui/apps/desktop` 声称 deb/AppImage/dmg/msi 四件套，但 `tauri build` 与 `check-artifacts.sh` 从未在 CI 执行（macOS/Windows 打包链路无回归保护） | 需 macOS/Windows runner + 签名/公证策略决策 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- 工程治理批次代码 / 门禁 / 文档 / summary(se/fe) 已收尾；剩余收尾动作：T21（同步 `AGENTS.md` /
  `README.md` 的门禁清单与本文件基线）→ T22（远端重命名：`origin` = GitHub 唯一权威、
  `gitcode` = 只读镜像，并把镜像快进到与 `origin/main` 相同的 tree）→ 分支推送两个远端 →
  进入测试验收（`jskill-test-acceptance`），通过后在 `origin` 开 PR（用户负责合并）。
- 下一特性候选：待实施 ⑥「用量历史服务端持久化」（特性）；或上表 ⑩–⑬、⑯、⑰、㉗–㉚、㉟
  （工程债 / 修复）；或前端跟进见 `summary_and_next_fe.md`（含 ③ Web 接入服务端搜索 + ㉓ ㉔）。
- 可选跟进（run 可靠性批次遗留，非阻塞）：非 TTY 未加 `--yes` 改为**立即**拒绝（现等
  `timeout_ms` 预算）；`glob` 超时由文本结果改为结构化 Err；把「工具结果已截断」作为可观测事件
  透出（当前仅内核内 `AgentEvent::ToolResultTruncated`）。
