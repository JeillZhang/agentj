# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.22.0 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口
> （四项，三处加性 wire）——后端部分**
> ——来源：用户请求（2026-09-24，四项）；设计文档
> `docs/superpowers/specs/2026-09-24-agentj-metrics-manifest-search-and-keys-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-24-agentj-metrics-manifest-search-and-keys-plan.md`
> （T0–T31 全部落地）。
> 后端范围：`agentj-ipc`（`login_limiter` 只读快照 + `web.rs` `/metrics`/运行期抽检/清单校验 +
> `file_list` query-aware 遍历）+ `agentj-protocol`（三处加性字段）+ `agentj-cli`
> （e2e 清单 fixture）；`PROTOCOL_VERSION` 0.13.0 → **0.14.0**；工作区 0.21.1 → **0.22.0**。
> 前端部分见 `summary_and_next_fe.md`。

## v0.22.0（后端部分）✅（本次完成）

- ✅ **① 限流指标导出**：`agentj-ipc` `login_limiter` 新增只读 `snapshot(now)` 投影
  （`ip`/`failures`/`locked`/`retry_after`/`last_seen`，排序 `locked desc → failures desc →
  ip asc`）；`web` 新增 `auth_required()` + 显式路由 `GET /metrics`（Prometheus 文本
  `text/plain; version=0.0.4` + `Cache-Control: no-store`：聚合 counters/gauges + per-IP
  序列 Top-50 截断 + `agentj_login_ip_series_truncated` 指示 + `integrity` 摘要；回环 /
  未配 token / 有效 cookie 直出，非回环 + token 无 cookie → `401` 不重定向）；
  `/version` 既有契约零变化。
- ✅ **② 运行期抽检（内核侧）**：`TcpIpcServer` 启动后 60s / 8 文件游标轮转后台抽检
  （`SpotCheckConfig` 可注入，首轮立即执行），不符/缺失 `warn!` 按路径去重；`/version`
  `integrity` 加性嵌套 `runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`；
  内核启动逐文件校验 `asset-manifest.json`（构建期清单生成属前端侧，见 `_fe.md`）。
- ✅ **③ `file/list` 服务端搜索（Rust 侧）**：加性参数 `query`（`path` 子树、忽略 `depth`、
  大小写不敏感子串、访问预算 `SEARCH_VISIT_LIMIT=20000`；**命中仅限文件**，目录参与
  遍历/预算但不作为命中返回）与结果 `total`（预算受限为下界；rank 名称前缀 > 名称子串 >
  路径子串、Top-100）；`session/new`/`session/status` 加性 `project_root`。
- ✅ **T31 全量门禁 + 文档同步**：10 项门禁全部通过（后端计数见下表）；本任务仅文档改动
  （`AGENTS.md` / `README.md` / `summary_and_next_se.md` + `summary_and_next_fe.md` /
  设计文档实施状态 + 准确性回填），无源码变更。
- ✅ **Phase 5 全分支复核（Oracle）+ 修复波**：复核 PASS-WITH-MUST-FIX，1 项 must-fix（a11y，
  前端侧 `SessionListMenu` 飞出层键盘进入路径 + 提示行键位修正，见 `_fe.md`）；下表为修复波后
  后端全量复跑结果。

## 验证结果（后端门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test --workspace` | **646 passed / 0 failed**（44 test binaries） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `./scripts/check-version.sh` | version-ok（**0.22.0**；`PROTOCOL_VERSION` **0.14.0**） |

> 环境预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`
> （e2e_permission 硬编码 `s1` 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> **验收状态：PASS（2026-09-25，`jskill-test-acceptance`）**——0 个新 Bug、10 项门禁独立复跑全绿；
> 后端验收项：`/metrics` 门控矩阵 + 错 token 锁定 per-IP series；清单含根级 public + 独立
> sha384 复核 + 篡改抽检告警 + 无清单不抽检；WS 探针深路径 query / 有效根 / 仅文件命中。
> 前端门禁与验收项见 `summary_and_next_fe.md`。

## 遗留问题 / 已知限制（后端）

- **① `/metrics` 为进程内计数**：重启清零、无跨实例共享、无 per-IP 历史；per-IP 序列
  Top-50 截断（全量需 `RUST_LOG=debug` 日志）；Prometheus 拉取需网络可达（门控下需 cookie）。
  gauge `agentj_login_ip_last_seen_seconds{ip}`（距上次可见秒数）为抽检/快照唯一消费
  `IpSnapshot.last_seen` 的出口。
- **② 抽检为采样**：非全量、非实时；60s 间隔内存在检测窗口；`public/` 覆盖依赖 dist 实目录
  （隐藏文件不在清单）；抽检首轮在启动时立即执行（Tokio interval 首 tick），随后每 60s；
  与「启动后 60s」的措辞差异已澄清。
- **② 抽检任务生命周期边界**：`tokio::spawn` 后无 abort-before-replace；`TcpIpcServer::run`
  错误路径不调用 `stop_spot_check`（进程退出即回收，无泄漏影响）。
- **③ 搜索受预算限制（服务端）**：visit ≤20 000、返回 ≤100；子串匹配（无 glob/regex/模糊）；
  `total` 在截断时为下界；rank 规则分别实现于 Rust `file_list::search_rank`、TUI
  `files.ts::searchRank` 与 mock kernel，跨语言镜像存在漂移风险（消除 → 待实施 ④）。
  旧内核降级行为属前端侧（见 `_fe.md`）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **v0.22.0 已合并 main（`553473f`，!39）**；当前功能完结，下一特性从「待实施」表选择
  （`jskill-feature-next` 合并本表 + `_fe.md` 表）。
- 后端候选即上表 ①②④（前端候选见 `summary_and_next_fe.md`）。
