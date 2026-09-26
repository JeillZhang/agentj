# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；
CLI、TUI、Web、Desktop 均通过 `agentj-ui` 协议消费者接入，任意语言可开发前端。

> 当前版本：**v0.24.0**——中栏 Composer 滚动自动收缩（纯前端，无协议变更，PROTOCOL_VERSION 保持 0.15.0）：上滚离开底部（>48px）自动平滑收缩为发送按钮原位「展开输入框」按钮（X/Y 零位移 + aria 同步），下滚回底（<40px）或点击平滑展开（点击聚焦 textarea、自动展开不抢焦点）；草稿全程保留；待答提问不收缩且强制展开；收缩期冻结 `--aj-composer-h`、程序化跟随不收缩、260ms 锁 + 8px 迟滞；`composerCollapse.ts` 纯状态机（7 测）+ `ConversationFeed` 意图双判（5 测）+ `RunPanel` 接线（10 测）+ `composer.css` 分段收拢（4 测）+ 评审修复波（真实浏览器点击展开焦点 / 展开期 --aj-composer-h 保持 / 锁过期回底重判，3 测）。门禁全绿（cargo fmt / **660 Rust tests** / clippy 零警告 / drift 2 passed / sdk **54** / **web 609 tests（52 files）** / **tui 146 tests（19 files）** / web build / check-version **0.24.0** / desktop-ui）；PROTOCOL_VERSION 保持 0.15.0；工作区 0.23.1 → 0.24.0。历史版本：**v0.23.1**——补齐 v0.23.0 计划遗漏（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）：(a) Web `changes.ts` 归约状态拆分 `ChangesState {summary, history}`——summary 按 `(run_id, path)` upsert、history 按设计决策 #3/#15 append-only（按 seq 排序 + 按 `tool_call_id` 分组），`ChangeList` 历史抽屉可达；(b) `ChangeList.tsx` 消费 `history` prop、`RightPanel.tsx` run 分组经 `App.tsx` 传入 `groups`/`history`/`highlightedPath`/`onSelectRun` 真正接线（run 组头可点击、每组独立空态；无分组保留 children/手风琴回退）；(c) `rightpanel.css` 补齐 `run-group*`/`history-drawer`/`hist-*`/`trunc-hint`/`src-badge*` token 化样式；(d) 补齐计划缺失测试：protocol T0（+5）/ SDK T2（+7）/ IPC T5（+4）/ TUI T9（+4）/ web（+21）；(e) 删除 `agentj-core` 死代码 `bash_file_changes` 及单测（−1），`git_sample` 测试改断言生产路径 `source == Some(Bash)`；(f) 用户直报修复：右栏空态文案 `待协议支持` → `暂无任务`/`暂无变更`（TodoPanel/ChangeList），`handleSelectRun` 改用 `todoState.focusRun` 保留非枚举 `byRun`（修复点击 run 组头后分组丢失回退旧空态）；web +2 回归测。门禁全绿（cargo fmt / **660 Rust tests** / clippy 零警告 / drift 2 passed / sdk **54** / **web 580 tests（51 files）** / **tui 146 tests（19 files）** / web build / check-version **0.23.1** / desktop-ui）；PROTOCOL_VERSION 保持 0.15.0；工作区 0.23.0 → 0.23.1。历史版本：**v0.23.0**——右边栏运行详情：`run_id` 维度（UUID v4 per `session/run`，per-run `revision`/`seq`）+ `FileChangeSource=write|bash`（`total_bytes` 截断披露）+ `session/history {run_id}` 过滤（非法→-32602，其余 kind 全量）+ Web 右栏按 run 分组/徽标/截断 hint/历史抽屉 + FileTree 跳转 + TUI 兼容；`PROTOCOL_VERSION` 0.14.0 → 0.15.0，52 schemas。历史版本：**v0.22.0**——限流指标导出 + 资产清单扩展/运行期抽检 + TUI 服务端搜索 + Web 键位收口（四项，三处加性 wire 变更，PROTOCOL_VERSION 0.13.0 → 0.14.0）：① 鉴权门控 `GET /metrics`（Prometheus 文本 `text/plain; version=0.0.4` + `no-store`；聚合计数 + per-IP 序列 Top-50 截断 + `integrity` 摘要；回环/无 token/有效 cookie 直出，远程无 cookie → `401` 不重定向）与 `GET /version` 的 `integrity.runtime` 加性嵌套（`rounds`/`checked`/`mismatched`/`missing`/`last_run_age_secs?`）；② `asset-manifest.json` 覆盖扩展至 dist 实目录全部部署文件（含 `public/` 透传，bundle 键优先）+ 内核 60s/8 文件游标轮转后台抽检（不符 `warn!` 按路径去重）；③ `file/list` 加性 `query`（`path` 子树、忽略 `depth`、访问预算 20_000、rank 前缀>子串>路径、Top-100）与 `total`，TUI 搜索改防抖 150ms 服务端查询（旧内核无 `total` 自动降级本地过滤），展开态键改为会话有效 `project_root`；④ Web 共享 `useRovingFocus` 收口 TopbarMenu/SessionListMenu/SidebarTabs/SettingsDialog + 侧栏底部快捷键提示行（三态文案组件；Web 当前两态可达，搜索态待 Web 搜索特性）。门禁全绿（cargo fmt / **646 Rust tests** / clippy 零警告 / drift 2 passed / sdk 47 / **web 552 tests（51 files）** / **tui 142 tests（19 files）** / web build / check-version **0.22.0** / desktop-ui）。历史版本：**v0.21.1**——v0.21.0 验收修复（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.13.0）：实机验收（Playwright chromium headless / HTTP / TUI）PASS；发现并修复 2 bugs——**BUG-011（low）**：按 `f` 切换侧栏视图后焦点掉出侧栏（`<body>`，后续 `j`/`k`/`r`/`Esc` 失效直至重新点击/Tab）→ `SidebarTabs` 活动 tab 按钮 `data-tab` + `UnifiedSidebar.toggleTab` 切换后 park 焦点（`89396f3`）；**BUG-012（low）**：焦点在文件树行（`j`/`k` 移动后）按 `r` 刷新 → 树卸载重挂、焦点掉出侧栏（BUG-011 同类、`r` 路径未覆盖）→ `r` handler 在 `refresh()` 前 park 焦点到 files tab 按钮（`71245d5`）；均回归 verified、零新增 Bug。web 537 → **539 tests（50 files）**；工作区 0.21.0 → 0.21.1。历史版本：**v0.21.0**——限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（四项，无 wire 变更，PROTOCOL_VERSION 保持 0.13.0）：① 登录限流可观测性——`agentj-ipc` `login_limiter` 在状态变化点发 tracing 事件（锁定新触发 `warn`、成功登录 `info`、逐次失败/锁定期拒绝 `debug`），进程内聚合计数经 `GET /version` 加性 `login_limiter` 块暴露（`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，无 per-IP 明细），CLI 日志支持 `RUST_LOG`（`EnvFilter`，未设默认 `info`，行为与 v0.20.0 逐字一致）；② 资产完整性——构建期生成 `dist/asset-manifest.json`（`sha384` 覆盖全部 bundle 产物含动态 chunk + SRI 注入后的 `index.html`，单遍哈希与 SRI 逐字节一致；排除 `version.json`/自身/`public/` 透传），内核启动逐文件校验并并入 `startup_warnings()`（缺失/不可解析/不符/文件缺失，警告不阻断服务），摘要进 `GET /version` 加性 `integrity` 块（`manifest_present`/`checked`/`mismatched`/`missing`）；③ TUI Files 搜索——`s` 进入搜索（`file/list {path:null, depth:8}` 单请求扁平索引 + 客户端过滤），可打印字符过滤（`Esc` 清空后退出）、`Enter` 跳转并展开祖先、`r` 重建、`truncated` 显示 `· partial`，展开状态按本地项目键持久化 `filetree.json`（per-key ≤100、恢复 ≤50、失效路径静默跳过）；④ Web 焦点作用域——焦点位于侧栏时 `f`/`n`/`r`/`j`/`k`/`Esc` 生效且 composer 输入零误触，文件树/会话列表 roving 键盘导航，`ModalDialog` 焦点恢复，**Tab 保持原生**。门禁全绿（cargo fmt / 622 Rust tests / clippy 零警告 / drift 2 passed / sdk 47 / web 537 tests（50 files）/ tui 130 tests（19 files）/ web build / check-version 0.21.0 / desktop-ui）。历史版本：**v0.20.1**——v0.20.0 验收修复（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.13.0）：实机/HTTP 验收 PASS——登录限流（第 5 次失败 429 + Retry-After 30s、锁定升级、到期 302 + cookie、成功清零、开放重定向守卫）、版本探测与 SRI（sha384 复算匹配、缺失/不一致启动警告、`GET /version` 未鉴权 + no-store 且不走 SPA fallback）、`serve_connection` 清理（`?token=` 403/101、cookie+Origin 通道、回环直通）与 TUI Files 视图；发现并修复 2 bugs——**BUG-009（high）** 新增全局 `f` 快捷键劫持 composer 输入（修复：`composer ⇄ sidebar` 焦点模型，Tab 切换焦点、单字母快捷键仅 sidebar 焦点生效，`5eea70e`）；**BUG-010（low）** sidebar 提示词在焦点模型下宣传失效键位（`12ec55a`）；均回归 verified、零新增 bug；TUI 85 → **88 tests（17 files）**；工作区 0.20.0 → 0.20.1。历史版本：**v0.20.0**——Web 加固 + TUI Files 视图（四项，无 wire 变更，PROTOCOL_VERSION 保持 0.13.0）：① `serve_connection` 遗留 token 参数清理（`TcpIpcServer` 死 token 字段一并移除；`WsServerState.token`/`WsQuery.token`/`?token=`/`initialize.token` 语义不变）；② 登录限流——`agentj-ipc` 新 `login_limiter` 模块（按来源 IP 计数，第 5 次失败起锁定 30s、×2、封顶 900s；`429 + Retry-After`（秒，向上取整 ≥1）；锁定期不校验 token 且新尝试升级锁定；成功清零；idle_ttl 1800s 懒淘汰 + max_tracked 1024 淘汰；回环同样计数）+ `/login` POST 接线透传 peer；③ 版本探测与 SRI——`apps/web/build/version-sri.ts` + `version-sri-plugin.ts`（`writeBundle` 生成 `dist/version.json`（product/protocol/build_time，单一真源 = sdk config）+ 入口 HTML 同源 script/stylesheet/modulepreload 注入 `sha384` `integrity`，本地引用缺失即构建失败；零新依赖），内核 `WebConfig` 读取 `DistVersion` + `startup_warnings()`（缺失/不可解析/不一致三类启动警告，不阻断服务）+ 未鉴权 `GET /version`（200 JSON + `Cache-Control: no-store`，显式路由优先于 SPA fallback）+ `TcpIpcServer::web_config()`，CLI `serve` 启动打印 dist 版本警告，Web `ConnectionStatus` 版本行「版本 UI x · 内核 y」（不一致 `data-state="warn"` + `⚠`）、`App` 消费 `initialize.server_info.version`、SDK barrel 导出 `VERSION`；④ TUI Files 视图——`files.ts` 纯函数 + `FilesView` + `useFileTree`（懒加载 `file/list` depth=1、stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取）、Sidebar Threads/Files 切换（`f`）+ 键位（`j,k,↑,↓` 移动 / `Enter,l,→` 目录开合 / `h,←` 收起或跳父级 / `r` 刷新）+ mock kernel `file/list` e2e；目录聚合状态字母色义 M→warning、A→success、U→textCaption、D→error；Oracle 全量复核 PASS（无 must-fix）+ 修复波（TUI stale-reply 守卫 + 父级跳转 clamp + Web warn 色 CSS 契约测试）；v0.19.0——内核托管 Web UI + 文件树状态补强（无 wire 变更）：① 内核同端口静态托管内置 Web UI——`agentj-ipc` 新 `web` 模块 `build_router` 在 `ws_router` 上 `nest_service("/assets", ServeDir + immutable 长缓存)` + `fallback_service(ServeDir::fallback(ServeFile(index.html)) 根 no-cache、未知页面 200)`；`/ws` 与 `/login`/`/logout` 显式路由优先且不在门后（防重定向环，错拼 `/assets/*` 仍 404）；② 混合鉴权登录门——回环 peer 或未配置 token 直出，非回环 + token 且无有效 cookie → `302 /login?next=<同源相对路径>`（`next` 白名单防开放重定向）；内嵌登录页（无 JS）POST 校验 token 后颁发不透明会话 cookie（`agentj_session`，`HttpOnly; SameSite=Lax; Path=/`，无 Max-Age，内存态重启即失效）/ `/logout` 撤销；③ WS 第三鉴权通道——有效会话 cookie + 同源 Origin 免 `?token=`/`initialize.token`（服务端为 cookie 通道注入 initialize token；另两通道语义不变）；④ `serve`/`dashboard` 新增 `--web-dist <PATH>`（四级发现 flag/env → cwd `agentj-ui/apps/web/dist` → exe 相对；启动打印 `http://<host>:<port>/`，缺失告警）；⑤ 文件树目录聚合状态——`file/list` 目录 `git_status` 取后代最高优先级字母（`U>D>M>A`，`srcx/` 不误伤 `src/`），Web `FileTree` 目录行渲染状态字母、移除冗余圆点。实机 + `e2e_http_static`（持有 stdout reader）覆盖；v0.18.1——token 统计修复（用户直报，纯前端，无协议变更）：① `Σ` 会话 token 读出从 composer 底栏（模型选项旁）移至输入框 header、紧邻「deepseek-chat」模型 chip 右侧（`RunPanel` `.aj-composer__header-meta`；`ComposerOptions` 移除 `usageTotal`/`.comp-usage`）；② 统计拆分输入/输出：`Σ 2.5k · 输入 2k · 输出 500`（新 `tokenUsage.ts` `formatUsageSummary` + `formatUsageTitle`；`formatTokenTotal` k/M 一位小数、999.95k 起进位 M），tooltip/aria-label 精确值 + 合计；header `flex-wrap` 窄视口读出换行右对齐；实机 Playwright 15/15 + Oracle 复核 PASS + 修复波；v0.18.0——Web 三项增强（用户直报，纯前端，无协议变更）：① 中栏 composer 75% 宽居中浮动于底部（`--aj-composer-h` 动态底部预留：内容延伸至卡片底部、滚动到底时末条消息停在输入框顶上方 ~16px；窄视口 `min-width: min(340px, calc(100% - 16px))` + `.comp-tools` 换行兜底）；② 权限选择器（逐次确认/自动允许：bypass 时客户端自动回 `permission/respond allow` 不弹窗，内核 deny/路径越界仍强制拒绝；跨标签 storage 同步）；③ 会话 token 总量 `Σ 5.2k`（`formatTokenTotal`：k/M 一位小数、999.95k 起进位 M；无用量隐藏）；实机 Playwright 验证 + Oracle 复核 PASS（6 minor/nit 修复波 + 2 补测）；v0.17.2——界面修复（用户直报 2 项，纯前端，无协议变更）：设置「显示」字号改为逐区 px 数值 + 横向「滑杆 + 数字输入 + px」行（`FONT_SIZE_LIMITS` 校验 + 旧 sm/md/lg/xl 迁移，行高/密度横排），消除弹窗纵向过度拉伸；`ModalDialog` 弹窗（设置等）新增 `.aj-dialog-frame` + 8 向指针手柄（4 边/4 角），`dialogResize.ts` 几何（对边锚定 + 最小 320×200 + 视口 8px 夹取 + per-dialog 下限，设置弹窗 560px）；实机 Playwright 验证；v0.17.1——v0.17.0 验收修复（BUG-008：窄视口 375–390px 左栏会话列表菜单横向夹取 + 「过滤/排序」飞出层按空间方向自适应 `.aj-menu__flyout--left`，resize 重算，桌面 ≥1024 行为不变）+ Web 工作台六项增强（纯前端，无协议变更）：代码块卡片化（`CodeBlockCard` + `copyText` clipboard→execCommand 降级链 + `rehype-highlight` 高亮）/ 显示设置（`displayPrefsStore` + 设置「显示」tab + `displayPrefs.css` 覆盖层 + `--aj-font-*`/`--aj-lh-conv`/`data-density`/`data-reduced-motion`）/ 文件树源码与 db 图标 + 文件名特例 / 左栏 `SessionListMenu`（清理 + 过滤/排序向右飞出二级菜单，设置「会话列表」移除）/ 项目编辑（`projectStore` 扩展字段 + `ProjectEditDialog` + composer 行内编辑 + `project_root` 随 `session/new` 打通 + 项目默认模型/Agent 应用）/ 设置「项目」tab（全局默认 + 列表 + 守卫删除 + `ModalDialog` 嵌套栈）；v0.16.5——Web 工作台四项修复：右栏折叠/展开单段右→左平滑动画（外层 `width` 过渡 + 内层 `align-self: flex-end` 右缘锚定、内容零回流；仅 ≥1024，拖拽期间兄弟选择器禁用过渡）/ 顶栏「...」五项菜单（项目、继续会话…、收藏会话、设置…、帮助；项目与收藏会话 hover 左向飞出二级菜单 + Esc 分层 + 键盘可达；排序/过滤/演示数据/连接设置迁入「设置」弹窗，左 tablist 分类 + `sidebarPrefsStore` 持久化）/ 右栏任务/文件变更成对面板（单开铺满至底部、双开 `role=separator` 拖拽/键盘调节 + `agentj.layout.rightSectionSplit` 比例持久化、双击/Home 复位）/ 文件树分类图标（Folder/FolderOpen + 扩展名 lucide 图标 + 任意深度缩进）；v0.16.4——Web 工作台体验修复（六项 + 验收 1 bug）：状态灯 hover 连接面板（左上角仅 `agentj` + 状态灯；⋯ 菜单移除状态区块）/ 右栏 VS Code 手风琴（折叠 = 紧凑标题行堆叠、展开 = 向下延展、空数据保留标题行、localStorage 记忆默认全展开）/ 全局 token 化滚动条 / composer 选项弹层二次点击收起（`OptionPopover.anchorRef`）/ 新建会话不再自动收藏 / 验收修复 BUG-007（顶栏 `z-index: 20` 高于侧栏）；v0.16.3——Web 工作台修复：左栏「项目」迁入右上角「...」菜单二级菜单（「连接设置」下方，选择项目 + 「＋ 添加项目」+ `agentj:current-project-updated` 同步）与文件 tab 永久「加载中」修复（内核非法请求回显可确定 id `-32600` + SDK 拒绝空/未定义 method）；v0.16.2——远程访问修复：内核 `--host` 绑定（默认仍 127.0.0.1，`0.0.0.0` 可局域网/远程直连）+ `dashboard --token` + Web 页面地址派生与连接设置持久化（修复远程访问误连 `ws://127.0.0.1:4096/ws`、刷新丢失指定 IP）；v0.16.1 三项修复：会话删除移至星标下方 + 顶栏模型 chip 移除 + 单击不再自动收藏；v0.16.0 六项修复：协议版本 semver 兼容 + composer 弹层锚定 + hover/主题对比度 + 会话管理 + 全宽拉伸；协议扩展：三栏工作台真实数据源
> （`file/list` 目录懒加载 + git 状态点；`$/progress` 新增 `kind:"todo"`
> 全量快照 + 会话内单调 `revision` 与 `kind:"file_change"` unified diff；
> `$/askQuestion` + `question/respond` owner 定向提问；会话级 `project_root`；
> 新 crate `agentj-question`，21 方法 / 52 schemas / PROTOCOL_VERSION 0.13.0）
> （v0.14.6 三栏工作台：Sessions/Files 侧栏 + 会话中栏 + 运行详情右栏 +
> composer 模型/Agent/熵变选择器 + 模型提问卡片 + demo 模式；
> Web / TUI / Desktop + SDK + tokens，17 crate + npm workspaces monorepo，
> 共享 `packages/sdk` + `packages/protocol` + `packages/tokens`，后端零改造，frontend stays in-repo）
> v0.16.4 修复：左上角仅 `agentj` + 状态灯（移除 `connected` 文字；连接信息移入状态灯
> hover 面板，闭合不在 DOM）/ ⋯ 菜单移除状态区块 / 右栏「任务」「文件变更」VSCode 手风琴
> （折叠 36px 紧凑堆叠、展开向下延展、空数据保留标题行，`agentj.layout.rightSections` 记忆）/
> 全局 token 化滚动条（webkit + Firefox）/ composer 模型/agent/熵变弹层二次点击收起
> （mousedown→click 竞态修复）/ 新建会话不再自动收藏（仅星标触发）/
> BUG-007 顶栏 z-index 5→20（hover 面板被侧栏遮挡）。
> Web 246 → 263 tests / sdk 46 / Rust 564；PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.3 → 0.16.4。
> v0.16.5 修复（四项，无协议变更）：(1) 右栏折叠/展开单段右→左平滑动画——外层
> `.aj-shell__right` `width` 过渡（40px ↔ 存储宽，仅 ≥1024；去除不可过渡的
> flex-basis/min/max 钳制），内层 `.aj-right` `align-self: flex-end` 右缘锚定
> （内容零回流、仅揭示；collapsed 头部右对齐 4px 内边距使 « 按钮落在 40px 可视窗），
> SplitPane 拖拽期间以兄弟选择器 `.aj-splitter[data-side="right"][data-dragging] ~
> .aj-shell__right` 关闭过渡；(2) 顶栏「...」菜单重构为恰好五项（项目/继续会话…/
> 收藏会话/设置…/帮助）：项目与收藏会话 hover 左向飞出二级菜单（`aria-haspopup/
> expanded`、桥接 + 120ms 关闭延迟、Esc 分层关闭、键盘可达、点击兼容触屏），
> 排序/过滤/演示数据/连接设置迁入新 `SettingsDialog`（左 `role=tablist`
> 连接/会话列表/演示数据 + 右 tabpanel；↑↓/Home/End 切换），新增
> `sidebarPrefsStore`（`agentj.sidebarPrefs` 排序/过滤持久化）；(3) 右栏
> 「任务」「文件变更」成对面板：单开铺满至面板底部；双开 `role=separator` 拖拽
> （指针位置比例 = flex-basis 精确逆映射，零漂移；↑↓ ±2%/Shift ±8%、双击/Home
> 复位），比例持久化 `agentj.layout.rightSectionSplit`（0.15–0.85，缺省自动）；
> (4) 文件树图标体系（新 `fileIcons.ts`：Folder/FolderOpen + 扩展名分类 lucide
> 图标 + 21 单测）+ 任意深度内联缩进 + token 化美化。验收（实机 Playwright）4 项
> 全过。Web 263 → 308 tests；PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.4 → 0.16.5。
> v0.16.3 修复：左栏「项目」区块（分组 + 添加按钮 + 对话框）迁入右上角「...」菜单
> 二级菜单（「连接设置」下方；当前项 ✓；选择即切换当前项目并同步 composer picker）/
> 文件 tab 永久「加载中」三层根因修复（长驻 dev server 陈旧 `packages/sdk` +
> 内核 `id:null` 错误响应不可匹配 + SDK 无 method 校验；内核 `-32600` 回显 id +
> `RpcClient.request` 拦截空/未定义 method）。
> v0.16.2 修复：`serve`/`dashboard` 新增 `--host`（默认 127.0.0.1 本机信任，0.0.0.0 局域网/远程）
> / `dashboard --token` / 非回环无有效 `--token` 告警 / Web 默认 WS 地址按页面来源派生
> （https → wss，Tauri 桌面保持明文回环）/ 连接设置持久化（刷新自动恢复重连、弹窗回填）。
> v0.16.1 修复：左栏会话删除按钮移至收藏星标下方（右栏堆叠、行高 56px）/
> 右上角模型 chip 移除（composer 与 usage meta 行已展示）/
> 会话单击不再自动收藏（仅星标按钮触发，回归测试锁定）。
> v0.16.0 修复：协议版本 banner 误报（semver major 兼容，0.12.1 client vs 0.13.0 server）/
> composer 选项弹层偏移 / 选项与会话行 hover / light 主题对比度 /
> 左栏会话管理（逐行删除 + 一键清理，`session/delete` 幂等）/ 对话区/composer 全宽拉伸。
> v0.15.2 修复：session 生命周期方法 dispatch 缺失（-32601）：session/status（Web/TUI 会话点击）
> + session/archive / session/delete 补齐（-32802 / -32803 SessionInactive + error.data）。
> v0.15.1 验收修复 2 bugs：serve --port/dashboard trajectory 接线 / initialize protocol_version 0.13.0。

