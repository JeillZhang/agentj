# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.6 断连重连后的 `SessionBusy`（修复）**
> ——来源：用户报告「断连后，重新连接会话，任务中断，提示继续后，提示 sessionbusy」。
> 后端范围：`crates/agentj-ipc/src/lib.rs`；前端配套见 `summary_and_next_fe.md`。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.5 → **0.29.6**（纯修复，无新增方法 / schema）。

## v0.29.6（owner 断连即结算待审批；Web 重连即重新订阅）🚧

### 缺陷与根因

`docs/specs/ipc-protocol.md` 异常断开一节明确：**断线客户端的 run 继续执行**，重连者用
`session/subscribe` 观测而非重试 `session/run`。内核与 Web 两侧都没跟上这条契约：

| 位置 | 缺陷 |
|---|---|
| `crates/agentj-ipc/src/lib.rs` `serve_connection_requests` | EOF 后先 `while pending.join_next().await.is_some() {}` **排空在飞 run**，`cleanup_connection` 在排空**之后**才执行 → owner 已断连，该会话的待审批权限无人可答，孤儿 run 只能等 `[permission] timeout_ms`（默认 60s）自动拒绝；多轮工具调用可反复叠加（最多 `agent.max_tool_iterations` 轮），会话长时间「忙」 |
| `crates/agentj-ipc/src/lib.rs` `attach_permission_notifier` | 找不到 owner 连接时**静默丢弃**审批请求，同样干等到超时 |
| 同上（排空期间） | `session_owner[sid]` 只在 `unregister`（排空**之后**）清除，故排空期间新发起的审批仍会 `send_to` 已消失的连接（`Ok(())` 静默丢弃）→ 仍干等整个审批超时（多轮工具调用「反复叠加」的真实来源）；且 EOF 之外的收尾路径（传输错误 / shutdown）完全未走结算 |
| `agentj-ui/apps/web`（前端，配套） | 重连后不重新 `session/subscribe`，把仍在运行的 run 显示成「任务中断」，用户点继续即撞上 `-32805 SessionBusy` |

### 修复

- 新增 `pub(crate) const DISCONNECTED_REASON: &str = "disconnected";` 与
  `IpcServer::settle_owner_pending(&self, conn_id)`：把该连接所拥有会话（`sessions_owned_by`）
  的 question 与 permission 待决项一并结算（`question_module.cancel_session` +
  `permission_module.cancel_session(sid, DISCONNECTED_REASON)`）。
- `serve_connection_requests` 拆出 `pump_requests`（请求循环），在**每条**收尾路径（EOF /
  传输错误 / shutdown）上先 `settle_owner_pending` 再排空在飞 run；`cleanup_connection` 复用
  同一逻辑后再 `unregister`（不重复实现）。
- 新增 `ConnectionRegistry::clear_owner(conn_id)`（`session_owner.retain(|_, owner| owner != conn_id)`），
  在 `settle_owner_pending` 里于排空**之前**调用：排空期间新发起的审批走 `owner.is_none()`
  分支立即被拒，而不是发给已消失的连接。
- `attach_permission_notifier`：`owner.is_none()` 时改为立刻
  `cancel_session(session_id, DISCONNECTED_REASON)`（对齐 question 模块「owner 断连即 skip」）。
- 语义保持：run 本身**不取消**（顺 `$/progress` 继续走完），只是不再有「等人审批」的僵局；
  重连者仍按契约重新 `session/subscribe` 观测。
- 规范同步：`docs/specs/ipc-protocol.md` 异常断开一节 + §7.5 新增「断连即拒绝
  （`@since 0.29.6`）」；`docs/guide/web.md` 补充「重连自动重新订阅 + `-32805` 友好提示」。
