# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **B9 可观测性 / 配置校验 / 协议截断上报批次（后端侧）**——协议
`PROTOCOL_VERSION` 由 `0.20.0` 升到 **`0.21.0`**（加性字段，方法数 29 / schemas 81 均不变），
工作区版本仍 `0.1.0`。

> 当前功能：**B9 可观测性 · 配置校验 · 协议截断上报 · 用量面板** —— 分支
> `feature/b9-observability-config-hardening`，16 任务计划（T1–T11 + b 后缀子任务）。
> 设计与计划是本地文档（`docs/superpowers/{specs,plans}/2026-10-25-b9-observability-config-hardening*`，
> 不入库）。

## B9 批次（后端侧） ✅

### 交付（每项一提交，均已过门禁）

| 计划项 | 提交 | 内容 |
|--------|------|------|
| T1 | `e57ff0b` | 限流快照落盘不丢更新：`LoginLimiter` 增 `generation`，写盘成功仅在代数未变时清 `dirty` |
| T2 | `6014f9a` | `Config::validate()` 逐项渲染钳制告警，`serve` / `dashboard` 启动打印 |
| T5 | `dc61fad` | `budget::definition_tokens` / `bounded_definitions`：工具定义占用 ≤ 请求预算 30%，超出从尾部丢弃 |
| T4/T4b | `93b4fcb` | `agentj-mcp/src/process.rs`：独立进程组 + 整组终止 + drop 回收，`McpConnectionManager` / `transport` 接线 |
| T8 | `ea01e4f` | `run --attach` 应答 `$/askQuestion`（stderr 提示 + 「跳过（未回答）」结算；自有请求 id 记账，拒绝回执不误判为 turn 失败） |
| T3 | （同批） | `agentj-tools` spawn `NotFound` 单独成句回报，不再算作工具执行失败 |
| T6/T6b/T6c/T7 | `6c2f387` | `truncated_chars` 上 wire（事件 + `$/progress`）+ SDK 解析 + Web 工具卡片显示「结果已截断 N 字符」，协议 0.21.0 |
| T11 | `9af7c8b` | `UsageSparkline` 密集序列按桶聚合（8px 阈值，每桶保留 min/max 顶点） |
| T10 | `8ba2aa6` | shell 作用域 `u` / `Shift+U` 打开用量 / 订阅设置，状态面板标出 `u` 提示 |
| T9（替代形态） | `ee80d57` | 设置拆出「用量」「订阅」两个分类（`SubscriptionPanel` 增 `section`），凭据缺失时订阅 tab 带装饰红点 |
| T9a（部分） | `a6353a2` | `deviceStateLabel` 移入 `subscriptionModel.ts` |
| 收尾 | `21c6e09` | `read_attach_stream` 拆出 `read_attach_frame` / `send_own_request`，回到 ≤50 行门禁 |
| CI 修复 | （本批末尾） | `smoke.yml` 五个 `- name: Smoke: …` 未加引号导致 YAML 解析失败（每次 push 0s 变红，`main` 同样）→ 加引号修复，`validate-workflow.sh` 通过 |
| CI 修复 | （本批末尾） | serve 就绪行之后的 banner 写 stdout 改走 `emit_line`：管道被关闭时记日志而非 `println!` panic 退出（coverage 作业 `e2e_ws` `ConnectionReset` 的根因）；补单测 + e2e `serve_survives_a_closed_stdout_pipe` |

**与计划的偏差**：T9 原计划把 `SubscriptionPanel` 拆成 `SubscriptionCredentialCard` +
`SubscriptionHistory` 两个组件文件；实际采用**同组件按 `section` 分段**（设置两个 tab 各渲染一半，
共用一份数据与轮询契约），未新建那两个文件——AC 的「凭据卡无 client 可用」「历史失败只影响历史区」
由既有测试与新增分段测试覆盖。T9a 的 `historyEmptyText` 未实现：当前没有「暂无用量记录」空态
（趋势图在 <2 点时整体不渲染），加了即为死代码。

### 门禁状态（本批收尾实测）

- `cargo test --workspace --locked`：43 个 test binary 全绿；`cargo clippy --workspace --all-targets
  --locked -- -D warnings` 零警告；`cargo fmt --all --check` 通过。
- `cargo test -p agentj-protocol drift` 通过（schemas 与 `protocol/methods.json` 无漂移，
  `protocol_version: 0.21.0`）。
- `python3 scripts/tests/fn-length.py crates/`：0 个函数 > 50 行（收尾前 `read_attach_stream`
  曾 59 行，已拆）。
