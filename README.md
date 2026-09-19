# AgentJ

AI 编码代理工具（Rust）——Linux 内核式「内核调度 + 可插拔模块」架构。
内核通过统一的 JSON-RPC 2.0 协议（stdio / TCP / WebSocket）对外暴露能力；
CLI、TUI、Web、Desktop 均通过 `agentj-ui` 协议消费者接入，任意语言可开发前端。

> 当前版本：**v0.17.1**——v0.17.0 验收修复（BUG-008：窄视口 375–390px 左栏会话列表菜单横向夹取 + 「过滤/排序」飞出层按空间方向自适应 `.aj-menu__flyout--left`，resize 重算，桌面 ≥1024 行为不变）+ Web 工作台六项增强（纯前端，无协议变更）：代码块卡片化（`CodeBlockCard` + `copyText` clipboard→execCommand 降级链 + `rehype-highlight` 高亮）/ 显示设置（`displayPrefsStore` + 设置「显示」tab + `displayPrefs.css` 覆盖层 + `--aj-font-*`/`--aj-lh-conv`/`data-density`/`data-reduced-motion`）/ 文件树源码与 db 图标 + 文件名特例 / 左栏 `SessionListMenu`（清理 + 过滤/排序向右飞出二级菜单，设置「会话列表」移除）/ 项目编辑（`projectStore` 扩展字段 + `ProjectEditDialog` + composer 行内编辑 + `project_root` 随 `session/new` 打通 + 项目默认模型/Agent 应用）/ 设置「项目」tab（全局默认 + 列表 + 守卫删除 + `ModalDialog` 嵌套栈）；v0.16.5——Web 工作台四项修复：右栏折叠/展开单段右→左平滑动画（外层 `width` 过渡 + 内层 `align-self: flex-end` 右缘锚定、内容零回流；仅 ≥1024，拖拽期间兄弟选择器禁用过渡）/ 顶栏「...」五项菜单（项目、继续会话…、收藏会话、设置…、帮助；项目与收藏会话 hover 左向飞出二级菜单 + Esc 分层 + 键盘可达；排序/过滤/演示数据/连接设置迁入「设置」弹窗，左 tablist 分类 + `sidebarPrefsStore` 持久化）/ 右栏任务/文件变更成对面板（单开铺满至底部、双开 `role=separator` 拖拽/键盘调节 + `agentj.layout.rightSectionSplit` 比例持久化、双击/Home 复位）/ 文件树分类图标（Folder/FolderOpen + 扩展名 lucide 图标 + 任意深度缩进）；v0.16.4——Web 工作台体验修复（六项 + 验收 1 bug）：状态灯 hover 连接面板（左上角仅 `agentj` + 状态灯；⋯ 菜单移除状态区块）/ 右栏 VS Code 手风琴（折叠 = 紧凑标题行堆叠、展开 = 向下延展、空数据保留标题行、localStorage 记忆默认全展开）/ 全局 token 化滚动条 / composer 选项弹层二次点击收起（`OptionPopover.anchorRef`）/ 新建会话不再自动收藏 / 验收修复 BUG-007（顶栏 `z-index: 20` 高于侧栏）；v0.16.3——Web 工作台修复：左栏「项目」迁入右上角「...」菜单二级菜单（「连接设置」下方，选择项目 + 「＋ 添加项目」+ `agentj:current-project-updated` 同步）与文件 tab 永久「加载中」修复（内核非法请求回显可确定 id `-32600` + SDK 拒绝空/未定义 method）；v0.16.2——远程访问修复：内核 `--host` 绑定（默认仍 127.0.0.1，`0.0.0.0` 可局域网/远程直连）+ `dashboard --token` + Web 页面地址派生与连接设置持久化（修复远程访问误连 `ws://127.0.0.1:4096/ws`、刷新丢失指定 IP）；v0.16.1 三项修复：会话删除移至星标下方 + 顶栏模型 chip 移除 + 单击不再自动收藏；v0.16.0 六项修复：协议版本 semver 兼容 + composer 弹层锚定 + hover/主题对比度 + 会话管理 + 全宽拉伸；协议扩展：三栏工作台真实数据源
> （`file/list` 目录懒加载 + git 状态点；`$/progress` 新增 `kind:"todo"`
> 全量快照 + 会话内单调 `revision` 与 `kind:"file_change"` unified diff；
> `$/askQuestion` + `question/respond` owner 定向提问；会话级 `project_root`；
> 新 crate `agentj-question`，21 方法 / 51 schemas / PROTOCOL_VERSION 0.13.0）
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
- [x] **JSON Schema**：51 Schema（schemars 生成，提交件 + 漂移门禁）
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
- [x] **v0.17.0 验收修复（v0.17.1）**：实机 Playwright 验收发现 BUG-008 —— 窄视口（375–390px）左栏会话列表菜单超出右缘被 `.aj-sidebar` 裁剪、「过滤/排序」二级飞出层溢出视口不可点。修复：`clampMenuLeft`（菜单左缘夹取到 `[8, innerWidth - menuWidth - 8]`）+ `chooseFlyoutSide`（行右缘 + gap + 飞出宽 > 视口宽时向左展开，`.aj-menu__flyout--left`）+ `updateMenuPos` resize 重算；桌面 ≥1024 行为不变。Web 405 → 417 tests（43 files）/ sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0，工作区 0.17.0 → 0.17.1
- [x] **Web 工作台四项修复（v0.16.5）**：右栏折叠/展开单段右→左平滑动画（外层 `.aj-shell__right` `width` 过渡 + 内层 `.aj-right` `align-self: flex-end` 右缘锚定，内容零回流；仅 ≥1024，SplitPane 拖拽期间兄弟选择器禁用过渡）；顶栏「...」菜单重构为恰好五项（项目/继续会话…/收藏会话/设置…/帮助），项目与收藏会话 hover 左向飞出二级菜单（桥接 + 120ms 关闭延迟 + Esc 分层关闭 + 键盘可达），排序/过滤/演示数据/连接设置迁入新 `SettingsDialog`（ModalDialog 加宽卡 + 左 tablist 连接/会话列表/演示数据 + 右面板），新 `sidebarPrefsStore` 持久化 `agentj.sidebarPrefs`；右栏「任务」「文件变更」成对面板：单开铺满至面板底部，双开 `role=separator` 拖拽/键盘调节（↑↓ ±2%/Shift ±8%、双击/Home 复位）+ `agentj.layout.rightSectionSplit` 比例持久化（0.15–0.85，缺省自动），打开状态经 `useSyncExternalStore` + `agentj:right-sections-updated` 跨实例同步；文件树 `fileIcons.ts` 分类图标（Folder/FolderOpen + 扩展名映射）+ 任意深度内联缩进 + token 化美化。验收（实机 Playwright）4 项全过。Web 263 → 308 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.4 → 0.16.5
- [x] **Web 工作台体验修复（v0.16.4）**：六项 + 验收 1 bug（无协议变更）—— 左上角仅 `agentj` + 状态灯，连接信息（ws/模型/模块状态）移入状态灯 hover/focus 面板（条件渲染；⋯ 菜单移除状态区块与分隔线）；右栏「任务」「文件变更」升级 VS Code 手风琴（折叠 = 36px 紧凑标题行自上而下堆叠、展开 = 内容向下延展、空数据保留标题行且占位符仅展开显示，展开状态 localStorage 记忆 `agentj.layout.rightSections` 默认全展开）；全局 token 化滚动条（webkit 10px 圆角 + Firefox thin/color，随 light/dark）；composer 模型/agent/熵变弹层二次点击收起（`OptionPopover.anchorRef` 忽略触发器 mousedown，修复「关了就重开」）；新建会话不再自动收藏（仅星标按钮触发）；验收修复 BUG-007（顶栏 `z-index: 5` < 侧栏 `10` 致 hover 面板被遮挡 → `z-index: 20` + CSS 契约测试）。Web 246 → 263 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.3 → 0.16.4
- [x] **Web 工作台修复（v0.16.3）**：左栏「项目」区块（项目分组 + 底栏「+ 添加项目」+ 添加对话框）整体迁入右上角「...」菜单二级菜单（位于「连接设置」下方）：子菜单列全部项目（当前项 ✓ / `menuitemradio` + `aria-checked`）、选择即切换当前项目（localStorage + 新 `agentj:current-project-updated` 事件同步 composer picker 与侧栏新建会话归属）、「＋ 添加项目」复用 ModalDialog；侧栏保留扁平最近会话列表。文件 tab 永久「加载中」修复：内核非法结构请求回显可确定 id（`-32600`；语法错误仍 `-32700` + null，对齐 `docs/specs/ipc-protocol.md`）+ SDK `RpcClient.request` 拒绝空/未定义 method；根因 = 长驻 Vite dev server 不监听 root 外 `packages/sdk` 源（陈旧 SDK 无 `METHODS.fileList` → 请求缺 `method`）+ `id:null` 响应无法匹配。Rust 560 → 564 / sdk 43 → 46 / web 239 → 246 tests（含 9801187 连接设置持久化加固 +2），PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.2 → 0.16.3
- [x] **远程访问修复（v0.16.2）**：内核 `serve`/`dashboard` 新增 `--host`（默认 127.0.0.1；`0.0.0.0` 局域网/远程直连；`dashboard` 补 `--token`；非回环无有效 token 告警；IPv6/实际端口打印修正）；Web `resolveWsUrl` 页面地址派生（无 `VITE_AGENTJ_WS_URL` 时取页面 host:4096，https → wss，Tauri 桌面保持明文回环）+ 连接设置持久化（`agentj.connection`：刷新自动恢复重连、弹窗回填）。Rust 560 / sdk 43 / web 239 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.1 → 0.16.2
- [x] **Web 工作台跟随修复（v0.16.1）**：左栏会话删除按钮移至收藏星标下方（右栏堆叠、行高 56px）；右上角模型 chip 移除（模型仍由 composer 与 usage meta 行展示）；会话单击不再自动收藏（仅星标按钮触发，回归测试锁定）。Web 230 → 231 tests，PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.0 → 0.16.1

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
  agentj-ipc/              # JSON-RPC IPC（stdio/TCP/WS + 连接分类器 + file/list + 提问接线）
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
        config.ts          # VERSION="0.17.1" + PROTOCOL_VERSION="0.13.0" + resolveWsUrl
        methods.ts         # METHODS 常量（由 methods.json 生成）
        transports/
          websocket.ts     # WebSocketTransport（浏览器）
          tcp.ts           # TcpTransport（Node 直连预留）
    tokens/                # 共享 tokens（palette.ts 24 keys + LAYOUT 常量 + tokens.css/json/md 生成）
  apps/
    web/                   # Web 三栏工作台（真实协议驱动：WorkspaceShell/RightPanel/TodoPanel/ChangeList/FileTree/AskQuestionCard/ConnectionStatus/SettingsDialog；v0.17.0 代码卡片 + 显示设置 + 源码/db 图标 + 会话列表菜单 + 项目编辑 + 设置项目面板；消费 @agentj/sdk + @agentj/tokens + lucide-react；components/*.css 模块化）
    tui/                   # TUI（Bun + @opentui/react，全功能 + runview 只读 TODO 区块）
    desktop/               # Tauri 壳（套 web/dist，输出安装包）