## Features

- [x] **IPC 协议契约**：JSON-RPC 2.0 信封 + 错误码 + 21 方法
  （lifecycle 4 / session 9 含 subscribe/unsubscribe 与 list/history /
  kernel/status 1 / 通知 4（`$/cancelRequest` `$/progress` `$/permissionRequest`
  `$/askQuestion`） / `permission/respond` + `file/list` + `question/respond`）
- [x] **权限审批系统**：`$/permissionRequest` + `permission/respond`
  + 60s 超时自动 Deny + allow_always 会话级 + 取消即拒绝
- [x] **JSON Schema**：52 Schema（schemars 生成，提交件 + 漂移门禁）
- [x] **方法注册表** `protocol/methods.json`（method ↔ schema/方向/类型）
- [x] **人类可读协议规范** `docs/specs/ipc-protocol.md`（11 节）
- [x] **跨语言验证**：TS 绑定 `agentj-ui/packages/protocol`（类型生成 + tsc）
- [x] **内核与模块注册表**：Kernel 混合总线 + `Kernel::modules()`
  + `ToolCallReply` + `AsyncKernelService` + `ServiceRegistry`
- [x] **Agent 循环（流式）**：`TurnRunner` 总线化编排 + `AgentLoopModule`
  + `ToolExecutorModule` + `SubAgentService`
