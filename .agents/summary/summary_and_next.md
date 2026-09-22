# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.21.0 限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（四项，无 wire 变更）+ v0.21.1 验收修复**
> ——来源：用户请求（四项增强）；设计文档
> `docs/superpowers/specs/2026-09-22-agentj-observability-integrity-and-interaction-design.md`
> + 实现计划 `docs/superpowers/plans/2026-09-22-agentj-observability-integrity-and-interaction-plan.md`
> （T0–T29 全部落地）。
> 范围：`agentj-ipc`（`login_limiter` 事件/聚合计数 + `web.rs` 清单启动校验 + `/version` 加性块）
> + `agentj-cli`（日志 `EnvFilter`）+ `agentj-ui/apps/web`（`build/version-sri*` 资产清单 +
> 侧栏快捷键作用域）+ `agentj-ui/apps/tui`（Files 搜索 + 展开持久化 + `config.ts` 抽取）；
> **Rust / SDK 协议零 wire 改动**；`PROTOCOL_VERSION` 保持 `0.13.0`；
> 工作区 0.20.1 → **0.21.0**（feature minor）→ **0.21.1**（验收 bugfix patch）。

## v0.21.0: 限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域 ✅（feature 已落地）

- ✅ **① 登录限流可观测性**：`agentj-ipc` `login_limiter` 在状态变化点发 tracing 事件
  （锁定**新触发** `warn`、成功登录 `info`、逐次失败/锁定期拒绝 `debug`），进程内
  `AtomicU64` 聚合计数经 `GET /version` 加性 `login_limiter` 块暴露
  （`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，无 per-IP 明细）；
  CLI 日志订阅器改 `EnvFilter`（`RUST_LOG` 可配，未设默认 `info`，行为与 v0.20.0 逐字一致）；
  限流策略语义（429/Retry-After/锁定升级/清零）零变化。
- ✅ **② 资产完整性清单**：构建期 `apps/web/build/version-sri*` 生成
  `dist/asset-manifest.json`（`algorithm:"sha384"` + `assets:{"/path":"sha384-<base64>"}`，
  覆盖全部 Rollup bundle 产物含动态 chunk + SRI 注入后的 `index.html`；单遍哈希与 SRI
  `integrity` 逐字节一致；排除 `version.json`/自身/`public/` 透传；键须为 `/` 开头规范路径），
  内核 `WebConfig` 启动逐文件校验并并入 `startup_warnings()`（缺失/不可解析/不符/文件缺失，
  警告不阻断服务），摘要进 `GET /version` 加性 `integrity` 块
  （`manifest_present`/`checked`/`mismatched`/`missing`）。
- ✅ **③ TUI Files 搜索**：`s` 进入搜索（`file/list {path:null, depth:8}` 单请求扁平索引
  ≤2000 + 客户端过滤），可打印字符过滤（含 `f`/`n`，吞键）、`Backspace` 删末字符、
  `Esc` 非空清空/空退出、`Enter` 跳转（祖先自动展开）并选中目标、`r` 重建索引、
  `truncated` 显示 `· partial`；展开状态经新 `filetree.json`（`$XDG_CONFIG_HOME/agentj/`）
  按本地项目键持久化（per-key ≤100、恢复 ≤50、失效路径静默跳过、陈旧键在下次保存时清理）；
  新 `config.ts` 抽取共享 `getConfigDir`/`readJsonFile`/`writeJsonFile`（theme/sidebar/session
  四模块统一引用）；Files 搜索态提示词经 `searchMode` 透传（T16 修复）在侧栏显示
  `type filter · enter jump · esc back`。
- ✅ **④ Web 焦点作用域**：新 `useShortcutScope`（`activeElement` 在侧栏内、目标非可编辑、
  非 IME、无修饰键、无模态/菜单才生效）提供 `f`/`n`/`r`/`j`/`k`/`Esc`；新 `useRovingFocus`
  驱动文件树/会话列表 roving tabindex 键盘导航；`ModalDialog` 开关焦点恢复（原元素卸载不恢复）；
  **Tab 保持原生**、composer 输入零误触；模态/菜单打开时快捷键抑制（`0d46431`）。
- ✅ **T29 全量门禁 + 文档同步**：10 项门禁全部通过（计数见下表）；本任务仅文档改动
  （`AGENTS.md` / `README.md` / 本文件 / 设计文档实施状态），无源码变更。
- ✅ **Phase 5 全分支复核（Oracle）+ 修复波**：复核 PASS-WITH-MUST-FIX，3 项发现
  （C1 TUI 搜索在途输入被索引回包覆盖 / C2 索引错误态被按键清除 / C3 空 `RUST_LOG`
  回退 ERROR 默认）已修复（`5cdb534`、`22fcc7e`）并通过 scoped 复核；下表为修复波后
  全量复跑结果（Rust 621 → 622、TUI 128 → 130，其余不变）。

## v0.21.1: 验收修复（BUG-011 / BUG-012）✅（本次完成，bugfix 无协议变更）

- ✅ **实机验收（`jskill-test-acceptance`）已执行 PASS**：Playwright（chromium headless）/
  HTTP / TUI 全过；两个 Bug 均在实机 + jsdom 套件独立复现与验证。
- ✅ **BUG-011（low，`89396f3`）**：按 `f` 切换侧栏视图后焦点掉出侧栏（`<body>`），
  后续 `j`/`k`/`r`/`Esc` 失效直至重新点击/Tab 进入。修复：`SidebarTabs` tab 按钮加
  `data-tab`，`UnifiedSidebar.toggleTab` 切换后把焦点 park 到始终挂载的活动 tab 按钮
  （新增回归用例 + 强化既有 2 用例，web 538 tests）；回归轮 1 verified。
- ✅ **BUG-012（low，`71245d5`）**：焦点在文件树行（`j`/`k` 移动后）按 `r` 刷新 → 树卸载重挂、
  焦点掉出侧栏（BODY），后续 `j`/`k`/`Esc` 失效（BUG-011 同类、`r` 路径未覆盖）。修复：
  `r` handler 在 `refresh()` 前把焦点 park 到 files tab 按钮（同模式；web 539 tests / 50 files）；
  回归轮 2 verified、零新 Bug。
- ✅ **回归收敛**：测试轮 1（发现 BUG-011）→ 修复 → 回归轮 1（verified + 发现同类 BUG-012）
  → 修复 → 回归轮 2（verified，零新 Bug）。
- ℹ️ **越范围观察（不记为 Bug）**：`n` 新建会话后侧栏列表不即时刷新（`listRows` 仅 mount 时
  拉取一次）为 v0.21 之前既有行为，非本特性引入。

## 验证结果（全量门禁，2026-09-23）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test --workspace` | **622 passed / 0 failed**（44 test binaries；604 → 622） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 47 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **539 passed（50 files）**（491 → 537 → 539；验收修复后复跑） |
| `bun --cwd agentj-ui/apps/tui test` | **130 passed（19 files）**（88 → 130） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（`dist/version.json` + `asset-manifest.json` + sha384 integrity 已注入） |
| `./scripts/check-version.sh` | version-ok（**0.21.1**，bump 后） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> 环境预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`
> （e2e_permission 硬编码 `s1` 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> 验收状态：**已执行 PASS**（`jskill-test-acceptance`，2026-09-23）；上表为复核修复波 +
> 验收修复后复跑结果（622 Rust / 47 sdk / 539 web（50 files）/ 130 tui（19 files）；
> `check-version` 于 bump 后为 0.21.1）。

## 遗留问题 / 已知限制

- **限流面**：WS `?token=` 通道未限流（限流仅覆盖 `/login` POST；query token 属既有通道语义）。
- **代理/NAT 共享 IP**：反向代理后所有客户端共享源 IP，锁定影响整组；未解析代理头
  （LAN 直连定位）。
- **`/version` 未鉴权**：向未登录者暴露版本号 + 限流聚合 + 完整性摘要（非机密；
  与登录页同策略，设计取舍）。
- **① 计数为进程内累计**：重启清零、无跨实例共享；`RUST_LOG` 需显式设置才有 debug 明细；
  无 per-IP 明细（排查具体 IP 需看日志）。
- **② 完整性为启动校验一次 + 清单边界**：仅覆盖清单内资产（`index.html` 与全部 bundle 产物
  含动态 chunk；`version.json`/`public/` 透传不在清单内）；运行中篡改不检测（部署完整性定位，
  D7 部分逆转，非安全边界）；浏览器端动态 import 无 SRI（SRI 为浏览器机制）。
- **`build/version-sri.test.ts` 未纳入 `tsc` 类型检查**：`tsconfig.node.json` 仅 include
  `vite.config.ts`，测试文件不在引用图内（由 vitest 运行）；如需纳入另立 tsconfig。
- **③ TUI 搜索/持久化**：索引受服务端上限（depth ≤8、entries ≤2000；`truncated` 提示可能
  不完整）；搜索模式吞可打印键（D10 显式取舍）；跳转依赖索引快照 —— 失效路径仅在 `Enter`
  时检测（退出搜索 + 根刷新 + 一次性错误行）；32 字符侧栏缩进上限；展开恢复 ≤50 条、
  按本地项目键持久化；D11 陈旧项目键仅在**下次保存**时自清理（非即时删除）；
  T15 注：TUI 目前无组件为 session 写入 `projectId`，有效键回退为 `projects[0].id`
  （stock Inbox 为 `default`，空项目列表才用 `__default__`）。
- **④ Web 焦点作用域**：快捷键需焦点在侧栏（Tab/点击进入 —— 不劫持 Tab 的代价，D14）；
  `ModalDialog` 仅恢复焦点、无焦点陷阱（D16）；`TopbarMenu`/`SessionListMenu` 既有 roving
  逻辑未统一重构（D17，列为未来项）。
- **测试 hermeticity（T13/T14/T15 注）**：`app.test.tsx` 曾写真实 `~/.config/agentj`
  （XDG 污染）→ 已用临时 `XDG_CONFIG_HOME` 修复（本次内完成）；其余 XDG 语义不变。

## 下一步

- **等待流程自动创建 PR**（`jskill-test-acceptance` 验收通过后由流程创建）；
  **合并由用户负责**（用户手动合并）。
- 后续候选：① 限流可观测扩展（per-IP 明细/指标导出）；② 资产清单扩展
  （`public/` 透传 + 运行期抽检）；③ TUI 搜索增强（服务端搜索、展开状态跨会话共享）；
  ④ Web 键位模型收口（D17 菜单 roving 统一重构 + 侧栏快捷键提示）。