- `bash scripts/tests/assert-{cargo,ui,ci}.sh all`：6 / 3 / 10 全 PASS，0 skipped。
- `cargo llvm-cov --workspace --fail-under-lines 92 --lcov --output-path lcov.info --locked`
  （本地镜像 CI coverage 作业）：全绿，行覆盖 **93.81%**（32298/34429）。
- `./scripts/check-version.sh`：`version-ok`（工作区 0.1.0 + 协议 0.21.0 全线一致）。
- 前端：`npm --prefix agentj-ui test --workspace=@agentj/web` 772 passed；
  `run build` / `run lint`（0 error）/ `@agentj/sdk` 91 passed / TUI `bun test` 204 passed。

## 遗留问题 / 已知限制（后端）

- **仍是启发式估算**：`HeuristicTokenizer` 按字符类加权，不是真 BPE 词表；`retry_budget`
  （上游超限收紧重试，最多 2 次）继续保留为兜底。见待实施 ⑰⁺b。
- **工具定义裁剪按注册顺序**：`bounded_definitions` 从尾部丢弃，注册顺序只是「有用性」的
  粗略代理；没有按当前任务 / 会话动态挑选工具的策略。
- **配置告警是「渲染后打印」而非结构化**：`Config::validate()` 返回 `Vec<String>`，`serve` 只在
  启动时打印一次，未进 `/metrics` 也未进 `/version`（`startup_warnings()` 只覆盖 dist 三类）。
- **attach 仍是单向**：只会 deny 审批与「跳过」提问，不能批准、也不能回答具体选项
  （回答需要跨进程交互式输入通道，本批未做）。
- **`dmg` / `msi` 未签名未公证**：见待实施 ㉟⁺b。
- **`temp_dist` 固定 `/tmp` 目录**：`agentj-cli-e2e-web-{tag}` 仅按 tag 区分，同 tag 并发会互踩。
- **fixture 基线重复**：`web_verify_off.toml` / `web_fast_spot.toml` / `attach_ask.toml` 各自复制
  了 `permission_deny.toml` 的完整基线（含 `timeout_ms`），基线变更需同步多处。
- **进程型工具报错里的 `name` 仍是程序名**：`ProgramMissing.name` 填程序（`sh` / `rg`），
  与工具名不一致；错误码与变体未变（无 wire 影响），属可选跟进。
- **`ToolExecutor::run_blocking` 只能在阻塞线程里调**：自建 runtime 后 `block_on`，在异步上下文
  直接调会 panic（当前唯一生产调用方是 `spawn_blocking`）。
- **bash 默认 120s 对冷编译偏紧**：可用入参 `timeout_ms`（上限 600s）覆盖，默认值待观察。
- **grep 回退不读 `.gitignore`**：无 `rg` 时只跳过固定目录，命中仍受 2000 条上限约束。
- **MCP 进程组回收只覆盖新链路**：`connection_manager` 已走 `process.rs`，但 `agentj-tools` 的
  `ToolExecutor` 与 MCP 的进程组语义是两套实现（未合并成单一抽象）。
- **不泄露**：凭据只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中。
- **远端**：`origin`（GitHub）是唯一权威，`gitcode` 是只读镜像；feature 分支推两个远端，
  PR 只在 `origin` 创建与合并（用户负责合并）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑰⁺b | 真 BPE 词表 | 可插拔 `TokenCounter` 接缝 + 字符类校准权重已交付，权重仍是启发式；下一步按 provider 家族嵌入真词表（`tiktoken` 类），只换 `TokenCounter` 实现 | 需引入分词器依赖 / 体积权衡 | 待实施 |
| ㉟⁺b | 产物签名与公证 | 三平台产物已由 CI 真实构建并校验（`deb` / `AppImage` / `dmg` / `msi`），仍缺签名与公证（macOS notarize / Windows signtool） | 需证书与 CI secret 策略 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。
> 本批实施完毕并移除的一行：**㊱⁺b `run --attach` 应答提问**（T8，attach 现在以「跳过」结算提问，
> 仍不能给出具体答案——见遗留问题）。

## 下一步

- 本批次（B9 可观测性 / 配置校验 / 协议截断上报 / 用量面板）已收尾并过门禁；分支推 `origin` 与
  `gitcode`，PR 开在 `origin`（用户负责合并）。
- 下一批候选（见上表）：⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证。
- 表外可选改进：把 `Config::validate()` 的告警接进 `/version` 与 `/metrics`（目前只打印）；
  工具定义按任务动态挑选而非注册顺序；attach 支持回答具体选项；合并 MCP 与 `ToolExecutor`
  两套进程组实现。

> 远端提示：本机 `git fetch gitcode` 若报 `fatal: couldn't find remote ref gitcode`，按纪律
> 不 `--force`；分支照常 `git push gitcode <branch>:<branch>`，失败即如实记录。