- 代码评审（`jskill-bug-fix` Phase 5）：修复了评审提出的 B1（排空期间新审批仍发往死连接）、
  S1（shutdown 收尾路径未结算）、S3（传输错误 `?` 提前返回导致排空被取消）；S2 按实现
  改写 spec（无 owner 用例理由为「请求已不存在」）；N5 修正 `attach_permission_notifier`
  陈旧注释（原「stdio falls back to stdout」与实际不符，stdio 靠 sentinel 连接即 owner）。
- **排空期间连接必须存活**：`pump_requests` 改为借用 `&mut Box<dyn Connection>`。一次回归：
  按值传递会让函数返回即析构 `TcpConnection`，tokio 的 `OwnedWriteHalf` 析构会 shutdown
  写方向 → 「EOF 前已受理、响应尚未写完」的请求被对端看成 EOF（服务器侧写入得到
  `BrokenPipe`），`serve --port` 下表现为紧随其后的 TCP 连接收到空响应。守护用例：
  `crates/agentj-cli/tests/e2e_ws.rs::serve_port_accepts_ws_clients_and_keeps_tcp_attach`
  （修复前 12 次里失败 3 次以上，修复后 12/12 通过）。
- **测试稳定性（非本缺陷引入）**：`crates/agentj-ipc/src/login_limiter.rs` 两个日志捕获用例
  原先用 `tracing::subscriber::set_default`（线程本地），其 set/drop 会重建全局 callsite
  interest 缓存并与同一测试二进制内并行测试的日志竞争，偶发丢掉 `record_success` 的
  `login ok` 事件。现改为进程级 `set_global_default`（`OnceLock` 只装一次）+ 互斥量串行化
  两个捕获用例：`--test-threads=8` 下 40 次压力原为 5 次失败，改后 0 次。

### 测试（RED → GREEN）

| 测试 | 文件 | RED 证据 |
|---|---|---|
| `owner_disconnect_denies_pending_approval` | `crates/agentj-ipc/src/lib.rs` | `panicked … owner disconnect must settle the pending approval: Elapsed(())` |
| `serve_connection_end_of_input_denies_pending_approval`（真实链路：`scripted_conn` + `serve_connection`：session/new → session/run → 观测 `$/permissionRequest` → drop 输入发送端 = EOF → 2s 内结算 + 5s 内释放 run + run 响应仍送达客户端） | 同上 | 关闭生产修复后：`owner disconnect must settle the pending approval at once` |
| `approvals_during_drain_are_denied_at_once`（fixture `blocking_run_server_with(2)`：mock 一次模型轮返回两个 `bash` 调用 → 第二个审批在排空期间发起） | 同上 | 去掉 `clear_owner` 后：`timed out waiting for a connection log line: Elapsed(())`（第二个审批干等 10s 审批预算，run 无法在 5s 内回 `id:2`） |
| `approval_without_reachable_owner_is_denied_immediately` | 同上 | `no owner must be settled at once, not after the 30s timeout: Elapsed(())` |

> 细节：`PermissionModule::register_ask` 在插入 pending 后**同步**调用 notifier，故无 owner 用例
> 收到的 reason 是 `request not found`（条目已被取消）而非 `disconnected`——断言写成
> `assert_ne!(reason, "timeout")` + `module.view(&request_id).is_none()`，避免绑定实现细节。
> 首版 `settle_owner_pending` 里误留 `unregister(conn_id)`，会在排空前摘掉 writer，导致孤儿 run
> 的最终响应**静默丢失**（`send_to` 对缺失 writer 返回 `Ok(())`）——被第 2 条测试捕获后删除。

### 验证结果（0.29.6，2026-10-16）

