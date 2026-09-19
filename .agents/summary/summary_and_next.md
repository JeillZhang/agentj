# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台修复（v0.16.3）—— 项目列表迁入顶栏菜单 + 文件树永久「加载中」修复**
> ——来源=用户反馈：① 左栏「项目」需移至右上角「...」菜单（连接设置下方，二级菜单选择项目）；
> ② 文件 tab 永久「加载中」，无法显示工作区文件树

## v0.16.3: Web 工作台修复 ✅（本次完成）

- ✅ **(1) 项目列表迁入顶栏菜单（无协议变更）**：左栏「项目」区块（项目分组 + 底栏
  「+ 添加项目」+ 添加对话框）整体移除，迁入 `TopbarMenu` 二级菜单（位于「连接设置」
  下方）：列出全部项目（当前项 ✓ / `menuitemradio` + `aria-checked`）、选择即切换
  当前项目（`setCurrentProject` → localStorage + 新 `agentj:current-project-updated`
  事件）、「＋ 添加项目」复用 ModalDialog；`RunPanel` composer picker 与
  `UnifiedSidebar`（新建会话归属）经事件同步；侧栏保留扁平最近会话列表；
  App hero 悬空 `.aj-project-new` 选择器顺带清理；`.gitignore` 加 `.playwright-mcp`。
- ✅ **(2) 文件树永久「加载中」修复（三层根因链）**：
  ① 长驻 Vite dev server 不监听 root 之外 `packages/sdk` 源（实测 touch 不失效）
  → 陈旧 SDK（无 `METHODS.fileList`，PROTOCOL_VERSION 0.12.1）→ `JSON.stringify`
  丢弃 `method` 字段；
  ② 内核 `handle_request_from` 对 schema 非法请求回 `-32700` + `id:null` →
  客户端 Promise 永不 settle → 「加载中」永久；
  ③ SDK 无 method 校验。
  修复：内核 `parse_request`（可确定 id → `-32600` 回显 id；语法错误仍 `-32700`+null，
  对齐 docs/specs/ipc-protocol.md §5）+ SDK `RpcClient.request` 空/未定义 method
  直接抛 `invalid RPC method`（不再发出畸形帧）；环境侧重启长驻 dev server
  （本机 5173/4096 已按原参数重启为 0.16.3）。
- ✅ 实机验证（Playwright，:5174 全栈）：文件 tab 渲染工作区树（README.md /
  rust-toolchain.toml / 6 个文件；修复前永久「加载中」）；「...」菜单顺序 =
  连接设置… → 项目 → 继续会话…；子菜单选择/新建项目 → ✓ 标记 + composer picker
  即时同步 + localStorage 持久化；侧栏无「项目」区块/底栏；内核方法缺失帧已回显
  id 99 + `-32600`（scratch :14098 实测）。
- ✅ TDD 回归锁定：Rust +4 tests（560 → 564）/ sdk +3（43 → 46）/ Web +7
  （239 → 246，含 PR #53 同期 9801187「连接设置持久化对畸形输入加固」+2 与本次 +5）；
  全量门禁绿（fmt / clippy 零警告 / drift 2 passed / web build /
  tui 71 / desktop-ui / check-version 0.16.3）。

## 遗留 / 下一步

- 待确认：新建会话仍会自动收藏（v0.11 遗留，测试 `new session becomes active and
  favorited`）——用户尚未表态，暂保留。
- 观察（非本批改动）：Vite dev server 不监听 root 外 `packages/sdk`（alias 直连源码）
  —— 改 SDK 后需重启 dev server（已记入 AGENTS.md Commands 注记）；内核回显 +
  SDK 拦截后同类故障将「快速失败」（显示「加载失败」+ 重试）而非永久加载。
- **下一特性（已确认，方案 2）：内核直接托管 Web UI** —— `serve` 增加静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接打开
  `http://<host>:4096/` 即用、WS 同源零配置；按 feature 流程（design → plan → dev）
  立项时需覆盖：dist 路径发现/`--web-dist` 参数、`/` 与 `/ws` 路由共存、
  token 下静态资源策略、测试与文档。
