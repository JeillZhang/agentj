# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**远程访问修复（v0.16.2）—— bugfix，无协议变更**
> ——来源=用户反馈：远程通过 http://192.168.3.31/ 打开 Web 工作台报
> `kernel closed on ws://127.0.0.1:4096/ws`；连接指定 IP 后刷新断连

## v0.16.2: 远程访问修复 ✅（本次完成）

- ✅ **(1) 服务端 `--host`（根因：绑定硬编码 127.0.0.1）**：`serve --port` / `dashboard`
  新增 `--host`（默认仍 127.0.0.1 保持本机信任；`--host 0.0.0.0` 支持局域网/远程直连）；
  `dashboard` 补齐 `--token`；非回环且无有效 `--token`（含空串）时 stderr 暴露告警；
  启动日志打印实际 `ws://<host>:<port>/ws`（IPv6 加方括号、端口取实际绑定值）；
  `listening on ` 行保持兼容（e2e 解析）。
- ✅ **(2) Web 端页面地址派生（根因：默认回落 127.0.0.1）**：`resolveWsUrl` 新增
  `location` 输入（无 `VITE_AGENTJ_WS_URL`/显式 host 时取 `location.hostname:4096`；
  https → wss；`tauri.localhost` 守卫保持明文回环，Desktop 不受影响）；Web App 初始化
  与「连接设置」手动连接均传入 `window.location`。
- ✅ **(3) 连接设置持久化（根因：刷新丢失指定 IP）**：手动连接存入
  `agentj.connection`，刷新自动恢复并重连；连接设置弹窗回填已保存值；
  显式设置优先于 `VITE_AGENTJ_WS_URL`。
- ✅ Oracle 评审：无 Critical；2 Important（dashboard 无 `--token` 致告警不可执行 /
  空 token 压制告警）与 2 Minor（IPv6 与端口打印 / Tauri https scheme 守卫）
  已全部修复并回归锁定。
- ✅ TDD 回归锁定：Rust +7 tests（553 → 560）/ SDK +6（37 → 43）/ Web +8（231 → 239）；
  全量门禁绿（fmt / clippy 零警告 / drift / web build / tui 71 / desktop-ui /
  check-version）。
- ✅ 文档同步 + 版本 0.16.1 → 0.16.2（PROTOCOL_VERSION 保持 0.13.0）。

## 遗留 / 下一步

- 无开放 bug。
- 待确认：新建会话仍会自动收藏（v0.11 遗留，测试 `new session becomes active and favorited`）
  ——用户尚未表态，暂保留。
- **下一特性（已确认，方案 2）：内核直接托管 Web UI** —— `serve` 增加静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接打开
  `http://<host>:4096/` 即用、WS 同源零配置；按 feature 流程（design → plan → dev）
  立项时需覆盖：dist 路径发现/`--web-dist` 参数、`/` 与 `/ws` 路由共存、
  token 下静态资源策略、测试与文档。
