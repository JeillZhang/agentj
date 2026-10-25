# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**合并收口批次（后端侧）**——wire 面零变更（`PROTOCOL_VERSION` 仍 `0.20.0`，
29 方法 / 81 schemas），工作区版本仍 `0.1.0`，`schemas/` 与 `protocol/methods.json` 未改。

> 当前功能：**tokenizer 接缝 / 三平台打包 / attach 审批 / grep 时限 合并收口** —— 分支
> `feature/tokenizer-packaging-attach-grep`，计划 17 任务（T0–T16）。
> 设计与计划是本地文档（`docs/superpowers/{specs,plans}/2026-10-25-tokenizer-packaging-attach-grep-*`，
> 不入库）。

## 合并收口批次（后端侧） ✅

### 交付

**A. 预算层可插拔 `TokenCounter` + 分类校准（待实施 ⑰⁺a，`crates/agentj-core/src/budget.rs`）**

- 新增接缝 `pub trait TokenCounter { fn count_text(&self, text: &str) -> usize; }`（`budget.rs:38`）
  与默认实现 `HeuristicTokenizer`（`budget.rs:44`）：真 BPE 词表落地前，按**字符类**计费——
  ASCII 字母/空白 4 chars/token、ASCII 标点 2.5、CJK（Han/Hiragana/Katakana/Hangul/兼容表意）
  1.25、其余 BMP 1、星平面 0.5（`astral * 2`）。旧启发式（ASCII/4 + 非 ASCII×1）对中文与标点
  严重低估，新权重保证**每一类至少和旧值一样高**（回退安全），`estimate_tokens` 仍等于
  `HeuristicTokenizer.count_text`（签名不变，既有调用点零改动）。
- 旁路辅助：`message_tokens_with` / `request_tokens_with`（`budget.rs`）接受任意 counter，
  原 `message_tokens` / `request_tokens` 委托给 `HeuristicTokenizer`。
- 共享反比常量 `pub const CHARS_PER_TOKEN: usize = 3`（`budget.rs:86`）替代工具调用收缩器里
  硬编码的 `4`：`shrink_calls` 走同一常数，估算侧与裁剪侧不再各写一个换算率。

**B. 工具定义占用预算（`budget.rs` + `ContextPipeline::fit_with_tools` + turn runner）**

- `definitions_tokens(defs)`（`budget.rs:124`）= 每个定义 `name + description + input_schema`
  的 token 数 + `TOOL_DEF_OVERHEAD`（12）框架开销；定义**永不**被裁剪器丢弃，但确实占窗口，
  因此必须在裁剪**之前**预留。
- `ContextPipeline` 新增默认方法 `fit_with_tools(messages, defs, budget)`（`module.rs:88`）：
  默认实现等价于 `fit(messages, budget - definitions_tokens(defs))`（下限 1，避免零预算），
  自定义 pipeline 无需改动即获得新语义。
- `TurnRunner` 组装请求时改调 `fit_with_tools`（`turn_runner.rs:579`），工具定义不再挤掉历史后
  才被上游以 400 拒绝。

**C. `grep` 进程内回退受同一 deadline 约束（待实施 ㊲⁺，`crates/agentj-tools/src/builtins.rs`）**

- 无 `rg` 时走的 `GrepTool::search_native`（`builtins.rs:265`）现在自带
  `Instant::now() + timeout` 期限：每进入一个 walked entry 检查一次，超期即
  `Err(ToolError::Timeout(timeout))`（与 `bash` / `rg` 一致的结构化超时，不再是一段成功的文本）。
- `scan_entry`（同文件）亦持 deadline，大文件逐行扫描不会单点跑穿预算；2000 条命中上限行为不变。
- 前提在 `run_with_rg` 的注释里写明：`rg` 子进程消失后，调用方已钳制的 deadline 是**唯一**预算。

**D. `run --attach` 不再对远端审批装死（待实施 ㊱⁺，`crates/agentj-cli/src/main.rs`）**

- attach 帧解码收敛到单一动作解析，收到 `$/permissionRequest` 时打印
  `warning: run --attach cannot answer approvals; denying {tool} ({request_id}) — ...`
  （`main.rs:750`）并主动回一条 `permission/respond` deny（`attach_deny_request`，`main.rs:756`；
  序列化由 `serde_json` 完成，避免手拼 JSON）。此前 attach 既不答复也不报警，只能等远端
  `[permission] timeout_ms` 到期。