models/                    # models/opencode-go.json
schemas/                   # 51 JSON Schema（机器事实来源）
protocol/                  # methods.json 方法注册表
docs/superpowers/          # 设计文档 + 实现计划（中文）
```

> 前端不拆独立仓：`agentj-ui` 保留在 `crates/` 同级 monorepo 内，
> 与 Rust workspace 共享版本 `0.17.1`（`VERSION` + 单一 `PROTOCOL_VERSION 0.13.0` 来源 `sdk/config.ts`）。

## Usage

```bash
# 后端常驻内核（Web/TUI/Desktop 均连此端点）
agentj serve --port 4096
agentj serve --port 4096 --host 0.0.0.0 --token SECRET  # 局域网/远程直连（建议配 --token）
agentj serve --port 4096 --token SECRET   # 鉴权：?token= 或 initialize.token
agentj dashboard --port 4096              # 同款协议入口（无 token 最简）
agentj dashboard --port 4096 --token SECRET  # dashboard 远程访问鉴权（v0.16.2）

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
- Schema：`schemas/*.json`（51 个）
- 方法注册表：`protocol/methods.json`（21 方法，protocol_version 0.13.0）
- 方法面：`initialize` / `initialized` / `shutdown` / `exit` /
  `session/new` / `session/run` / `session/status` / `session/archive` /
  `session/delete` / `session/subscribe` / `session/unsubscribe` /
  `session/list` / `session/history` / `kernel/status` /
  `$/cancelRequest` / `$/progress` / `$/askQuestion` / `$/permissionRequest` /
  `permission/respond` / `file/list` / `question/respond`