- [x] **内置工具**：bash / glob / grep / read / write + todo + ToolExecutor
- [x] **MCP 桥**：子进程 MCP 客户端 + McpConnectionManager 长驻连接
- [x] **JSON-RPC IPC**：stdio + TCP 多客户端 + WebSocket 网关
  + per-session 路由 + 权限属主门控
- [x] **CLI run/serve**：`agentj run` / `serve [stdio]` / `serve --port`
  + `dashboard --port`（常驻多客户端，`/ws` 同端口共存）
- [x] **模型提供方**：DeepSeek + opencode-go + Ollama + Mock
  （三线格式，ProviderFactory 按 kind 实例化）
- [x] **事件总线**：`EventBus` 订阅/发布 + 重入防护
  （EventGuard thread_local depth/stack + MAX_EVENT_DEPTH=16）
- [x] **子代理 / Skills / 插件 / 会话持久化 / 守护内核 / 用量统计 / License**：
  均已落地（见 AGENTS.md 架构节）
- [x] **WebSocket 网关**：`ws://127.0.0.1:<port>/ws`（axum + hyper-util
  + `?token=` 403 + `initialize.token` -32806 双通道，WS 不参与全局关闭；
  v0.16.2 起 `serve/dashboard --host` 可绑定局域网/远程地址）