- 归属语义不变：`--attach` 的审批归远端内核的订阅客户端，本进程只是「如实报告并拒绝」。

**E. 三平台打包作业（待实施 ㉟⁺，`.github/workflows/ci.yml`）**

- 新增 `desktop-package-macos`（`macos-latest`）与 `desktop-package-windows`（`windows-latest`）：
  各跑 `npm ci` → protocol / sdk 类型生成 → web 构建 → `tauri build --bundles dmg|msi` →
  `check-artifacts.sh macos|windows`（`shell: bash`）→ 上传 `agentj-desktop-macos` /
  `agentj-desktop-windows`；`timeout-minutes: 60`，跑测试套件仍只在 Linux 作业之外的那套里。
- 两个作业都**只在** PR 带 `desktop-package` label 或手动 dispatch 时运行
  （`if: github.event_name == 'workflow_dispatch' || contains(github.event.pull_request.labels.*.name, 'desktop-package')`），
  避免每个 PR 都付 macOS/Windows runner 的成本。

**F. 仓库不变量同步（`scripts/tests/assert-ci.sh`，17 → 19 个 check）**

- 新 check `desktop-package-platforms`：断言两个作业存在、runner 正确、`timeout-minutes <= 60`、
  打包 bundle 与平台校验脚本匹配、校验步骤用 `shell: bash`、不带 `cargo test`、带 label 门控、
  上传名与 `retention-days: 7` / `if-no-files-found: error` 正确。check 清单自检
  （`CHECKS` ↔ `EXPECTED_CI_CHECKS` ↔ `--list`）同步到 10 个 ci check。
- `check_docs_summaries` 的 `se_keep` 收窄为 `["⑰⁺"]`（㉟⁺/㊱⁺/㊲⁺ 本批已实施），
  仍要求待实施表出现 `dmg` / `msi` 两词元。

**G. 空 `[web] state_dir` 视为未设置（`crates/agentj-cli/src/main.rs`）**

- `metrics_state_dir_from` 对显式空串加 `.filter(|dir| !dir.as_os_str().is_empty())`：
  `PathBuf::from("")` 不是目录，此前却赢得解析链，把 metrics/usage 指向会解析到进程 CWD 的路径，
  导致每次 flush 都失败（`cannot persist login metrics state ...`）。现在回落到
  `$XDG_DATA_HOME/agentj` → `$HOME/.local/share/agentj`，两者皆无时返回 `None` 并复用
  「无状态目录」启动警告。

### 测试（新增 26 项 e2e/单测）

- core：`TokenCounter` 分类校准（ASCII / 标点 / CJK / 星平面各自的边界值）、
  `definitions_tokens` 覆盖 name/description/schema + overhead、`fit_with_tools` 等价于
  `fit(budget - definitions_tokens)` 且零预算有下限、`CHARS_PER_TOKEN` 与收缩器一致。
- tools：grep 进程内回退在 deadline 到点时返回 `Timeout`（不返回部分结果）。
- cli 单测：`attach_deny_request` 序列化出 `PermissionDecision::Deny`、attach 告警行文案、
  `state_dir_empty_string_is_not_a_path` / `state_dir_empty_string_without_env_warns`。
- e2e 新文件 `crates/agentj-cli/tests/e2e_attach_ownership.rs`（3 例）：attach 报告并拒绝远端审批
  （stderr 命中 `warning: run --attach cannot answer approvals`，且远端的 `timeout_ms = 30000`
  远大于测试 10s deadline，证明告警来自 attach 而非超时）；owner 断连后第二客户端重新认领并批准
  （`session/subscribe` 成功 + `session/run` 的 `claim_run_owner` 完成归属转移）；非 owner 的
  `permission/respond` 被 `NOT_AUTHORIZED` 拒绝且 owner 随后仍能收尾。
  > 注意：`serve` 在**最后一个**连接断开时收尾（resident 决策 #3），所以「断连后重新认领」的夹具
  > 必须**先**连上第二个客户端再 drop owner，否则内核直接关机、第二次写入 broken pipe。

### 门禁