| 门禁 | 结果 |
|---|---|
| `cargo test -p agentj-ipc --lib`（4 条新增） | 193 passed，0.03s |
| `cargo test -p agentj-cli --test e2e_ws` | 12/12 通过（排空生命周期回归守护） |
| `login_limiter` 捕获用例压力（`--test-threads=8` × 40） | 修复前 5/40 失败 → 修复后 0/40 |
| `cargo test --workspace` | 48 套件 / **866 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo fmt --all` | clean |
| Web（`npm --prefix agentj-ui test --workspace=@agentj/web`） | 53 文件 / 655 passed（含新增 3 条） |
| `./scripts/check-version.sh` | `version-ok`（0.29.6 / 0.18.0） |

## 已完成（0.29.0 – 0.29.5，详见 `docs/changelog/v0.29.md`）

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。
- ✅ 0.29.4：真实 key 端到端调测适配（4 个上游缺陷 + 1 个测试挂起）——合成 agentj-model
  部分：`catalog_source::bare_id()` 去 `opencode-go/` 前缀（三条路由的 model id 泄漏）；
  `responses` 文本/工具 delta 改「字符串优先、旧对象形状容忍」；新增 `retry::send()` 温度类
  400 去参数重试一次；`gateway_error::api_error` 收敛三份私有副本 + `429` → `RateLimited`；
  `[permission] timeout_ms` 使 e2e fixture 审批超时降到 800ms，
  `crates/agentj-cli/tests/e2e_permission.rs` 改用真实 `session_id` + 20s 截止通道
  （修复前该用例**无限挂起**导致 `cargo test --workspace` 永不终止，现 0.85s）。
  真实 key 实测 9 模型 × 3 路由全绿（含 `glob` 工具调用命中唯一标记文件
  `./UNIQUE_MARKER_7f3a.txt`）。
- ✅ 0.29.5：`serve` / `dashboard` 不再强制预先配置密钥（`providers::NO_USABLE_PROVIDER`
  只作为警告，`main.rs` 仅对无可录入客户端凭据的命令 fatal；`run` 行为不变）。

## 遗留问题 / 已知限制（后端）

- **`agentj run` 非 TTY 下遇需审批工具会一直等待**：无自动拒绝、无 `--yes` 开关，也没有上限
  （0.29.4 以只读工具绕过，未改该行为）。跟进项：加 `run --yes` 或非 TTY 自动拒绝。
- **`[permission] timeout_ms` 未做下界校验**：`0` 语义为「立即自动拒绝」，当前不视为非法配置。
- **2 条 legacy 路由**：`qwen3.6-plus` / `qwen3.7-max` 按 models.dev 判为 `openai-compat`
  （旧手写表标 `anthropic`），二者均不在官方在售列表内；如实测不符，加回一条种子即可。
- **温度降级只重试一次**：`retry::send` 仅处理「仅 temperature 不支持」的 400；若模型同时拒绝
  其他参数，仍按原错误上抛（不盲目剥参）。
- **凭据时序**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间进行中的那一轮 run
  可能仍使用旧凭据，新凭据从下一轮生效。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 即远端主机）；
  非回环 `serve --host` 必须配 `--token`。
- **`e2e_cli` / `e2e_permission` 未隔离 `XDG_DATA_HOME`**（仅 `e2e_auth.rs` / `e2e_serve.rs` 隔离）：
  实测在「注入假凭据 + `AGENTJ_OPENCODE_GO_API_KEY`」的环境下仍全绿，非必须但可硬化。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中；两个 auth 方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。

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

- v0.29.6 代码 + 测试已落地，版本元数据已收尾（`Cargo.lock` / desktop `Cargo.lock` 已刷到
  0.29.6，`./scripts/check-version.sh` 已过）；剩余：重跑全量门禁 → 提交 →
  推送**两个远端**（本机 `github` = GitHub 权威、`origin` = GitCode 镜像；注意 AGENTS.md 的
  远端命名与本机相反）→ 在 GitHub 开 PR（用户负责合并）。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
- 可选跟进：为 `run` 增加 `--yes`（或非 TTY 自动拒绝），让 CI / 自动化能跑需审批工具路径；
  以及把 `serve` 的「无 provider」从 WARN 提升为 `/version` 可见的启动警告（目前仅日志）。