- [x] **agentj-ui SDK**：`packages/sdk`（RpcClient + ResponseRouter
  + WebSocketTransport + TcpTransport + resolveWsUrl + METHODS + PROTOCOL_VERSION）
- [x] **agentj-ui Web**：`apps/web`（Vite + React 19，对齐项：常量化方法名、
  `?token=` 透传、`protocol_version` 同源、会话收藏、composer、
  `$/progress` 流式、审批对话框、usage 结算）
- [x] **agentj-ui TUI**：`apps/tui`（Bun + @opentui/react，全功能：
  连接管理、会话新建/切换/继续、composer 多行/@补全//palette、
  流式渲染、审批模态、模型只读展示、主题切换、usage 结算、优雅退出）
- [x] **agentj-ui Desktop**：`apps/desktop`（Tauri v2 壳，套 `apps/web` 产物，
  输出 deb / AppImage / dmg / msi 四件套，check-artifacts.sh 校验）
- [x] **Visual 2.1 refine（v0.12.3）**：`packages/tokens` 扩展至 19 keys（shadowSoft/shadowRaised/radiusXl/textCaption + spacing/font/z/motion/icon vars）
  + Web 精细化：App Shell 居中列 + 抽屉 scrim / 悬浮胶囊 Composer（22px + shadow + sticky）/ 卡片化 ConnectionBar 与 SessionPanel / 气泡与 timeline 工具卡 / Trajectory timeline / Hero 空状态与 dashed trigger / Dialog 固定遮罩 + lucide 替换 emoji
