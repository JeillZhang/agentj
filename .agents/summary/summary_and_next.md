# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.22.0 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口（四项，三处加性 wire）**
> ——来源：用户请求（2026-09-24，四项）；设计文档
> `docs/superpowers/specs/2026-09-24-agentj-metrics-manifest-search-and-keys-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-24-agentj-metrics-manifest-search-and-keys-plan.md`
> （T0–T31 全部落地）。
> 范围：`agentj-ipc`（`login_limiter` 只读快照 + `web.rs` `/metrics`/抽检/清单覆盖 +
> `file_list` query-aware 遍历）+ `agentj-protocol`（三处加性字段）+ `agentj-cli`
> （e2e 清单 fixture）+ `agentj-ui/apps/web`（清单 `public/` 覆盖 + 共享 `useRovingFocus`
> 收口 + 侧栏提示行）+ `agentj-ui/apps/tui`（服务端搜索 + `project_root` 键）；
> `PROTOCOL_VERSION` 0.13.0 → **0.14.0**；工作区 0.21.1 → **0.22.0**（feature minor）。

## v0.22.0: 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口 ✅（本次完成）

- ✅ **① 限流指标导出**：`agentj-ipc` `login_limiter` 新增只读 `snapshot(now)` 投影
  （`ip`/`failures`/`locked`/`retry_after`/`last_seen`，排序 `locked desc → failures desc →
  ip asc`）；`web` 新增 `auth_required()` + 显式路由 `GET /metrics`（Prometheus 文本
  `text/plain; version=0.0.4` + `Cache-Control: no-store`：聚合 counters/gauges + per-IP
  序列 Top-50 截断 + `agentj_login_ip_series_truncated` 指示 + `integrity` 摘要；回环 /
  未配 token / 有效 cookie 直出，非回环 + token 无 cookie → `401` 不重定向）；
  `/version` 既有契约零变化。
- ✅ **② 资产清单扩展 + 运行期抽检**：构建期清单覆盖 dist 实目录全部部署文件
  （含 `public/` 透传，bundle 键优先，排除 `version.json`/自身/隐藏文件）；
  `TcpIpcServer` 启动后 60s / 8 文件游标轮转后台抽检（`SpotCheckConfig` 可注入，
  首轮立即执行），不符/缺失 `warn!` 按路径去重；`/version` `integrity` 加性嵌套
  `runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`。
- ✅ **③ `file/list` 服务端搜索**：加性参数 `query`（`path` 子树、忽略 `depth`、
  大小写不敏感子串、访问预算 `SEARCH_VISIT_LIMIT=20000`；**命中仅限文件**，目录参与
  遍历/预算但不作为命中返回）与结果 `total`（预算受限为下界；rank 名称前缀 > 名称子串 >
  路径子串、Top-100）；TUI 搜索改防抖 150ms 服务端查询（旧内核无 `total` → 自动降级
  depth-8 本地过滤）；展开态键改为会话有效 `project_root`（`session/new`/`session/status`
  加性 `project_root` 捕获，未知回退 `__default__`）。
- ✅ **④ Web 键位收口**：共享 `useRovingFocus`（`wrap`/Home-End）扩展，
  TopbarMenu / SessionListMenu / SidebarTabs / SettingsDialog 四处迁移
  （ComposerOptions listbox 保留独立模式）+ 侧栏底部快捷键提示行
  （单一常量源、`aria-hidden`；三态文案组件，Web 当前两态可达）。
- ✅ **T31 全量门禁 + 文档同步**：10 项门禁全部通过（计数见下表）；本任务仅文档改动
  （`AGENTS.md` / `README.md` / 本文件 / 设计文档实施状态 + 准确性回填），无源码变更。
- ✅ **Phase 5 全分支复核（Oracle）+ 修复波**：复核 PASS-WITH-MUST-FIX，1 项 must-fix
  （a11y）：四处共享 roving 迁移后 `SessionListMenu` 的过滤/排序飞出层丢失键盘进入路径
  （箭头键无法进入子菜单）→ 恢复飞出层方向键进入（`cd24cb2`）；并修正提示行宣传未绑定
  键位（从 Files 浏览态提示去除 Web 不存在的 `s 搜索`，`784ed8b`）；下表为修复波后
  全量复跑结果。

## 验证结果（全量门禁，2026-09-25）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test --workspace` | **648 passed / 0 failed**（49 test binaries） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 47 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **552 passed（51 files）** |
| `bun --cwd agentj-ui/apps/tui test` | **142 passed（19 files）** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `./scripts/check-version.sh` | version-ok（**0.22.0**；`PROTOCOL_VERSION` **0.14.0**） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> 环境预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`
> （e2e_permission 硬编码 `s1` 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> **验收状态：待执行**（live/实机验收由 `jskill-test-acceptance` 负责，尚未运行）。

## 遗留问题 / 已知限制

- **① `/metrics` 为进程内计数**：重启清零、无跨实例共享、无 per-IP 历史；per-IP 序列
  Top-50 截断（全量需 `RUST_LOG=debug` 日志）；Prometheus 拉取需网络可达（门控下需 cookie）。
  gauge `agentj_login_ip_last_seen_seconds{ip}`（距上次可见秒数）为抽检/快照唯一消费
  `IpSnapshot.last_seen` 的出口。
- **② 抽检为采样**：非全量、非实时；60s 间隔内存在检测窗口；`public/` 覆盖依赖 dist
  实目录（隐藏文件不在清单）；浏览器端动态 import 仍无 SRI（定位：部署完整性）。
  抽检**首轮在启动时立即执行**（Tokio interval 首 tick），随后每 60s；与「启动后 60s」
  的措辞差异已澄清。
- **② 抽检任务生命周期边界**：`tokio::spawn` 后无 abort-before-replace；`TcpIpcServer::run`
  错误路径不调用 `stop_spot_check`（进程退出即回收，无泄漏影响）。
- **③ 搜索受预算限制**：visit ≤20 000、返回 ≤100；子串匹配（无 glob/regex/模糊）；
  `total` 在截断时为下界；防抖引入 ≤150ms 输入延迟；旧内核降级为 depth≤8/≤2000 索引
  （不完整）；TUI 仍不发送 `project_root`（键为内核 cwd 回退）。
- **③ 注（TUI 降级边界）**：`r` 在 150ms 防抖窗口内按下时，该次搜索会话回退为本地索引模式。
- **③ 注（rank 三处镜像）**：rank 规则分别实现于 Rust `file_list::search_rank`、
  TUI `files.ts::searchRank` 与 mock kernel，跨语言镜像，未来变更存在漂移风险。
- **④ Web 展开态键仅 Web-safe**：既非 `__default__` 也不以 `/` 开头的键
  （如 Windows `C:\...`）在保存时被剪除，仅保留当前活动键。
- **④ 菜单 Tab 行为变更**：迁移后 Tab 不再逐项循环（ARIA 对齐，显式记录）；
  ComposerOptions 保持独立模式；提示行不覆盖菜单/对话框键位（「帮助」项仍为 no-op）。
  Web 无搜索模式（设计非目标 ③），故 Files 搜索态提示文案组件支持但不可达。

## 下一步

- **等待 `jskill-test-acceptance` 验收**（live/实机验收由该流程负责）；
  验收通过后由流程**自动创建 PR**；**合并由用户负责**（用户手动合并）。
- 后续候选：① 指标导出扩展（计数持久化/跨实例共享、代理头解析）；② 抽检扩展
  （逐请求校验 / 手动全量触发端点）；③ Web 接入服务端搜索（打通 Files 搜索态提示）；
  ④ rank 规则单一真源化（消除跨语言镜像漂移）。