- 传输：stdio（单客户端） + TCP（NDJSON 多客户端） + WebSocket `/ws`
  （同端口分类器，`?token=` + `initialize.token` 双通道鉴权）

## Development

```bash
cargo test --workspace            # 全量 Rust 测试（含 e2e_cli/e2e_permission/e2e_serve/e2e_ws）
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p agentj-model       # 模型流式基准
cargo run -p agentj-protocol --bin gen_schemas   # 重新生成 schemas/*.json
cargo test -p agentj-protocol -- drift          # 漂移门禁（51 schemas 无漂移）
./scripts/check-version.sh        # 版本一致性：Cargo.toml + sdk/config.ts + desktop 均为 0.17.1（PROTOCOL_VERSION 0.13.0）

# 前端 monorepo（npm workspaces，根为 agentj-ui/）
npm --prefix agentj-ui install                           # 安装（agentj-ui workspaces）
npm --prefix agentj-ui run generate --workspace=@agentj/protocol   # 生成 protocol types
npm --prefix agentj-ui test --workspace=@agentj/sdk                # SDK 单测
npm --prefix agentj-ui test --workspace=@agentj/web                # Web 单测 + 构建
npm --prefix agentj-ui run build --workspace=@agentj/web
bun --cwd agentj-ui/apps/tui test           # TUI 单测（含 mock WS e2e）
./agentj-ui/apps/desktop/check-artifacts.sh linux  # Desktop 产物校验
node agentj-ui/apps/desktop/check-desktop-ui.mjs    # Desktop 前端产物/配置一致性校验
```