- [x] **Visual 2.2 Opencode redesign（v0.13.0）**：UnifiedSidebar 256px（Threads + Project 文件夹 + 底栏）+ local Project（localStorage）+ Composer 增强（Project picker + model chip + status popover + @/slash）+ 密度收紧（conversation gap 8px）+ TUI 32char 侧栏 + a11y/CLS/虚拟化 + Desktop 同步
- [x] **三栏工作台（v0.14.6）**：`WorkspaceShell` 三栏布局（Sessions/Files 左栏 + 会话中栏 + 运行详情右栏 `TodoPanel`/`ChangeList` unified diff）+ `SplitPane` 拖拽/键盘/双击与宽度持久化 + `<1024` 右抽屉 + 右栏满高 + 任务/文件变更标题点击折叠 + composer 模型/Agent/熵变选择器（互斥 + 持久化）+ `AskQuestionCard` 模型提问卡片 + `TopbarMenu` 顶栏化（含 demo 开关 + `demoData` 驱动 TODO/文件树/变更/提问卡）+ tokens 24 keys + `LAYOUT` 常量 + 共享 `ModalDialog`（连接设置/继续会话/添加项目统一）
- [x] **协议扩展（v0.15.0）**：`file/list` 沙箱化目录懒加载 + git 状态点（M/A/U/D）；`$/progress` 新增 `kind:"todo"`（全量快照 + 会话内单调 `revision`）与 `kind:"file_change"`（write 自研行级 diff / bash git 前后采样，unified diff ≤64 KiB）；`$/askQuestion` + `question/respond` owner 定向提问（重复响应幂等、超时/取消/owner 断开按 skip 结算）；会话级 `project_root`（路径基准五处同源）+ 新错误码 `-32807 PathNotAllowed`；`session/history` 回放重建 TODO/变更/Q&A；新 crate `agentj-question`（17 crates）；Web 三栏工作台 `TodoPanel`/`ChangeList`/`FileTree`/`AskQuestionCard` 切换真实协议驱动（`deriveChanges` 半推导退役，demo 模式保留为可切换预览通道）+ TUI runview 只读 TODO 区块
- [x] **Web 工作台体验修复（v0.16.0）**：六项修复 —— 协议版本 semver 兼容语义（major 相同即兼容，仅 major 不匹配才显示 banner，0.12.1 client vs 0.13.0 server 不再误报）；composer 选项弹层逐触发器锚定（`.sel-wrap`），不再偏移到发送按钮附近；选项/会话行 hover 反馈（背景 + accent + 过渡）；light 主题对比度（`--aj-border #E4E7EA→#CBD2DB`、`--aj-raised #F9FAFB→#E9EDF1`）；左栏会话管理（逐行左侧删除 + 一键清理，`session/delete` 排除当前会话、-32802/-32803 幂等容忍、`session/list` 过滤 deleted）；中栏对话区/composer 移除 720px 居中上限、全宽拉伸。Web 219 → 230 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.15.2 → 0.16.0
- [x] **Web 工作台六项增强（v0.17.0）**：纯前端，无协议变更。① 对话代码块卡片化：`rehype-highlight`（hljs common 语言集，`.hljs-*` 映射现有语义色变量）+ `CodeBlockCard`（`figure > figcaption + pre`：左上折叠/展开保留 header + 语言标签、右上复制；折叠不卸载，流式追加保持折叠态）+ `copyText`（`navigator.clipboard` → textarea/`execCommand` 降级链 + 失败瞬态，http 局域网非安全上下文可用）。② 显示设置：`displayPrefsStore`（`agentj.displayPrefs`：对话/左栏/右栏/代码四档字号 + 行高紧凑 1.4/默认 1.55/宽松 1.8 + 密度 + 减弱动画）+ `SettingsDisplayPanel`（设置「显示」tab + 恢复默认）+ `displayPrefs.css` 覆盖层 + `WorkspaceShell` inline `--aj-font-*`/`--aj-lh-conv`/`data-density` + App 写 `documentElement[data-reduced-motion]`。③ 文件树 `fileIcons.ts` 扩展主流源码扩展名 + db 类（`Database`）+ 文件名特例（Dockerfile/Makefile/CMakeLists.txt → code）+ `.file-icon--db` 配色。④ 左栏 `SessionListMenu`：清理所有会话…（无可删禁用）+ 过滤/排序悬停向右飞出二级 `menuitemradio`（镜像 TopbarMenu 延迟/桥接/Esc 分层/键盘，直写 `sidebarPrefsStore`），设置弹窗「会话列表」tab 移除。⑤ 项目编辑：`projectStore` 扩展 `workDir/isDefault/defaultModel/defaultAgent` + `updateProject`（默认互斥）/`deleteProject`（Inbox 守卫 + 归属回退 + 计数）+ `ProjectEditDialog` + composer picker 行内编辑 + `project_root` 随 `session/new` 打通 + 项目默认模型/Agent 随切换应用。⑥ 设置「项目」tab：`SettingsProjectPanel`（全局默认 + 新项目工作目录 + 列表编辑/删除 + Inbox 不可删 + 删除确认会话回退数）+ `ModalDialog` 模块级模态栈。Web 308 → 401 tests（43 files）/ sdk 46 / Rust 564 tests / tui 71；新依赖 `rehype-highlight`；PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.5 → 0.17.0
- [x] **token 统计修复（v0.18.1）**：用户直报（纯前端，无协议变更）—— ① `Σ` 会话 token 读出从 composer 底栏（模型选项旁）移至输入框 header「deepseek-chat」模型 chip 右侧：`RunPanel` 新增 `.aj-composer__header-meta`（chip + 读出右对齐 + 内部换行兜底），`ComposerOptions` 移除 `usageTotal` prop 与 `.comp-usage`（options.css 同步移除）；② 统计拆分输入/输出：`Σ 2.5k · 输入 2k · 输出 500`（新 `tokenUsage.ts`：`formatTokenTotal` 迁出 + `formatUsageSummary` 摘要 + `formatUsageTitle` 精确 tooltip/aria-label），header `flex-wrap` 窄视口读出换行右对齐。实机 Playwright 15/15（位置 / chip 右侧 / 拆分 / tooltip+aria 精确值 / 390px 无溢出 / 无控制台错误）+ Oracle 复核 PASS（a11y 明细 / meta 内部换行 / total 去重 / 4 补测）。Web 468 → 479 tests（46 files）/ sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.18.0 → 0.18.1
- [x] **Web 三项增强（v0.18.0）**：用户直报（纯前端，无协议变更）—— ① composer 75% 居中浮动：`.aj-composer-card` `position:absolute; bottom:16px; width:75%; margin:0 auto; max-height:calc(100% - 32px)`（layout.css 拆分全宽规则，特异性 (0,2,0) 承接 75%），`--aj-composer-h`（RunPanel `useLayoutEffect` + `ResizeObserver` `borderBoxSize` 全盒高）驱动 `.aj-conv` `padding-bottom`/`scroll-padding-bottom`（= 高度 + 32px）→ 内容延伸至卡片底部（滚动穿透浮层）、滚底末条停在输入框顶上方 ~16px；窄视口 `min-width: min(340px, calc(100% - 16px))` + `.comp-tools` `flex-wrap: wrap` 两行兜底。② 权限选择器（逐次确认/自动允许，`agentj.composer.permissionMode` + 跨标签 `storage` 同步 + bypass accent 边框）：bypass 时 `useApproval` 对当前会话 `$/permissionRequest` 自动 `permission/respond allow`（不弹窗；失败 console.debug；内核 deny/路径越界仍强制拒绝）。③ 会话 token 总量：`RunPanel` 聚合 usage entries（历史回放 + 实时逐轮恰好一条）→ 模型触发器右侧 `Σ 5.2k`（`formatTokenTotal`：k/M 一位小数、`.0` 修剪、999.95k 起进位 M；title 输入/输出明细；无用量隐藏）。实机 Playwright 验证（75.0% 居中偏差 0 / 浮底 16px / 穿透 / 滚底对齐 / 权限跨标签双向同步 / 390px 无溢出）+ Oracle 复核 PASS（6 minor/nit 修复波 + 2 补测）。Web 446 → 468 tests（45 files）/ sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.17.2 → 0.18.0
- [x] **界面修复（v0.17.2）**：用户直报 2 项（纯前端，无协议变更）—— ① 显示设置字号：`displayPrefsStore` 字号字段由 `sm|md|lg|xl` 四档枚举升级为逐区像素数值（`FONT_SIZE_LIMITS`：对话 12–24/默认 15、左右栏 10–20/默认 13、代码 10–20/默认 12；clamp+取整；旧枚举按区迁移保持原渲染），`SettingsDisplayPanel` 每区一行「标签 + range 滑杆 + number 输入 + px」（draft 受控，blur/Enter 提交夹取），行高/密度选项横排，`WorkspaceShell` 直接注入 `${px}px`；② 可缩放弹窗：`ModalDialog` 新增 `.aj-dialog-frame`（尺寸/位置载体，首次拖拽冻结 fixed 矩形）+ 8 向手柄（`data-handle`，hover accentTint + 方向光标）+ 新 `dialogResize.ts` `resizeRect`（对边锚定、最小 320×200、视口 8px 边距夹取、可选 `minWidth/minHeight`），`SettingsDialog` 下限 560px；`.aj-settings` 宽度修饰移至 frame，`.aj-dialog-card` 基类规则不变（`ApprovalDialog` 不受影响）。实机 Playwright 验证（拖拽/锚定/夹取/联动）；复核修复波（Oracle 评审 6 项：视口上限夹取/锚定边夹取/窗口重夹取/指针守卫/原型键校验/手柄避让滚动条）。Web 417 → 446 tests（44 files）/ sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.17.1 → 0.17.2
- [x] **v0.17.0 验收修复（v0.17.1）**：实机 Playwright 验收发现 BUG-008 —— 窄视口（375–390px）左栏会话列表菜单超出右缘被 `.aj-sidebar` 裁剪、「过滤/排序」二级飞出层溢出视口不可点。修复：`clampMenuLeft`（菜单左缘夹取到 `[8, innerWidth - menuWidth - 8]`）+ `chooseFlyoutSide`（行右缘 + gap + 飞出宽 > 视口宽时向左展开，`.aj-menu__flyout--left`）+ `updateMenuPos` resize 重算；桌面 ≥1024 行为不变。Web 405 → 417 tests（43 files）/ sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.17.0 → 0.17.1
- [x] **Web 工作台四项修复（v0.16.5）**：右栏折叠/展开单段右→左平滑动画（外层 `.aj-shell__right` `width` 过渡 + 内层 `.aj-right` `align-self: flex-end` 右缘锚定，内容零回流；仅 ≥1024，SplitPane 拖拽期间兄弟选择器禁用过渡）；顶栏「...」菜单重构为恰好五项（项目/继续会话…/收藏会话/设置…/帮助），项目与收藏会话 hover 左向飞出二级菜单（桥接 + 120ms 关闭延迟 + Esc 分层关闭 + 键盘可达），排序/过滤/演示数据/连接设置迁入新 `SettingsDialog`（ModalDialog 加宽卡 + 左 tablist 连接/会话列表/演示数据 + 右面板），新 `sidebarPrefsStore` 持久化 `agentj.sidebarPrefs`；右栏「任务」「文件变更」成对面板：单开铺满至面板底部，双开 `role=separator` 拖拽/键盘调节（↑↓ ±2%/Shift ±8%、双击/Home 复位）+ `agentj.layout.rightSectionSplit` 比例持久化（0.15–0.85，缺省自动），打开状态经 `useSyncExternalStore` + `agentj:right-sections-updated` 跨实例同步；文件树 `fileIcons.ts` 分类图标（Folder/FolderOpen + 扩展名映射）+ 任意深度内联缩进 + token 化美化。验收（实机 Playwright）4 项全过。Web 263 → 308 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.4 → 0.16.5
- [x] **Web 工作台体验修复（v0.16.4）**：六项 + 验收 1 bug（无协议变更）—— 左上角仅 `agentj` + 状态灯，连接信息（ws/模型/模块状态）移入状态灯 hover/focus 面板（条件渲染；⋯ 菜单移除状态区块与分隔线）；右栏「任务」「文件变更」升级 VS Code 手风琴（折叠 = 36px 紧凑标题行自上而下堆叠、展开 = 内容向下延展、空数据保留标题行且占位符仅展开显示，展开状态 localStorage 记忆 `agentj.layout.rightSections` 默认全展开）；全局 token 化滚动条（webkit 10px 圆角 + Firefox thin/color，随 light/dark）；composer 模型/agent/熵变弹层二次点击收起（`OptionPopover.anchorRef` 忽略触发器 mousedown，修复「关了就重开」）；新建会话不再自动收藏（仅星标按钮触发）；验收修复 BUG-007（顶栏 `z-index: 5` < 侧栏 `10` 致 hover 面板被遮挡 → `z-index: 20` + CSS 契约测试）。Web 246 → 263 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.3 → 0.16.4
- [x] **Web 工作台修复（v0.16.3）**：左栏「项目」区块（项目分组 + 底栏「+ 添加项目」+ 添加对话框）整体迁入右上角「...」菜单二级菜单（位于「连接设置」下方）：子菜单列全部项目（当前项 ✓ / `menuitemradio` + `aria-checked`）、选择即切换当前项目（localStorage + 新 `agentj:current-project-updated` 事件同步 composer picker 与侧栏新建会话归属）、「＋ 添加项目」复用 ModalDialog；侧栏保留扁平最近会话列表。文件 tab 永久「加载中」修复：内核非法结构请求回显可确定 id（`-32600`；语法错误仍 `-32700` + null，对齐 `docs/specs/ipc-protocol.md`）+ SDK `RpcClient.request` 拒绝空/未定义 method；根因 = 长驻 Vite dev server 不监听 root 外 `packages/sdk` 源（陈旧 SDK 无 `METHODS.fileList` → 请求缺 `method`）+ `id:null` 响应无法匹配。Rust 560 → 564 / sdk 43 → 46 / web 239 → 246 tests（含 9801187 连接设置持久化加固 +2），PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.2 → 0.16.3
- [x] **远程访问修复（v0.16.2）**：内核 `serve`/`dashboard` 新增 `--host`（默认 127.0.0.1；`0.0.0.0` 局域网/远程直连；`dashboard` 补 `--token`；非回环无有效 token 告警；IPv6/实际端口打印修正）；Web `resolveWsUrl` 页面地址派生（无 `VITE_AGENTJ_WS_URL` 时取页面 host:4096，https → wss，Tauri 桌面保持明文回环）+ 连接设置持久化（`agentj.connection`：刷新自动恢复重连、弹窗回填）。Rust 560 / sdk 43 / web 239 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.1 → 0.16.2
- [x] **Web 工作台跟随修复（v0.16.1）**：左栏会话删除按钮移至收藏星标下方（右栏堆叠、行高 56px）；右上角模型 chip 移除（模型仍由 composer 与 usage meta 行展示）；会话单击不再自动收藏（仅星标按钮触发，回归测试锁定）。Web 230 → 231 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.0 → 0.16.1
- [x] **内核托管 Web UI + 文件树状态补强（v0.19.0）**：无 wire 变更（PROTOCOL_VERSION 保持 0.13.0）。① 同端口静态托管：`agentj-ipc` 新 `web` 模块（`WebConfig` + `build_router`）在 `ws_router` 上 `nest_service("/assets", ServeDir + Cache-Control: public, max-age=31536000, immutable)` + `fallback_service(ServeDir::fallback(ServeFile(index.html)) + no-cache)`（未知页面 200；`/ws`、`/login`、`/logout` 显式路由优先且不在门后）。② 混合鉴权登录门：回环 peer / 未配置 token 直出；非回环 + token 无有效 cookie → `302 /login?next=<同源相对路径>`（`safe_next` 白名单防开放重定向，错拼 `/assets/*` 仍 404）；内嵌登录页 POST 校验后颁发不透明 `agentj_session` cookie（`HttpOnly; SameSite=Lax; Path=/`，无 Max-Age，内存态）+ `/logout` 撤销。③ WS cookie 第三通道：有效会话 cookie + 同源 Origin 免 `?token=`/`initialize.token`（`serve_connection` token 参数遗留，服务端为 cookie 通道注入 initialize token）。④ CLI：`serve`/`dashboard --web-dist <PATH>` 四级发现 + 启动打印 `http://<host>:<port>/` + 缺失告警。⑤ 文件树目录聚合：`file/list` 目录 `git_status` 聚合后代最高优先级字母（`U>D>M>A`，前缀边界 `src/` 严格匹配）+ Web `FileTree` 目录状态字母、移除冗余圆点。测试：agentj-ipc `web` 单测（静态/缓存头/登录门/cookie/伪造 cookie/dist 缺失）+ CLI `e2e_http_static`（持有 stdout reader）。Rust 564 → 591 tests / sdk 46 / Web 479 → 480 tests（46 files）/ tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.18.1 → 0.19.0
- [x] **Web 加固 + TUI Files 视图（v0.20.0）**：四项（无 wire 变更，PROTOCOL_VERSION 保持 0.13.0，无第三方新增依赖）。① `serve_connection` 遗留 token 参数清理（`TcpIpcServer` 死 token 字段一并移除；`WsServerState.token`/`WsQuery.token`/`?token=`/`initialize.token` 语义不变）。② 登录限流：`agentj-ipc` 新 `login_limiter` 模块（按来源 IP 计数，第 5 次失败起锁定 30s、×2、封顶 900s；`429 + Retry-After`（秒，向上取整 ≥1）；锁定期不校验 token 且新尝试升级锁定；成功清零；idle_ttl 1800s 懒淘汰；max_tracked 1024 淘汰；回环同样计数），`/login` POST 接线（`auth_routes` 透传 peer）。③ 版本探测与 SRI：`apps/web/build/version-sri.ts` + `version-sri-plugin.ts`（`writeBundle` 生成 `dist/version.json`（product/protocol/build_time，单一真源 = sdk config）+ 入口 HTML 同源 script/stylesheet/modulepreload 注入 `sha384` `integrity`，本地引用缺失即构建失败；零新依赖）；内核 `WebConfig` 读取 `DistVersion` + `startup_warnings()`（缺失/不可解析/不一致三类启动警告，不阻断服务）+ 未鉴权 `GET /version`（200 JSON + `Cache-Control: no-store`，显式路由优先于 SPA fallback）+ `TcpIpcServer::web_config()`；CLI `serve` 启动打印 dist 版本警告 + `e2e_http_static` 覆盖；Web `ConnectionStatus` 版本行「版本 UI x · 内核 y」（不一致 `data-state="warn"` + `⚠` + warning 色）、`App` 消费 `initialize.server_info.version`、SDK barrel 导出 `VERSION`。④ TUI Files 视图：`files.ts` 纯函数（flatten/format/color/updateNode 等）+ `files.tsx` `FilesView` + `useFileTree`（懒加载 `file/list` depth=1、stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取、不持久化）+ `sidebar.tsx` Threads/Files 切换（`f`）+ `app.tsx` 键位（`j,k,↑,↓` 移动 / `Enter,l,→` 目录开合 / `h,←` 收起或跳父级 / `r` 刷新）+ mock kernel `file/list` + e2e；目录聚合状态字母色义 M→warning、A→success、U→textCaption、D→error。Oracle 全量复核 PASS（无 must-fix）+ 修复波（TUI stale-reply 守卫 + 父级跳转 clamp + Web warn 色 CSS 契约测试）。Rust 591 → **604 tests** / sdk 46 → **47** / web 480 → **491 tests（47 files）** / tui 71 → **85 tests（17 files）** / web build / check-version **0.20.0** / desktop-ui；PROTOCOL_VERSION 保持 0.13.0，工作区 0.19.0 → 0.20.0
- [x] **限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（v0.21.0）**：四项（无 wire 变更，PROTOCOL_VERSION 保持 0.13.0）。① 登录限流可观测性：`agentj-ipc` `login_limiter` 在状态变化点发 tracing 事件（锁定新触发 `warn`、成功登录 `info`、逐次失败/锁定期拒绝 `debug`），`AtomicU64` 聚合计数经 `GET /version` 加性 `login_limiter` 块暴露（`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，无 per-IP 明细），CLI 日志订阅器改 `EnvFilter`（`RUST_LOG` 可配，未设默认 `info`，行为与 v0.20.0 逐字一致）；限流策略语义（429/Retry-After/锁定升级/清零）零变化。② 资产完整性清单：构建期 `apps/web/build/version-sri*` 生成 `dist/asset-manifest.json`（`algorithm:"sha384"` + `assets:{"/path":"sha384-<base64>"}`，覆盖全部 Rollup bundle 产物含动态 chunk + SRI 注入后的 `index.html`；单遍哈希与 SRI `integrity` 逐字节一致；排除 `version.json`/自身/`public/` 透传；键须为 `/` 开头规范路径），内核 `WebConfig` 启动逐文件校验并并入 `startup_warnings()`（缺失/不可解析/不符/文件缺失，警告不阻断服务），摘要进 `GET /version` 加性 `integrity` 块（`manifest_present`/`checked`/`mismatched`/`missing`）。③ TUI Files 搜索：`s` 进入搜索（`file/list {path:null, depth:8}` 单请求扁平索引 ≤2000 + 客户端过滤），可打印字符过滤（含 `f`/`n`，吞键）、`Backspace` 删末字符、`Esc` 非空清空/空退出、`Enter` 跳转（祖先自动展开）并选中目标、`r` 重建索引、`truncated` 显示 `· partial`；展开状态经新 `filetree.json`（`$XDG_CONFIG_HOME/agentj/`）按本地项目键持久化（per-key ≤100、恢复 ≤50、失效路径静默跳过、陈旧键清理）。④ Web 焦点作用域：新 `useShortcutScope`（`activeElement` 在侧栏内、目标非可编辑、非 IME、无修饰键、无模态/菜单才生效）提供 `f`/`n`/`r`/`j`/`k`/`Esc`；文件树/会话列表 roving tabindex 键盘导航；`ModalDialog` 开关焦点恢复（原元素卸载不恢复）；**Tab 保持原生**、composer 输入零误触。门禁全绿（cargo fmt / 622 Rust tests / clippy 零警告 / drift 2 passed / sdk 47 / web 537 tests（50 files）/ tui 130 tests（19 files）/ web build / check-version 0.21.0 / desktop-ui）；PROTOCOL_VERSION 保持 0.13.0，工作区 0.20.1 → 0.21.0
- [x] **右边栏运行详情（v0.23.0）**：`run_id` 维度（UUID v4 per `session/run`，per-run `revision`/`seq`）+ `FileChangeSource=write|bash`（`total_bytes` 截断披露）+ `session/history {run_id}` 过滤（非法→-32602，其余 kind 全量）+ Web 右栏按 run 分组/徽标/截断 hint/历史抽屉 + FileTree 跳转 + TUI 兼容；`PROTOCOL_VERSION` 0.14.0 → 0.15.0，52 schemas
- [x] **补齐 v0.23.0 计划遗漏（v0.23.1）**：bugfix，无协议变更（PROTOCOL_VERSION 保持 0.15.0）。① Web `changes.ts` 状态拆分 `ChangesState {summary, history}`——summary 按 `(run_id, path)` upsert、history 按设计决策 #3/#15 append-only（按 seq 排序 + 按 `tool_call_id` 分组），`ChangeList` 历史抽屉可达；② `ChangeList.tsx` 消费 `history` prop、`RightPanel.tsx` run 分组经 `App.tsx` 传入 `groups`/`history`/`highlightedPath`/`onSelectRun` 真正接线（run 组头可点击、每组独立空态；无分组保留 children/手风琴回退）；③ `rightpanel.css` 补齐 `run-group*`/`history-drawer`/`hist-*`/`trunc-hint`/`src-badge*` token 化样式（此前裸节点无样式）；④ 补齐计划缺失测试 protocol T0（+5）/ SDK T2（+7）/ IPC T5（+4）/ TUI T9（+4）/ web（+21）；⑤ 删除 `agentj-core` 死代码 `bash_file_changes` 及单测（−1），`git_sample` 测试改断言生产路径 `source == Some(Bash)`。⑥ 用户直报修复：右栏空态文案 `待协议支持` → `暂无任务`/`暂无变更`（TodoPanel/ChangeList），`handleSelectRun` 改用 `todoState.focusRun` 保留非枚举 `byRun`（修复点击 run 组头后分组丢失回退旧空态）；web +2 回归测。Rust 652 → **660 tests** / sdk 47 → **54** / web 557 → **580 tests（51 files）** / tui 142 → **146 tests（19 files）**；工作区 0.23.0 → 0.23.1
- [x] **中栏 Composer 滚动自动收缩（v0.24.0）**：纯前端，无协议变更（PROTOCOL_VERSION 保持 0.15.0）。① 新增 `composerCollapse.ts` 纯状态机（48/40 迟滞带 + 迁移守卫 + 260ms 动画锁，7 测）；② `ConversationFeed` 用户意图双判（`wheel`/`touchmove` 手势 + 非程序化 `scroll` 方向）与程序化 `scrollIntoView` 自标记排除（5 测）；③ `RunPanel` 接线：`data-collapsed` 属性 + 发送按钮双职责（收缩态变「展开输入框」`ChevronsUp`，X/Y 零位移 + `aria-label`/`aria-expanded`/`aria-controls` 同步）+ 待答提问守卫与强制展开 + 收缩期冻结 `--aj-composer-h` + 收缩时关闭 composer 弹层 + 点击路径 `focus(textarea, preventScroll)`（10 测）+ `ComposerOptions` `collapsed` prop；④ `composer.css` 分段收拢契约（header/输入区/选择器收拢、按钮行保持）+ reduced-motion 复用既有双层机制（4 测）。⑤ 评审修复波（代码评审 + Oracle 对抗复核，headless-Chromium 探针）：延迟 visibility 切换修复真实浏览器「点击展开聚焦 textarea」、展开期 `--aj-composer-h` settle 保持（防 scrollTop 钳制/底部漂移）、260ms 锁过期回底重判、程序化标记仅 `delta >= 0` 消耗（不吞真实上滚）、header 展开上限 120px；3 回归测 + freeze 测试强化。门禁全绿（cargo fmt / **660 Rust tests** / clippy 零警告 / drift 2 passed / sdk 54 / web 580 → **609 tests（52 files）** / tui 146（19 files）/ web build / check-version **0.24.0** / desktop-ui）；工作区 0.23.1 → 0.24.0
- [x] **限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口（v0.22.0）**：四项（三处加性 wire，PROTOCOL_VERSION 0.13.0 → **0.14.0**）。① 限流指标导出：`login_limiter` 新增只读 `snapshot()` 投影（`ip`/`failures`/`locked`/`retry_after`/`last_seen`，排序 `locked desc → failures desc → ip asc`）；`agentj-ipc` `web` 新增 `auth_required()` + 显式路由 `GET /metrics`（Prometheus 文本 `text/plain; version=0.0.4` + `Cache-Control: no-store`；聚合 counters/gauges + per-IP series Top-50 截断 + `agentj_login_ip_series_truncated` 指示 + `integrity` 摘要；回环/未配 token/有效 cookie 直出，非回环 + token 无 cookie → `401` 不重定向）；`/version` 既有契约零变化。② 资产清单扩展 + 运行期抽检：构建清单覆盖 dist 实目录全部部署文件（含 `public/` 透传，bundle 键优先，排除 `version.json`/自身/隐藏文件）；`TcpIpcServer` 启动后 60s / 8 文件游标轮转后台抽检（`SpotCheckConfig` 可注入），不符/缺失 `warn!` 按路径去重；`/version` `integrity` 加性嵌套 `runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`。③ `file/list` 服务端搜索：加性参数 `query`（`path` 子树、忽略 `depth`、大小写不敏感子串、访问预算 `SEARCH_VISIT_LIMIT=20000`）与结果 `total`（预算受限为下界），rank 名称前缀 > 名称子串 > 路径子串、返回 Top-100；TUI 搜索改防抖 150ms 服务端查询（旧内核无 `total` 自动降级 depth-8 本地过滤），展开态键改为会话有效 `project_root`（`session/new`/`session/status` 加性 `project_root` 捕获，未知回退 `__default__`）。④ Web 键位收口：共享 `useRovingFocus` 扩展 `wrap`/Home-End，TopbarMenu/SessionListMenu/SidebarTabs/SettingsDialog 四处迁移（ComposerOptions listbox 保留独立模式）+ 侧栏底部快捷键提示行（三态文案组件；Web 当前两态可达，搜索态待 Web 搜索特性；单一常量源、`aria-hidden`）。PROTOCOL_VERSION 0.15.0，工作区 0.21.1 → 0.22.0

## Project Structure

```
crates/                    # 17 个 workspace crate（统一目录）
  agentj-protocol/         # IPC 协议契约（wire 类型 + schema 生成）
  agentj-core/             # 内核调度 + 事件总线（含重入防护）+ TurnRunner + TurnArtifact（diff）
  agentj-config/           # TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）
  agentj-session/          # 会话生命周期（FileBackedStore + 过期清理 + 会话级 project_root）
  agentj-question/         # 提问通道（QuestionModule + AskTool，镜像 permission 归属）
  agentj-model/            # 模型提供方（DeepSeek/opencode-go/Ollama/Mock）
  agentj-tools/            # 内置工具（bash/glob/grep/read/write + todo）
  agentj-permission/       # 权限系统
  agentj-context/          # 上下文管线
  agentj-stats/            # 用量统计 UsageCollector
  agentj-license/          # Ed25519 许可证校验
  agentj-mcp/              # MCP 客户端 + McpConnectionManager
  agentj-skills/           # skills 加载器
  agentj-plugin/           # 插件系统 + libloading
  agentj-ipc/              # JSON-RPC IPC（stdio/TCP/WS + 连接分类器 + file/list（含目录聚合状态 + v0.22.0 query/total）+ 提问接线
                           #   + web 模块：静态托管 ServeDir/SPA fallback + 登录门 + WS cookie 通道
                           #   + v0.20.0 login_limiter（按 IP 限流 429/Retry-After）+ dist version.json 读取/启动警告 + GET /version
                           #   + v0.22.0 GET /metrics（鉴权门控 Prometheus 文本）+ asset-manifest public/ 覆盖 + 60s/8 文件运行期抽检）
  agentj-cli/              # agentj 二进制（run/serve/dashboard）
  agentj-plugin-fixture/   # 插件 cdylib 测试夹具（dev-only）