| 门禁 | 结果 |
|------|------|
| `cargo test --workspace --locked` | 全绿，1247 项（含 3 项新 e2e） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 警告 |
| `cargo fmt --all --check` | 干净 |
| `python3 scripts/tests/fn-length.py` | 0 函数 > 50 行 |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh all` | 3 / 6 / 10 passed，0 failed（共 19 个 check） |
| `scripts/validate-workflow.sh .github/workflows/ci.yml <9 jobs>` | PASS |
| `./scripts/check-version.sh` | `version-ok`（版本与协议版本均未变） |

### 提交（`origin/main..HEAD`，最旧 → 最新）

- `ee0b3f8` feat(core): add TokenCounter seam with calibrated character classes
- `d5640e9` feat(core): reserve tool definition tokens in the request budget
- `06c5fec` refactor(core): share one chars-per-token constant with the tool-call shrinker
- `db49c23` feat(core): fit requests against the budget left after tool definitions
- `2f73e86` fix(tools): bound the in-process grep fallback by the caller's deadline
- `2720802` feat(cli): deny remote approvals from run --attach instead of hanging
- `eb97702` test(cli): share the resident-server socket client across e2e binaries
- `89b2e78` test(cli): cover attach approval denial and cross-client approval ownership
- `ad74450` test(ci): pin the macOS/Windows desktop packaging jobs
- `f08693f` ci: bundle dmg and msi on label-gated macOS/Windows runners
- `080e8b9` fix(cli): treat an empty [web] state_dir as unset
- `9a32579` style: cargo fmt after the tokenizer/budget edits
- `0340ae1` test(e2e): keep a second client connected while the owner drops
- 本提交 docs: 同步 changelog / guide / summary / AGENTS.md 基线

## 遗留问题 / 已知限制（后端）

- **仍是启发式估算**：`HeuristicTokenizer` 按字符类加权，不是真 BPE 词表——它把「中文/标点被
  低估 2–4 倍」这一结构性问题压了下去，但逐 provider 的词表差异仍未刻画；`retry_budget`
  （上游超限收紧重试，最多 2 次）继续保留为兜底。见待实施 ⑰⁺b。
- **工具定义只做「预留」不参与裁剪**：`fit_with_tools` 只在预算里扣掉定义开销；若定义本身就
  超过窗口（超多 MCP 工具），`fit` 拿到的预算会被下限钳到 1，请求仍会超限——该情形目前靠上游
  400 + `retry_budget` 收尾，没有「按需裁工具」的策略。
- **`CHARS_PER_TOKEN = 3` 是保守反比**：`HeuristicTokenizer` 对标点按 2.5 chars/token 计费，
  收缩器取 3 表示「每个 token 至少对应 3 字符」，因此收缩后的字节数可能比允许的更小（更保守，
  不会超预算）。
- **attach 侧仍是单向**：`run --attach` 只会 deny，不能批准，也不能应答 `$/askQuestion`
  （提问只能等远端 `[agent] question_timeout_s` 或 owner 客户端回答）。见待实施 ㊱⁺b。
- **`dmg` / `msi` 未签名未公证**：作业只构建并校验产物存在；macOS 用户需右键打开或
  `xattr -d com.apple.quarantine`，Windows 会撞 SmartScreen。见待实施 ㉟⁺b。
- **两个新作业的首次 CI 运行尚未观测**：本地用 `scripts/validate-workflow.sh` 与
  `assert-ci.sh desktop-package-platforms` 静态校验结构，真实 runner 上的耗时/失败模式待首跑。
- **`temp_dist` 固定 `/tmp` 目录**：`agentj-cli-e2e-web-{tag}` 仅按 tag 区分，同 tag 并发跑同一
  测试目标（或残留服务）会互踩；既有模式，未在本阶段改动。
- **fixture 基线重复**：`web_verify_off.toml` / `web_fast_spot.toml` 各自复制了
  `permission_deny.toml` 的完整基线（含 `timeout_ms = 800`），基线变更时需同步三处；
  本批新增的 `attach_ask.toml` 同理（`timeout_ms = 30000` 是刻意远大于测试 deadline）。
- **进程型工具报错里的 `name` 变成程序名**：`NotFound` / spawn 失败时 executor 把
  `ProgramMissing.name` 填成**程序**（`crates/agentj-tools/src/executor.rs:27-30`：bash → `sh`、
  grep → `rg`），而改动前填的是工具名；管道读失败则是 `"child"`。错误码与变体未变（无 wire 影响），
  属可选跟进。
- **`ToolExecutor::run_blocking` 只能在阻塞线程里调**：它自建 runtime 后 `block_on`
  （`crates/agentj-tools/src/executor.rs:220-237`），当前唯一生产调用方是 `spawn_blocking`；
  若将来在异步上下文直接调 `Tool::execute` 会 panic。
- **MCP 子进程链路仍未接线**：`crates/agentj-mcp/src/connection_manager.rs:222` 仍是裸
  `tokio::spawn` 管理子进程，未走 `ToolExecutor` 的进程组 / 超时 / drop 回收。
- **bash 默认 120s 对冷编译偏紧**：Rust 全量 `cargo build` / 大测试套件可能超过 120s；模型可用
  入参 `timeout_ms`（上限 600s）覆盖，默认值是否上调待观察。
- **`[web] state_dir = ""` 已按未设置处理**：解析链回落，但配置文件本身不报错——静默容忍而非
  配置校验告警；若要「写错就喊」需在 `agentj-config` 加载期加校验。
- **flush 与变更之间存在理论竞态（未复现）**：`flush_metrics_state`
  （`crates/agentj-ipc/src/web/persist.rs`）先快照、后**无条件** `mark_clean()`；仅「在该窗口内
  SIGKILL」才真正丢一次增量。
- **grep 回退不读 `.gitignore`**：无 `rg` 时的进程内检索只跳过固定目录
  （`.git`/`target`/`node_modules` 等），不解析 `.gitignore`；命中结果仍受 2000 条上限约束。
- **`web` 子模块的运行期盲点**：跨域行为（例如 metrics 落盘 ↔ 限流计数）仍需在
  `web/tests/integrity.rs` 与 `web/tests/persist.rs` 两侧同时改动；`structure.rs` 只钉薄根。
- **不泄露**：凭据只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中。
- **远端**：`origin`（GitHub）是唯一权威，`gitcode` 是只读镜像；feature 分支推两个远端，
  PR 只在 `origin` 创建与合并（用户负责合并）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑰⁺b | 真 BPE 词表 | 本批已交付可插拔 `TokenCounter` 接缝 + 字符类校准权重，但权重仍是字符类启发式；下一步按 provider 家族嵌入真词表（`tiktoken` 类），只换 `TokenCounter` 实现 | 需引入分词器依赖 / 体积权衡 | 待实施 |
| ㉟⁺b | 产物签名与公证 | 三平台产物已由 CI 真实构建并校验（`deb` / `AppImage` / `dmg` / `msi`），仍缺签名与公证（macOS notarize / Windows signtool） | 需证书与 CI secret 策略 | 待实施 |
| ㊱⁺b | `run --attach` 应答提问 | attach 已能报告并拒绝远端审批，但仍无法应答 `$/askQuestion`（现状由远端 `[agent] question_timeout_s` 收尾） | 需要跨进程提问夹具 + 交互式 attach 输入通道 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。
> 上一批的「真 tokenizer / macOS-Windows 打包 / attach 审批归属 / grep 进程内时限」四行已在本批
> 实施完毕并从本表移除（前三个以降级形态转为上面的 ⑰⁺b / ㉟⁺b / ㊱⁺b）。

## 下一步

- 本批次（合并收口：tokenizer 接缝 / 三平台打包 / attach 审批 / grep 时限）已收尾并过门禁；
  分支推 `origin` 与 `gitcode`，PR 开在 `origin`（用户负责合并）。
- 下一批候选（见上表）：⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证、㊱⁺b `run --attach` 应答提问。
- 表外可选改进：`[web]` 配置校验（空 `state_dir` 喊出来而不是静默回落）；`flush_metrics_state`
  快照与 `mark_clean()` 之间的理论竞态；把「工具结果已截断」作为可观测事件透出
  （当前仅内核内 `AgentEvent::ToolResultTruncated`）；按需裁剪超量 MCP 工具定义。

> 远端提示：本机 `git fetch gitcode` 若报 `fatal: couldn't find remote ref gitcode`，按纪律
> 不 `--force`；分支照常 `git push gitcode <branch>:<branch>`，失败即如实记录。
