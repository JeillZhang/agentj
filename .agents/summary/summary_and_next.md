# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.20.0 Web 加固 + TUI Files 视图（四项，无 wire 变更）—— 实机/HTTP 验收通过（v0.20.1 验收修复，bugfix）**
> ——来源=用户请求：① `serve_connection` 遗留 token 参数清理；② 登录失败限流（429/Retry-After）；
> ③ 内核托管 dist 的版本探测 / 资产完整性（SRI）；④ TUI Files 视图（懒加载 `file/list`）。
> 范围：`agentj-ipc`（新 `login_limiter` / `web.rs` / `tcp.rs` / `ws.rs`）+ `agentj-cli`（启动 dist 版本警告）
> + `agentj-ui/apps/web`（`build/version-sri*` + `ConnectionStatus` + `App`）+ `agentj-ui/packages/sdk`（`VERSION` 导出）
> + `agentj-ui/apps/tui`（Files 视图）；**Rust / SDK 协议零 wire 改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.19.0 → 0.20.0 → **0.20.1**（验收 patch）。

## v0.20.1: 验收修复 ✅（本次完成）

- ✅ **实机/HTTP 验收 PASS（2026-09-21，v0.20.0 四项全过）**：
  - ① 登录限流：第 5 次失败 → `429 + Retry-After 30`；锁定期新尝试继续升级锁定；
    到期后 302 + cookie；成功登录清零；开放重定向守卫（`next` 白名单）。
  - ② 版本探测与 SRI：`sha384` 复算匹配；缺失/不一致启动警告；`GET /version` 未鉴权 +
    `no-store` + 不走 SPA fallback（无 dist 时 null）。
  - ③ `serve_connection` 清理：`?token=` 403/101、cookie + Origin 通道、回环直通、签名清理。
  - ④ TUI Files 视图（懒加载 / 键位 / 状态字母）。
- ✅ **BUG-009（high）**：新增全局 `f` 快捷键劫持 composer 输入 → 修复为
  `composer ⇄ sidebar` 焦点模型（Tab 切换焦点；单字母快捷键仅 sidebar 焦点生效）——
  commit `5eea70e`。
- ✅ **BUG-010（low）**：sidebar 提示词在焦点模型下宣传失效键位 → 提示与焦点模型对齐——
  commit `12ec55a`。
- ✅ 两 bug 均回归 verified、零新增 bug；验收 patch：工作区 0.20.0 → 0.20.1
  （`chore: bump the version to 0.20.1`）+ 文档同步（本提交）。
- ✅ 版本：`Cargo.toml`/`Cargo.lock`/`check-version.sh`/sdk `VERSION`/desktop ×3 + 两侧
  lockfile 全部 0.20.1；`PROTOCOL_VERSION` 保持 0.13.0。

## v0.20.0: Web 加固 + TUI Files 视图 ✅（前置，已验收）

- ✅ **① `serve_connection` 遗留 token 参数清理**：`serve_connection` 与 `TcpIpcServer`
  的 token 参数/死字段全部移除（v0.19.0 cookie 第三通道后已无调用方）；
  `WsServerState.token` / `WsQuery.token` / `?token=` / `initialize.token` 语义不变，
  WS 三鉴权通道（query token / initialize token / 会话 cookie + 同源 Origin）行为不变。
- ✅ **② 登录限流**：新 `agentj-ipc` `login_limiter` 模块——按来源 IP 计数
  （`Mutex<HashMap>`，`now: Instant` 注入可测），第 5 次失败起锁定 30s、之后 ×2、封顶 900s；
  锁定期请求直接 `429 + Retry-After`（秒，`div_ceil(1000)` 向上取整、≥1）且不校验 token，
  锁定期内新失败继续升级锁定；成功登录清零；idle_ttl 1800s 懒淘汰；max_tracked 1024
  淘汰最旧条目；回环同样计数（无特殊分支）。`/login` POST 接线（`auth_routes` 透传 peer IP）。