agentj-ui/                 # 前端 monorepo（npm workspaces，frontend stays in-repo）
  package.json             # 根 workspaces: ["apps/*", "packages/*"]
  packages/
    protocol/              # 协议类型生成（schemas → types.d.ts，复用 drift 门禁）
    sdk/                   # 共享 SDK（client/router/transports/config/methods）
      src/
        client.ts          # RpcClient
        router.ts          # ResponseRouter + RpcTransport
        config.ts          # VERSION="0.24.0" + PROTOCOL_VERSION="0.15.0" + resolveWsUrl（index barrel 导出 VERSION）
        methods.ts         # METHODS 常量（由 methods.json 生成）
        transports/
          websocket.ts     # WebSocketTransport（浏览器）
          tcp.ts           # TcpTransport（Node 直连预留）
    tokens/                # 共享 tokens（palette.ts 24 keys + LAYOUT 常量 + tokens.css/json/md 生成）
  apps/
    web/                   # Web 三栏工作台（真实协议驱动：WorkspaceShell/RightPanel/TodoPanel/ChangeList/FileTree/AskQuestionCard/ConnectionStatus/SettingsDialog；v0.17.0 代码卡片 + 显示设置 + 源码/db 图标 + 会话列表菜单 + 项目编辑 + 设置项目面板；v0.18.0 composer 75% 居中浮动 + 权限选择器；v0.18.1 token 读出移至 header 模型 chip 右侧 + 输入/输出 拆分（`tokenUsage.ts`）；v0.19.0 `FileTree` 目录聚合状态字母（目录行渲染后代最高优先级 git 字母）+ 移除冗余圆点；v0.20.0 build/version-sri*（dist version.json + sha384 integrity 注入）+ ConnectionStatus 版本行（UI/内核不一致 warn）；v0.22.0 共享 `useRovingFocus`（`wrap`/Home-End）收口 TopbarMenu/SessionListMenu/SidebarTabs/SettingsDialog + 侧栏底部快捷键提示行（三态文案；Web 两态可达）；消费 @agentj/sdk + @agentj/tokens + lucide-react；components/*.css 模块化；v0.24.0 新增 composerCollapse 纯状态机 + 滚动收缩态 composer：data-collapsed / 双职责按钮 / --aj-composer-h 冻结）
    tui/                   # TUI（Bun + @opentui/react，全功能 + runview 只读 TODO 区块；v0.20.0 Files 视图：Threads/Files 切换（`f`）+ 懒加载 file/list + 键位 j/k/Enter/h/r；v0.22.0 服务端搜索（防抖 150ms query/total + 旧内核降级）+ 展开态键 = 会话有效 project_root）
    desktop/               # Tauri 壳（套 web/dist，输出安装包）
