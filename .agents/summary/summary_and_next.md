# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**v0.19.0 内核托管 Web UI + 文件树状态补强（无 wire 变更）—— T0–T13 全部落地 + 全量门禁通过 + 文档同步完成；PR 由测试验收流程在 `feature/kernel-hosted-web-ui` 分支通过后创建（合并由用户负责）**
> ——来源=用户请求：① 内核直接托管 Web UI（`serve`/`dashboard` 同端口静态托管 `agentj-ui/apps/web/dist`，
> 远程打开 `http://<host>:4096/` 即用、WS 同源零配置）；② 文件树状态补强（目录聚合 git 状态字母）。
> 范围：`agentj-ipc`（新 `web` 模块 / `ws.rs` / `file_list.rs`）+ `agentj-cli`（`--web-dist` 发现与日志）
> + `agentj-ui/apps/web`（`FileTree`）+ 文档；**Rust / SDK 协议零 wire 改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.18.1 → 0.19.0。

## v0.19.0: 内核托管 Web UI + 文件树状态补强 ✅（本次完成）

- ✅ **T0/T1 文件树目录聚合**：`agentj-ipc` `file/list` 目录 `git_status` 聚合后代最高优先级字母
  （`aggregate_dir_status`：`U>D>M>A>Unknown`，前缀 `"{dir}/"` 严格匹配，`srcx/` 不误伤 `src/`）；
  Web `FileTree` 目录行 `StatusCell` 渲染状态字母、移除冗余圆点。
- ✅ **T2/T3 同端口静态托管**：新 `web` 模块（`WebConfig` + `build_router`）在 `ws_router` 上
  `nest_service("/assets", ServeDir)`（immutable 长缓存）+ `fallback_service(ServeDir::fallback(
  ServeFile(index.html)))`（根 `no-cache`、未知页面 200）；`/ws`、`/login`、`/logout` 显式路由优先
  且不在门后；两静态服务各自包裹 cache 层与登录门（`Router::layer` 会波及 `/ws`）。
- ✅ **T4/T5 混合鉴权登录门**：回环 peer / 未配置 token 直出；非回环 + token 且无有效 cookie →
  `302 /login?next=<同源相对路径>`（`safe_next` 白名单 + `pct_encode`；错拼 `/assets/*` 仍 404）；
  内嵌无 JS 登录页 POST 校验后颁发不透明会话（`uuid`，内存 `RwLock<HashSet>`，重启即失效）经
  `Set-Cookie: agentj_session=<id>; HttpOnly; SameSite=Lax; Path=/`（无 Max-Age）；`/logout` 撤销。
- ✅ **T6 WS cookie 第三通道**：有效会话 cookie + 同源 `Origin`（`origin_matches_host` 去 scheme
  比 Host）免 `?token=`/`initialize.token`；`serve_connection` token 参数遗留，服务端为 cookie 通道
  装配 initialize 参数时注入 server token（`WsConnection::with_init_token` + `supply_initialize_token`）。
- ✅ **T7/T8 CLI + e2e**：`serve`/`dashboard --web-dist <PATH>` 四级发现（显式 flag/env 无效即停并
  告警 → cwd `agentj-ui/apps/web/dist` → exe 相对两档）+ 启动打印 `http://<host>:<port>/`；
  `e2e_http_static`（断言 `/assets/*` 状态码并持有 stdout reader 防子进程 stdout 提前关闭）。
- ✅ **T9–T11 版本**：Rust workspace 0.18.1 → 0.19.0（`Cargo.toml`/`Cargo.lock`/`check-version.sh`）+
  sdk `VERSION` 0.19.0（含断言测试）+ desktop 三处 0.19.0。
- ✅ **T12 协议文档**：`docs/specs/ipc-protocol.md` §2.3 HTTP 静态面 + 0.19.0 changelog + 设计文档
  实施状态回填。
- ✅ **T13 全量门禁 + 文档同步（本次）**：10 项门禁全部通过（见下）；`README.md` / `AGENTS.md` /
  本文件同步 v0.19.0；文档提交 `docs: auto-sync AGENTS.md, README.md, summary after kernel-hosted web ui (v0.19.0)`。

## 验证结果（T13 全量门禁，2026-09-20）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test --workspace` | **591 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 46 passed |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **480 passed / 46 files** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（tsc -b && vite build） |
| `bun --cwd agentj-ui/apps/tui test` | 71 passed |
| `./scripts/check-version.sh` | version-ok（0.19.0） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

> 预置清理：`rm -f ~/.local/share/agentj/sessions/s1.{json,trajectory}`（e2e_permission 硬编码 `s1`
> 会话，前次运行被中断会 hang —— 预存在环境问题，非本特性引入）。
> 实机验收（Playwright：本地打开即用 / 远程登录闭环 / `curl -I` 缓存头）由测试验收流程执行并记录。

## 遗留问题 / 遗留 minors

- 登录无速率限制/锁定（LAN 工具定位；候选：失败计数 + 指数退避）。
- `ServeDir` 跟随符号链接（不 canonicalize）—— dist 为本地构建产物，接受；如需 containment 另立。
- 错拼的 `/assets/*` 返回 404（assets 子树无 SPA fallback）—— 单页应用常规行为，已记录为已知限制。
- WS cookie 通道的 token 注入适配：`serve_connection` 的 token 参数已属遗留/冗余（cookie 通道由服务端
  注入 `initialize.token`）—— 清理该遗留参数为后续重构候选。
- 多段 Range 返回 416（tower-http 语义，无 multipart）；HTTP 静态请求不受 `max-connections` 限制、
  不触发停机（既有分类器语义）。
- 会话内存态：重启 / 换 token 即需重新登录（符合预期）。
- 文档 nit（可选）：`ipc-protocol.md` 静态面可再补 `/logout` 与 `next` 白名单示例。
- 复核修复（Phase-5 Oracle）：v4-mapped loopback 已修（`peer_is_loopback` 认 `::ffff:127.0.0.1`）；
  login `next` 白名单已加固（拒绝反斜杠/控制字符）；`supply_initialize_token` 已补单测。
  遗留可选项：会话集合无上限/TTL、无 dist 时 `/login` 仍挂载、gate 重定向不保留查询串、
  `initialize` 缺 `params` 时不注入 token。

## 下一步

- PR 由测试验收流程在 `feature/kernel-hosted-web-ui` 分支通过后创建（合并由用户负责）。
- 后续特性候选：① WS cookie 通道遗留 token 参数清理（`serve_connection` 去 token arg）；
  ② 登录失败速率限制 / 退避；③ 内核托管 dist 的版本探测 / 资产完整性（SRI）；
  ④ 目录聚合状态在 TUI 侧栏的呈现。