- ✅ **③ 版本探测与 SRI**：`agentj-ui/apps/web/build/version-sri.ts` 纯函数
  （`injectSri` / `buildVersionJson` / `finalizeDist`）+ `version-sri-plugin.ts`
  （Vite `writeBundle`：生成 `dist/version.json`（product/protocol/build_time，单一真源 =
  `packages/sdk/src/config.ts`）+ 入口 HTML 同源 script/stylesheet/modulepreload 注入
  `sha384` `integrity`；本地引用缺失即构建失败；零新依赖）；内核 `WebConfig` 读取
  `DistVersion` + `startup_warnings()`（缺失/不可解析/不一致三类启动警告，不阻断服务）+
  未鉴权 `GET /version`（200 JSON + `Cache-Control: no-store`，显式路由优先于 SPA fallback）+
  `TcpIpcServer::web_config()`；CLI `serve` 启动打印警告 + `e2e_http_static` 覆盖；
  Web `ConnectionStatus` 版本行「版本 UI x · 内核 y」（不一致 `data-state="warn"` + `⚠` +
  warning 色）、`App` 消费 `initialize.server_info.version`、SDK barrel（`index.ts`）导出 `VERSION`。
- ✅ **④ TUI Files 视图**：`files.ts` 纯函数（flatten/format/color/updateNode 等）+
  `files.tsx` `FilesView` + `useFileTree`（懒加载 `file/list` depth=1、stale-reply
  代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取、不持久化）+
  `sidebar.tsx` Threads/Files 切换（`f`）+ `app.tsx` 键位（`f` 切换 / `j,k,↑,↓` 移动 /
  `Enter,l,→` 目录开合 / `h,←` 收起或跳父级 / `r` 刷新）+ mock kernel `file/list` + e2e；
  目录聚合状态字母色义 M→warning、A→success、U→textCaption、D→error。
- ✅ **T20 全量门禁 + 文档同步**：10 项门禁全部通过（见下）；README / AGENTS /
  本文件同步 v0.20.0；文档提交
  `docs: auto-sync AGENTS.md, README.md, summary after web hardening and tui files (v0.20.0)`。
- ✅ **Oracle 全量复核 PASS（无 must-fix）** + 修复波：TUI stale-reply 守卫
  （`fix(tui): drop file-list replies from stale sessions`）+ 父级跳转 clamp
  （`fix(tui): ignore parent-row jumps that are not visible`）+ Web 版本不一致行 warn 色
  （`fix(web): color the version mismatch row with the warning token`，含 CSS 契约测试）。

## 验证结果（验收轮门禁，2026-09-21）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean（T20 后 Rust 源无改动） |
| `cargo test --workspace` | **604 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 47 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **491 passed / 47 files** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（`dist/version.json` + `sha384` integrity 已注入） |
| `bun --cwd agentj-ui/apps/tui test` | **88 passed / 17 files**（BUG-009/010 回归 +3） |
| `./scripts/check-version.sh` | version-ok（**0.20.1**） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> 预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`（e2e_permission 硬编码 `s1`
> 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> 验收状态：**PASS**（实机/HTTP：429/Retry-After + 锁定升级/到期/清零/重定向守卫；dist
> `integrity` + `version.json` 复算匹配；`GET /version`；TUI Files 视图）；2 bugs 已修复并回归。

## 遗留问题 / 遗留 minors

- WS `?token=` 通道未限流（设计范围：限流仅覆盖 `/login` POST；query token 属既有通道语义）。
- 代理/NAT 共享 IP：反向代理后所有客户端共享源 IP，锁定影响整组；未支持代理头解析（LAN 直连定位）。
- `/version` 未鉴权：向未登录者暴露版本号（非机密；与登录页同策略，设计取舍）。
- `build/version-sri.test.ts` 未纳入 `tsc` 类型检查：`tsconfig.node.json` 仅 include
  `vite.config.ts`，测试文件不在引用图内（由 vitest 运行）；如需纳入另立 tsconfig。
- 设计已知限制（见 design 文档）：SRI 仅覆盖入口 HTML 直接引用资产；内核不校验哈希
  （SRI 由浏览器执行）；TUI 32 字符侧栏缩进上限 6 层；Files 展开状态不持久化；
  Enter 于文件无动作（仅展示）。
- ~~全局快捷键与 composer 输入拦截~~：已由 BUG-009 `composer ⇄ sidebar` 焦点模型修复
  （Tab 切换焦点、单字母快捷键仅 sidebar 焦点生效），BUG-010 同步提示词。

## 下一步

- 创建 PR（验收通过后由流程自动创建，**合并由用户负责**）。
- 后续候选：① 登录限流可观测性（日志/指标）；② SRI 覆盖动态 chunk（构建侧 hash 清单）；
  ③ TUI Files 搜索/过滤与展开状态持久化；④ Web 端焦点模型对齐（单字母快捷键分层目前仅 TUI）。