models/                    # models/opencode-go.json
schemas/                   # 52 JSON Schema（机器事实来源）
protocol/                  # methods.json 方法注册表
docs/superpowers/          # 设计文档 + 实现计划（中文）
```

> 前端不拆独立仓：`agentj-ui` 保留在 `crates/` 同级 monorepo 内，
> 与 Rust workspace 共享版本 `0.24.0`（`VERSION` + 单一 `PROTOCOL_VERSION 0.15.0` 来源 `sdk/config.ts`）。

## Usage

```bash
# 后端常驻内核（Web/TUI/Desktop 均连此端点）
agentj serve --port 4096
agentj serve --port 4096 --host 0.0.0.0 --token SECRET  # 局域网/远程直连（建议配 --token）
agentj serve --port 4096 --token SECRET   # 鉴权：?token= 或 initialize.token
agentj dashboard --port 4096              # 同款协议入口（无 token 最简）
agentj dashboard --port 4096 --token SECRET  # dashboard 远程访问鉴权（v0.16.2）

# 内核托管 Web UI（同端口静态托管内置前端，v0.19.0）
agentj serve --port 4096 --web-dist path/to/agentj-ui/apps/web/dist   # 显式指定 dist
# 四级发现：--web-dist → AGENTJ_WEB_DIST → cwd agentj-ui/apps/web/dist → exe 相对（缺失告警）
# 本地打开即用：浏览器访问 http://127.0.0.1:4096/（回环 peer 免登录）
# 远程访问闭环：agentj serve --host 0.0.0.0 --token SECRET
#   → 浏览器打开 http://<host>:4096/ → 内嵌登录页输入 token → cookie 会话生效，UI + WS 同源可用
#   （非回环未配 token 直出并告警；dist 缺失则静态 UI 停用、仅 /ws 可用）
# 版本探测 / 限流可观测 / 资产完整性 / 指标导出（v0.20.0 → v0.22.0）
curl http://127.0.0.1:4096/version   # 未鉴权 JSON：product/protocol + dist 版本事实（Cache-Control: no-store）
#   v0.21.0 加性块：login_limiter {attempts, failures, lockouts, blocked, locked_now, tracked_ips}（无 per-IP 明细）
#                   integrity     {manifest_present, checked, mismatched, missing}
#   v0.22.0 加性：integrity.runtime {rounds, checked, mismatched, missing, last_run_age_secs?}（运行期抽检，未运行省略）
# 指标导出（Prometheus 文本，v0.22.0）：回环 / 未配 token / 有效 cookie 直出 200；远程无 cookie → 401（不重定向）
curl http://127.0.0.1:4096/metrics   # Content-Type: text/plain; version=0.0.4；Cache-Control: no-store
#   含聚合计数 + per-IP 序列（Top-50 截断 + agentj_login_ip_series_truncated）+ agentj_web_spot_* 抽检计数 + 清单摘要
RUST_LOG=debug agentj serve --port 4096   # 限流日志下探 debug（默认 info；锁定触发 warn、成功登录 info）
# 构建期 dist/asset-manifest.json：sha384 覆盖全部 bundle 产物（含动态 chunk）+ 重写后 index.html
#   + dist 实目录其余部署文件（含 public/ 透传，如 /favicon.svg；bundle 键优先）
# 内核启动逐文件校验（缺失/不符仅 stderr 警告不阻断服务）+ 运行期每 60s 抽检 8 个文件（游标轮转，不符 warn 去重）
# 非回环登录失败：第 5 次起 429 + Retry-After（30s 起 ×2、封顶 900s；成功即清零）

# 单轮运行 / 附着运行
agentj run "explain this codebase"
agentj run --attach tcp://127.0.0.1:4096 "hi"

# Web 前端（开发）
npm --prefix agentj-ui install
npm --prefix agentj-ui run dev --workspace=@agentj/web        # Vite dev，VITE_AGENTJ_WS_URL 可配
npm --prefix agentj-ui run build --workspace=@agentj/web      # tsc -b && vite build
npm --prefix agentj-ui test --workspace=@agentj/web

# TUI 前端（Bun + OpenTUI）
bun --cwd agentj-ui/apps/tui install
bun --cwd agentj-ui/apps/tui run dev                                # 直连 dashboard --port / serve --port
#   Files 视图（f 切换）内按 s 搜索：防抖 150ms 服务端查询 file/list {path:null, depth:8, query}
#   （服务端 rank + total；旧内核回包无 total → 自动降级 depth-8 索引本地过滤）
#   Enter 跳转（祖先展开）、Esc 清空/退出、r 重建；展开态按会话有效 project_root 共享（未知回退 __default__）
bun --cwd agentj-ui/apps/tui test                                   # @opentui/react testRender + mock WS e2e

# Desktop 安装包（Tauri，需 Rust 工具链）
npm --prefix agentj-ui run build --workspace=@agentj/web      # 先产出 frontendDist
npm --prefix agentj-ui run build --workspace=@agentj/desktop  # tauri build → target/release/bundle/
./agentj-ui/apps/desktop/check-artifacts.sh linux   # 校验 deb + AppImage

# SDK / Protocol
npm --prefix agentj-ui run generate --workspace=@agentj/protocol  # schemas → types.d.ts
npm --prefix agentj-ui run build --workspace=@agentj/protocol     # tsc --noEmit
npm --prefix agentj-ui run generate --workspace=@agentj/sdk       # methods.json → METHODS
npm --prefix agentj-ui test --workspace=@agentj/sdk
```

配置示例 `config.toml`：

```toml
[model]
provider = "deepseek"
default_model = "deepseek-chat"

[[model.providers]]
name = "deepseek-chat"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
kind = "openai-compat"   # openai-compat | responses | anthropic | opencode-go

[permission]
[permission.tools.bash]
mode = "ask"

[[permission.tools.bash.command_rules]]
prefix = "rm -rf"
effect = "deny"

[session]
dir = "/data/agentj/sessions"

[license]
key = "base64(payload).base64(sig)"

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
env = { FOO = "bar" }
```

## Protocol

- 规范：`docs/specs/ipc-protocol.md`
- Schema：`schemas/*.json`（52 个）
- 方法注册表：`protocol/methods.json`（21 方法，protocol_version 0.15.0）
- 方法面：`initialize` / `initialized` / `shutdown` / `exit` /
  `session/new` / `session/run` / `session/status` / `session/archive` /
  `session/delete` / `session/subscribe` / `session/unsubscribe` /
  `session/list` / `session/history` / `kernel/status` /
  `$/cancelRequest` / `$/progress` / `$/askQuestion` / `$/permissionRequest` /
  `permission/respond` / `file/list` / `question/respond`
- 传输：stdio（单客户端） + TCP（NDJSON 多客户端） + WebSocket `/ws`
  （同端口分类器，`?token=` + `initialize.token` + 登录 cookie 三通道鉴权；
  v0.20.0 非回环登录失败限流：第 5 次起 `429 + Retry-After`）
- HTTP 面（内核托管）：`/`（Web UI，no-cache）+ `/assets`（immutable）+
  `/login`/`/logout`（会话 cookie）+ 未鉴权 `GET /version`（JSON + no-store；
  v0.20.0 版本事实；v0.21.0 加性 `login_limiter` 聚合计数 + `integrity` 清单摘要；
  v0.22.0 加性 `integrity.runtime` 抽检状态）+ 鉴权门控 `GET /metrics`
  （Prometheus 文本 `text/plain; version=0.0.4` + no-store；v0.22.0；
  回环/无 token/有效 cookie 直出，远程无 cookie → `401` 不重定向）；
  内核启动校验 `asset-manifest.json`（v0.22.0 覆盖 `public/` 透传），
  运行期 60s/8 文件抽检，警告不阻断）
- 协议扩展（v0.23.0，七处加性 wire，`PROTOCOL_VERSION` 0.15.0）：`FileChange {run_id/seq/source/total_bytes}`
  （`FileChangeSource=write|bash` + `Unknown`；`total_bytes` 仅 `diff_truncated:true` 时）+
  `ProgressValue::Todo/FileChange {run_id}`（路由提示）+ `TrajectoryValue::Todo/FileChange {run_id}` +
  `SessionHistoryParams {run_id?}`（非法 → `-32602` + `data{reason:"invalid_run_id"}`）+
  `SessionRunResult {run_id?}`；v0.22.0：`file/list` 参数 `query` + 结果 `total`（服务端搜索，旧内核自动降级）+
  `SessionNewResult` / `SessionStatusResult.project_root`（会话有效根）

## Development

```bash
cargo test --workspace            # 全量 Rust 测试（660 passed；含 e2e_cli/e2e_permission/e2e_serve/e2e_ws/e2e_http_static）
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p agentj-model       # 模型流式基准
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cargo test -p agentj-protocol -- drift          # 漂移门禁（52 schemas 无漂移）
./scripts/check-version.sh        # 版本一致性：Cargo.toml + sdk/config.ts + desktop 均为 0.24.0（PROTOCOL_VERSION 0.15.0）
# 运行期可观测：RUST_LOG=debug 查看限流逐次明细（默认 info）；curl :4096/version 读取 login_limiter/integrity 摘要
#   curl :4096/metrics 拉取 Prometheus 文本（聚合 + per-IP Top-50 + 抽检 + 清单摘要）
# 完整性产物：npm run build --workspace=@agentj/web 生成 dist/asset-manifest.json（sha384 全产物 + index.html + public/ 透传）

# 前端 monorepo（npm workspaces，根为 agentj-ui/）
npm --prefix agentj-ui install                           # 安装（agentj-ui workspaces）
npm --prefix agentj-ui run generate --workspace=@agentj/protocol   # 生成 protocol types
npm --prefix agentj-ui test --workspace=@agentj/sdk                # SDK 单测（54 tests）
npm --prefix agentj-ui test --workspace=@agentj/web                # Web 单测（609 tests / 52 files）
npm --prefix agentj-ui run build --workspace=@agentj/web
bun --cwd agentj-ui/apps/tui test           # TUI 单测（146 tests / 19 files，含 mock WS e2e）
./agentj-ui/apps/desktop/check-artifacts.sh linux  # Desktop 产物校验
node agentj-ui/apps/desktop/check-desktop-ui.mjs    # Desktop 前端产物/配置一致性校验
```
