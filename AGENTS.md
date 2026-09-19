# AGENTS.md

> Updated 2026-09-19 — v0.15.1 协议扩展（三栏工作台真实数据源）+ v0.15.2 session/status dispatch 修复 + v0.16.0 Web 工作台体验修复 + v0.16.1 跟随修复 + v0.16.2 远程访问修复 + v0.16.3 Web 工作台修复（项目菜单 + 文件树）+ v0.16.4 Web 工作台体验修复（状态灯 hover 面板 + 右栏手风琴 + 滚动条）+ v0.16.5 Web 工作台四项修复（右栏右→左展开动画 + 任务/文件变更高度可调 + 顶栏菜单重构/设置弹窗 + 文件树图标）+ v0.17.0 Web 工作台六项增强（代码卡片 + 显示设置 + 文件树图标 + 会话列表菜单 + 项目编辑 + 设置项目面板）+ v0.17.1 验收修复（BUG-008 窄视口菜单/飞出层夹取）+ v0.17.2 界面修复（显示设置字号 px 滑杆/数字输入 + 弹窗边框/四角缩放）
> `file/list` 沙箱化目录懒加载 + git 状态点；`$/progress` 新增 `kind:"todo"`
> （全量快照 + 会话内单调 `revision`）与 `kind:"file_change"`（write 自研行级
> diff / bash git 前后采样，unified diff ≤64 KiB）；`$/askQuestion` +
> `question/respond` owner 定向提问往返（幂等 + 超时/取消/断开按 skip 结算）；
> 会话级 `project_root`（路径基准五处同源）+ 新错误码 `-32807 PathNotAllowed`
> + `error.data`；`session/history` 回放重建 TODO/变更/Q&A；新 crate
> `agentj-question`（17 crates）；21 方法 / 51 schemas / PROTOCOL_VERSION 0.13.0；
> Web 三栏工作台 TODO/文件树/变更/提问卡切换真实协议驱动（`deriveChanges`
> 半推导退役，demo 模式保留为可切换预览通道）；TUI runview 只读 TODO 区块。
> T0-T42 全部落地，T42 全量门禁通过：cargo fmt / 545 Rust tests / clippy 零警告 /
> drift 2 passed / sdk 37 / web 219 / web build / tui 71 / check-version /
> desktop-ui。
> 验收（v0.15.1）：发现并修复 2 bugs（BUG-005 TCP/dashboard trajectory 接线；
> BUG-006 initialize `protocol_version` 常量），回归 verified，门禁全绿（cargo 549 tests）。
> v0.15.2 bugfix：session 生命周期方法协议已声明但 IPC dispatch 未实现（-32601）——
> `session/status`（Web/TUI 会话点击失败）+ `session/archive`（TUI 已导出）/`session/delete`，
> 补齐 dispatch（`-32802` / `-32803 SessionInactive` + error.data），回归 4 tests（cargo 553 tests）。
> v0.16.0 Web 工作台体验修复（六项，无协议变更）：协议版本 semver 兼容语义（major
> 相同即兼容，仅 major 不匹配才显示 banner；0.12.1 client vs 0.13.0 server 不再误报）；
> composer 选项弹层逐触发器锚定（`.sel-wrap`）；选项/会话行 hover 反馈；light 主题
> 对比度（`--aj-border #E4E7EA→#CBD2DB`、`--aj-raised #F9FAFB→#E9EDF1`）；左栏会话管理
> （逐行删除 + 一键清理，`session/delete` 幂等容忍）；对话区/composer 全宽拉伸。
> Web 219 → 230 tests；PROTOCOL_VERSION 保持 0.13.0；工作区 0.15.2 → 0.16.0。
> v0.16.1 Web 工作台跟随修复（三项，无协议变更）：左栏会话删除按钮移至收藏星标
> 下方（右栏堆叠、行高 56px）；移除右上角模型 chip（composer 与 usage meta 行
> 已展示）；会话单击不再自动收藏（仅星标按钮触发，回归测试锁定）。
> Web 230 → 231 tests；PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.0 → 0.16.1。
> v0.16.2 远程访问修复（bugfix，无协议变更）：`serve --port` / `dashboard` 新增 `--host`
> （默认仍 127.0.0.1 本机信任；`--host 0.0.0.0` 支持局域网/远程直连），`dashboard` 补齐
> `--token`，非回环绑定无有效 `--token`（含空串）时输出暴露告警，启动日志打印实际
> `ws://<host>:<port>/ws`（IPv6 自动加方括号、端口取实际绑定值）；Web
> `resolveWsUrl` 页面地址派生（无 `VITE_AGENTJ_WS_URL` 时取页面 `host:4096`，https 自动
> wss，`tauri.localhost` 保持明文回环），「连接设置」手动连接持久化到
> `agentj.connection`（刷新自动恢复并重连、弹窗回填），修复远程打开页面仍连
> `ws://127.0.0.1:4096/ws`、刷新丢失指定 IP 的问题。
> Rust 553 → 560 tests / sdk 37 → 43 / web 231 → 239；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.16.1 → 0.16.2。
> v0.16.3 Web 工作台修复（两项，无协议变更）：(1) 左栏「项目」区块（项目分组 +
> 底栏「+ 添加项目」+ 添加对话框）整体迁入右上角「...」菜单二级菜单（位于「连接设置」
> 下方）：子菜单列全部项目（当前项 ✓ / menuitemradio + aria-checked）、选择即切换
> 当前项目（localStorage + 新 `agentj:current-project-updated` 事件同步 composer
> picker 与侧栏新建会话归属）、「＋ 添加项目」复用 ModalDialog；侧栏保留扁平最近会话
> 列表。(2) 文件 tab 永久「加载中」修复（三层根因）：长驻 Vite dev server 不监听
> root 之外的 `packages/sdk` 源（陈旧 SDK 无 `METHODS.fileList` → 请求缺 `method`
> 字段）+ 内核对非法结构请求回 `-32700` + `id:null`（客户端无法匹配响应）+ SDK 无
> method 校验；修复 = 内核 `parse_request`（可确定 id → `-32600` 回显 id；语法错误
> 仍 `-32700` + null，对齐 docs/specs/ipc-protocol.md）+ SDK `RpcClient.request`
> 空/未定义 method 直接抛 `invalid RPC method`。
> （PR #53 同期含 9801187「连接设置持久化对畸形输入加固」+2 tests。）Rust 560 → 564 tests /
> sdk 43 → 46 / web 239 → 246；PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.2 → 0.16.3。
> v0.16.4 Web 工作台体验修复（六项 + 验收 1 bug，无协议变更）：(1)(2) 左上角仅
> `agentj` + 状态灯（移除 `connected` 文字）；连接信息（ws/模型/模块状态）移入新建
> `ConnectionStatus` 组件的 hover/focus 面板（条件渲染，闭合不在 DOM）；⋯ 菜单
> 移除状态区块与分隔线。(3) 右栏「任务」「文件变更」升级 VSCode 手风琴：折叠 =
> 36px 紧凑标题行自上而下堆叠、展开 = 内容向下延展，空数据保留标题行（占位符仅
> 展开显示），展开状态 localStorage 记忆（`agentj.layout.rightSections`，默认全
> 展开）；新增 `useRightSection` hook。(4) 全局 token 化滚动条（webkit 10px 圆角
> thumb + Firefox `scrollbar-width: thin`，随 light/dark）。(5) composer 模型/agent/
> 熵变弹层二次点击收起（OptionPopover 可选 `anchorRef`：mousedown 命中触发器时跳过
> 关闭，修复 mousedown→click 竞态导致的「关了就重开」）。(6) 新建会话不再自动收藏
> （仅星标按钮触发）。验收（实机 Playwright）修复 BUG-007：顶栏 `z-index: 5` 低于
> 侧栏 `z-index: 10` → hover 面板/顶栏下拉被侧栏遮挡 → `.aj-topbar z-index: 20` +
> CSS 契约回归测试。Rust 564 tests / sdk 46 / web 246 → 263；PROTOCOL_VERSION 保持
> 0.13.0；工作区 0.16.3 → 0.16.4。
> v0.16.5 Web 工作台四项修复（无协议变更）：(1) 右栏折叠/展开改为单段右→左平滑
> 动画——外层 `.aj-shell__right` `width` 过渡（40px ↔ 存储宽，仅 ≥1024 规则，去除
> 不可过渡的 flex-basis/min/max 钳制），内层 `.aj-right` `align-self: flex-end`
> 右缘锚定（内容零回流、仅揭示；collapsed 头部 chrome 右对齐 4px 内边距使 « 按钮
> 落在 40px 可视窗），SplitPane 拖拽期间以兄弟选择器
> `.aj-splitter[data-side="right"][data-dragging] ~ .aj-shell__right` 关闭过渡；
> (2) 顶栏「...」菜单重构为恰好五项（项目 / 继续会话… / 收藏会话 / 设置… / 帮助）：
> 项目与收藏会话改为悬停向「左」飞出的二级菜单（`aria-haspopup/expanded`、hover
> 桥接 + 120ms 关闭延迟、Esc 分层关闭、焦点/键盘可达、点击兼容触屏），排序/过滤/
> 演示数据/连接设置全部迁入新 `SettingsDialog` 弹窗（ModalDialog 加宽卡 + 左
> `role=tablist` 连接/会话列表/演示数据 + 右 `tabpanel`；↑↓/Home/End 切换），新增
> `sidebarPrefsStore`（`agentj.sidebarPrefs`，排序/过滤持久化，替换 legacy 事件）；
> (3) 右栏「任务」「文件变更」升级成对面板：仅一个展开时铺满至面板底部；都展开时
> 渲染 `role=separator` 水平分隔条（拖拽取指针位置比例 = CSS flex-basis 精确逆映射，
> 零漂移；↑↓ ±2%（Shift ±8%）、双击/Home 复位自动），比例持久化
> `agentj.layout.rightSectionSplit`（0.15–0.85，缺省 null=内容自适应），面板
> `data-open/data-section` + 打开状态经 `useSyncExternalStore` + 同 tab 事件
> `agentj:right-sections-updated` 跨实例同步；
> (4) 文件树图标体系：目录 Folder/FolderOpen 开合切换、按扩展名分类的 lucide 图标
> （新 `fileIcons.ts` 纯映射，21 单测）、任意深度内联 padding 缩进（替换 4 级上限）、
> token 化悬停/focus-visible/分类配色与 chevron 旋转过渡。验收（实机 Playwright）：
> 4 项全过（折叠/展开 13 帧单调过渡、内层宽恒 320 零回流；菜单左向飞出 + 设置弹窗
> 持久化 + 焦点归还；面板拖拽 ±120px 1:1、刷新保留、Home 复位；目录开合图标 +
> 分类图标 + 30px 深缩进）。Rust 564 tests / sdk 46 / web 263 → 308；
> PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.4 → 0.16.5。
> v0.17.0 Web 工作台六项增强（纯前端，无协议变更）：(1) 对话代码块卡片化 ——
> `rehype-highlight`（hljs common 语言集，`.hljs-*` 映射现有语义色变量）+
> 新 `CodeBlockCard`（`figure > figcaption + pre`：左上折叠/展开保留 header +
> 语言标签、右上复制；折叠不卸载，流式追加保持折叠态）+ 新 `copyText`
> （`navigator.clipboard` → textarea/`execCommand` 降级链 + 失败瞬态，
> `http` 局域网非安全上下文可用）；(2) 显示设置 —— 新 `displayPrefsStore`
> （`agentj.displayPrefs`：对话/左栏/右栏/代码四档字号 + 行高紧凑 1.4/默认 1.55/宽松 1.8
> + 密度 + 减弱动画）+ `SettingsDisplayPanel`（设置「显示」tab + 「恢复默认」）+ 新
> `displayPrefs.css` 覆盖层（最后导入、`var()` fallback 保值）+ `WorkspaceShell`
> inline `--aj-font-conv/-left/-right/-code`、`--aj-lh-conv`、`data-density` +
> `App` effect 写 `documentElement[data-reduced-motion]`；(3) 文件树源码图标 ——
> `fileIcons.ts` 扩展主流源码扩展名（rs/go/py/java/c/cpp/cs/rb/php/kt/swift/scala/lua/dart 等）
> + 新 db 类（`Database`）+ 文件名特例（Dockerfile/Makefile/CMakeLists.txt → code）+
> `.file-icon--db` 配色；(4) 会话列表菜单 —— 新 `SessionListMenu`（左栏清理按钮改为
> ⋯ 菜单：`清理所有会话…` 打开既有确认弹窗、无可删禁用；`过滤 ›`/`排序 ›` 悬停向右
> 飞出二级 `menuitemradio`，镜像 TopbarMenu 延迟/桥接/Esc 分层/键盘；直写
> `sidebarPrefsStore`）—— 设置弹窗「会话列表」tab 整体移除；
> (5) 项目编辑 —— `projectStore` 扩展 `workDir/isDefault/defaultModel/defaultAgent`
> 可选字段（零迁移）+ `updateProject`（`isDefault` 互斥 + 修复 rename 不通知）/
> `deleteProject`（Inbox 守卫 + 会话归属回退 + 计数）+ `ProjectDefaults`；新
> `ProjectEditDialog`（名称 / 工作目录绝对路径校验 / 默认项目 / 默认模型 / Agent）+
> composer picker 每行编辑按钮 + `session/new` 首次携带 `project_root`（工作目录打通）+
> 项目默认模型/Agent 随切换应用；(6) 设置「项目」tab —— `SettingsProjectPanel`
> （全局默认项目 + 新项目工作目录预填 + 项目列表 名称/工作目录/默认徽标/编辑/删除，
> Inbox 不可删、删除确认展示会话回退数）+ `ModalDialog` 模块级模态栈（嵌套弹窗仅栈顶
> 响应 Esc/遮罩）。门禁全绿：web 308 → 401 tests（43 files）/ sdk 46 / Rust 564 tests /
> clippy 零警告 / drift 2 passed / web build / tui 71 / check-version 0.17.0 / desktop-ui。
> 新依赖 `rehype-highlight`（web workspace）；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.16.5 → 0.17.0。
> v0.17.1 验收修复（bugfix，无协议变更）：实机 Playwright 验收 v0.17.0 六项时发现
> BUG-008 —— 窄视口（375–390px）左栏会话列表菜单超出右缘被 `.aj-sidebar` 裁剪，
> 「过滤/排序」二级飞出层亦溢出视口不可点。修复：导出 `clampMenuLeft`
> （菜单左缘夹取到 `[8, innerWidth - menuWidth - 8]`）与 `chooseFlyoutSide`
> （行右缘 + gap + 飞出宽 > 视口宽时向左展开），飞出层方向自适应
> （`.aj-menu__flyout--left`）；resize 经 `updateMenuPos` 重算。桌面（≥1024）行为不变。
> web 405 → 417 tests（43 files）/ sdk 46 / Rust 564 tests；clippy 零警告 / drift 2 passed /
> web build / tui 71 / check-version 0.17.1 / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.17.0 → 0.17.1。
> v0.17.2 界面修复（bugfix，无协议变更，用户直报 2 项）：(1) 设置「显示」字号设置由
> 四档枚举（小/中/大/特大 × 4 区，16 个单选纵向堆叠致弹窗过度拉伸）改为逐区像素数值
> —— `displayPrefsStore` 字号字段转为 px（`FONT_SIZE_LIMITS` 校验：对话 12–24 默认 15、
> 左右栏 10–20 默认 13、代码 10–20 默认 12；clamp+取整；旧 sm/md/lg/xl 按区迁移），
> `SettingsDisplayPanel` 每区一行「标签 + range 滑杆 + number 输入 + px」（draft 受控，
> blur/Enter 提交夹取），行高/密度选项横排；`WorkspaceShell` 直接注入 `${px}px`。
> (2) 弹窗边框/四角可缩放 —— `ModalDialog` 新增 `.aj-dialog-frame`（承载尺寸/位置，
> 首次拖拽冻结 fixed 矩形）+ 8 个指针手柄（4 边 + 4 角，hover accentTint + 方向光标）；
> 新 `dialogResize.ts` 纯函数 `resizeRect`（对边锚定、最小 320×200、视口 8px 边距夹取、
> 可选 per-dialog `minWidth/minHeight`，设置弹窗下限 560px）；`.aj-settings` 宽度修饰
> 移至 frame，`.aj-dialog-card` 基类规则不变（`ApprovalDialog` 不受影响）。
> 实机 Playwright 验证：4 行横向布局、滑杆/数字/变量联动与夹取、SE/W 拖拽（对边锚定）、
> 最小尺寸 560×200（滑杆仍 200px 可用）、hover 手柄反馈。web 417 → 439 tests（44 files）/
> sdk 46 / Rust 564 tests；clippy 零警告 / drift 2 passed / web build / tui 71 /
> check-version 0.17.2 / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.1 → 0.17.2。
> (17 crates + agentj-ui monorepo：Web / TUI / Desktop + SDK + tokens，
> npm workspaces `apps/*`+`packages/*`，共享 SDK + protocol + tokens，
> WebSocket 网关同端口复用，后端零改造，frontend stays in-repo)

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。
v0.17.2 已落地：**界面修复（显示设置 px 滑杆 + 可缩放弹窗，无协议变更）**
（(1) 显示字号：`displayPrefsStore` 转像素数值 + 旧枚举迁移，`SettingsDisplayPanel`
每区一行滑杆/数字/px + 行高密度横排，消除弹窗纵向拉伸；WorkspaceShell 注入 px；
(2) 弹窗缩放：`ModalDialog` `.aj-dialog-frame` + 8 向手柄 + `dialogResize.ts`
几何（对边锚定/最小 320×200/视口夹取/可覆盖下限，设置 560px）；实机 Playwright 验证。
web 417 → 439 tests（44 files）/ sdk 46 / Rust 564 tests / tui 71；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.1 → 0.17.2）。
v0.17.1 已落地：**v0.17.0 验收修复（BUG-008，无协议变更）**
（窄视口 375–390px 左栏会话列表菜单横向夹取 + 「过滤/排序」飞出层方向自适应
`.aj-menu__flyout--left`；`clampMenuLeft` / `chooseFlyoutSide` 导出纯函数 + resize 重算；
桌面行为不变。web 405 → 417 tests（43 files）/ sdk 46 / Rust 564 tests / tui 71；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.0 → 0.17.1）。
v0.17.0 已落地：**Web 工作台六项增强（纯前端，无协议变更）**
（(1) 对话代码块卡片化：`rehype-highlight` 高亮 + `CodeBlockCard`（折叠/展开 +
复制）+ `copyText`（clipboard → execCommand 降级链）；(2) 显示设置：
`displayPrefsStore` + 设置「显示」tab + `displayPrefs.css` 覆盖层 +
WorkspaceShell `--aj-font-*`/`--aj-lh-conv`/`data-density` + App
`data-reduced-motion`；(3) 文件树源码/db 图标 + 文件名特例；
(4) 左栏 `SessionListMenu`（清理所有会话 + 过滤/排序向右飞出二级菜单，
设置「会话列表」tab 移除）；(5) 项目编辑：`projectStore` 可选字段 +
`ProjectEditDialog` + composer 行内编辑 + `project_root` 随 `session/new` 打通 +
项目默认模型/Agent 随切换应用；(6) 设置「项目」tab（全局默认 + 列表 + 守卫删除 +
`ModalDialog` 嵌套栈）。web 308 → 401 tests / sdk 46 / Rust 564 tests / tui 71；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.5 → 0.17.0）。
v0.16.5 已落地：**Web 工作台四项体验修复（无协议变更）**
（(1) 右栏折叠/展开单段右→左平滑动画（外层 width 过渡 + 内层 `align-self:
flex-end` 右缘锚定、内容零回流、仅 ≥1024 动画、拖拽期间 sibling 选择器禁用过渡）；
(2) 顶栏「...」五项菜单（项目/继续会话…/收藏会话/设置…/帮助），项目与收藏会话
hover 左向飞出二级菜单（桥接 + 延迟关闭 + Esc 分层 + 键盘可达），排序/过滤/演示
数据/连接设置迁入 `SettingsDialog`（左 tablist + 右面板），新 `sidebarPrefsStore`
持久化 `agentj.sidebarPrefs`；(3) 右栏任务/文件变更成对面板：单开铺满至底部，
双开拖拽/键盘调节 + `agentj.layout.rightSectionSplit` 比例持久化（精确逆映射零漂移，
Hand/双击复位）；(4) 文件树 `fileIcons.ts` 分类图标（Folder/FolderOpen + 扩展名
映射）+ 任意深度缩进 + token 化美化。验收（实机 Playwright）4 项全过。
Rust 564 tests / sdk 46 / web 263 → 308；PROTOCOL_VERSION 保持 0.13.0；
工作区 0.16.4 → 0.16.5）。
v0.16.4 已落地：**Web 工作台体验修复（六项 + 验收 1 bug，无协议变更）**
（(1)(2) 顶栏左上角仅 `agentj` + 状态灯；连接信息（ws/模型/模块状态）移入状态灯
hover/focus 面板（条件渲染），⋯ 菜单移除状态区块；(3) 右栏「任务」「文件变更」升级
VSCode 手风琴——折叠 = 紧凑标题行堆叠（36px）、展开 = 向下延展、空数据保留标题行，
展开状态 localStorage 记忆（默认全展开，`useRightSection` + `agentj.layout.rightSections`）；
(4) 全局 token 化滚动条（webkit + Firefox，随主题）；(5) composer 选项弹层二次点击
收起（`OptionPopover.anchorRef` 修复 mousedown→click 竞态）；(6) 新建会话不再自动
收藏（仅星标）；验收修复 BUG-007：顶栏 z-index 5 < 侧栏 10 致 hover 面板被遮挡 →
z-index 20 + 契约测试。Rust 564 tests / sdk 46 / web 246 → 263；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.3 → 0.16.4）。
v0.16.3 已落地：**Web 工作台修复（项目菜单 + 文件树，无协议变更）**
（(1) 左栏「项目」区块 / 底栏「+ 添加项目」/ 添加对话框迁入顶栏「...」菜单二级菜单
（连接设置下方；子菜单列项目 + 当前 ✓ + 「＋ 添加项目」；选择即切换当前项目，
新增 `agentj:current-project-updated` 事件同步 composer picker 与侧栏新建会话归属）；
(2) 文件 tab 永久「加载中」修复：内核非法结构请求回显可确定 id（`-32600`；语法错误
仍 `-32700` + null）+ SDK 拒绝空/未定义 method；根因 = 长驻 dev server 陈旧
`packages/sdk`（无 `METHODS.fileList` → 请求缺 `method`）+ `id:null` 响应不可匹配；
Rust 560 → 564 tests / sdk 43 → 46 / web 239 → 246（含 9801187 连接设置持久化加固 +2）；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.2 → 0.16.3）。
v0.16.2 已落地：**远程访问修复（bugfix，无协议变更）**
（(1) `serve` / `dashboard` 新增 `--host` 绑定选项：默认仍 127.0.0.1 保持本机信任，
`--host 0.0.0.0` 支持局域网/远程直连；`dashboard` 补齐 `--token`，非回环且无有效
`--token`（含空串）时输出暴露告警；启动日志打印实际 `ws://<host>:<port>/ws`
（IPv6 加方括号、端口取实际绑定值）。(2) Web SDK `resolveWsUrl` 页面地址派生：
无 `VITE_AGENTJ_WS_URL`/显式 host 时取 `location.hostname:4096`，https → wss，
`tauri.localhost` 守卫保持明文回环，Web App 初始化与手动连接均传入 `window.location`。
(3) Web 连接设置持久化：手动连接存入 `agentj.connection`，刷新自动恢复并重连，
弹窗回填已保存值；显式设置优先于 `VITE_AGENTJ_WS_URL`。
Rust 553 → 560 tests / sdk 37 → 43 / web 231 → 239；PROTOCOL_VERSION 保持 0.13.0；
工作区 0.16.1 → 0.16.2）。
v0.16.1 已落地：**Web 工作台跟随修复（三项，无协议变更）**
（(1) 左栏会话删除按钮移至收藏星标下方（右栏堆叠、行高 56px）；
(2) 右上角模型 chip 移除，模型仍由 composer 与 usage meta 行展示；
(3) 会话单击不再自动收藏，仅星标按钮触发（回归测试锁定）；Web 230 → 231 tests；
PROTOCOL_VERSION 保持 0.13.0；工作区 0.16.0 → 0.16.1）。
v0.16 已落地：**Web 工作台体验修复（六项，无协议变更）**
（(1) 协议版本 semver 兼容语义：major 相同即兼容，仅 major 不匹配才显示 banner，
0.12.1 client vs 0.13.0 server 不再误报；(2) composer 选项弹层逐触发器锚定
（`.sel-wrap`），不再偏移到发送按钮附近；(3) 选项/会话行 hover 反馈
（背景 + accent + 过渡）；(4) light 主题对比度
（`--aj-border #E4E7EA→#CBD2DB`、`--aj-raised #F9FAFB→#E9EDF1`）；
(5) 左栏会话管理：逐行左侧删除 + 一键清理（`session/delete`，排除当前会话，
-32802/-32803 幂等容忍，`session/list` 过滤 deleted）；(6) 中栏对话区/composer
移除 720px 居中上限、全宽拉伸；Web 219 → 230 tests；PROTOCOL_VERSION 保持 0.13.0；
工作区 0.15.2 → 0.16.0）。
v0.15 已落地：**协议扩展 —— 三栏工作台真实数据源（后端 + 三端前端）**
（`file/list` 沙箱化目录懒加载 + git 状态点（M/A/U/D）；`$/progress`
`kind:"todo"` 全量快照 + 会话内单调 `revision`、`kind:"file_change"`
（write 自研行级 LCS diff ≤64 KiB / bash git 前后采样 + numstat，
tracked 走 `git diff HEAD`、untracked 仅计数）；`$/askQuestion` +
`question/respond` owner 定向提问往返（重复响应幂等 `accepted:false`，
超时/取消/owner 断开按 skip 结算一次）；`session/new` 加性 `project_root`
（canonicalize 持久化，file/list 沙箱 / bash cwd / 权限 workspace_root /
工具相对路径 / file_change 相对路径五处同源）；新错误码
`-32807 PathNotAllowed` + `error.data`；`session/history` 回放重建
TODO 快照/变更列表/问答记录；新 crate `agentj-question`（16 → 17 crates）；
Web `TodoPanel`/`ChangeList`/`FileTree`/`AskQuestionCard` 由真实协议驱动
（`deriveChanges` 半推导退役，demo 模式保留为可切换预览通道）；
TUI runview 只读 TODO 区块；协议 0.13.0（21 方法 / 51 schemas），
工作区 0.14.6 → 0.15.1（minor + 验收 patch））。
v0.14 已落地：**agentj-ui 三栏工作台（Web）**
（三栏布局：`WorkspaceShell` 组合 Sessions/Files 左栏 + 会话中栏 +
运行详情右栏；`SplitPane` 拖拽/键盘/双击 + `layoutStore` 宽度持久化
+ `<1024` 右抽屉；右栏 `RightPanel` = `TodoPanel` + `ChangeList`
（unified diff 展开，`deriveChanges` 从 write tool_call 半数据派生）；
composer 模型/Agent/熵变三选择器（`OptionPopover` + `ComposerOptions`，
互斥 + localStorage 持久化）；`AskQuestionCard` 模型提问卡片（可折叠 +
问答记录）；`TopbarMenu` 顶栏化（连接设置/继续会话/收藏/排序/过滤/
设置/帮助 + demo 开关）；`demoData` 顶栏开关驱动 TODO/文件树/变更/
提问卡 demo 数据；tokens 19 → 24 keys + `LAYOUT` 常量块
（kebab→camel radius 迁移）；后端零改造，协议 gap 以 demo/空态呈现，
0.13.0 → 0.14.6 patch）。
v0.11 已落地：**agentj-ui 三端统一前端**
（`web-dashboard` → `agentj-ui/apps/web` 重命名，`bindings` 迁入
`agentj-ui/packages/protocol`，新建 `packages/sdk` 共享 SDK，
三端同源消费 `@agentj/sdk` + `@agentj/protocol`，后端零改造；
Web 对齐：方法常量化/`?token=` 透传/`protocol_version` 同源/
会话收藏/composer/`$/progress`/审批对话框/usage；
TUI 全功能：Bun + @opentui/react，连接管理/会话列表/新建/继续/
composer 多行/@补全//palette/流式渲染/审批模态/模型只读/主题/usage/
优雅退出 + mock WS e2e；
Desktop：Tauri v2 壳套 `apps/web` 产物，四件套 deb/AppImage/dmg/msi，
`check-artifacts.sh` 校验；G7/G8/WebSocket 等 0.10 能力全保留）。

## Setup

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`agentj-ui/apps/web` + `agentj-ui/packages/protocol`
  + `agentj-ui/packages/sdk` 构建与测试需要）
- Bun ≥1.2（`agentj-ui/apps/tui` dev/build/test 需要）
- Rust 工具链（`agentj-ui/apps/desktop` Tauri 壳构建需要）
- DeepSeek API Key（v0.1 集成测试与手动验证用，当前阶段未使用）

## Commands

- `cargo test --workspace` — 全量测试
  （564 passed / 0 failed；含 bus 密封 + FileBackedStore/Config/CLI 单测
  + e2e_cli/e2e_permission/e2e_serve/e2e_ws 密封测试）
- `cargo clippy --workspace --all-targets -- -D warnings` — lint 门禁（零警告）
- `cargo bench -p agentj-model` — 模型流式基准（criterion）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（51 schemas 无漂移）
- `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` — 生成 protocol 类型
  （`schemas/*.json` → `agentj-ui/packages/protocol/types.d.ts`）
- `npm --prefix agentj-ui run build --workspace=@agentj/protocol` — `tsc --noEmit` 校验
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — SDK 单测（client/router/transports/config，
  46 tests）
- `npm --prefix agentj-ui run generate --workspace=@agentj/sdk` — 生成 `METHODS` 常量
  （`protocol/methods.json` → `agentj-ui/packages/sdk/src/methods.ts`）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — Web 单测（vitest，439 tests / 44 files）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — Web 构建（`tsc -b && vite build`）
  > 注意：Vite dev server 不监听 root 之外的 `packages/sdk` 源（alias 直连源码）；
  > 改动 SDK 后须重启 dev server，否则运行陈旧的 `METHODS`（v0.16.3「加载中」根因）。
  > Web 新依赖 `rehype-highlight`（hljs common 语言集，随 `npm --prefix agentj-ui install`
  > + lockfile 落地，v0.17.0）。
- `bun --cwd agentj-ui/apps/tui test` — TUI 单测（含 mock WS e2e，71 tests）
- `./agentj-ui/apps/desktop/check-artifacts.sh linux` — Desktop 产物校验
- `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — Desktop 前端产物/配置一致性校验
- `./scripts/check-version.sh` — 版本一致性（Cargo + sdk VERSION + desktop 均为 0.17.2，
  PROTOCOL_VERSION 0.13.0 + `protocol/methods.json` 版本 0.13.0）

### CI Pipelines (GitHub Actions)

- `.github/workflows/ci.yml` — 单元测试（PR + push main + 手动）：
  `cargo fmt`、`cargo clippy`、`cargo test --workspace --locked`、
  `cargo llvm-cov --workspace --fail-under-lines 50`
- `.github/workflows/smoke.yml` — 冒烟（PR + push main + 手动）：
  `cargo build --release` + `agentj --version/--help` 断言 + `cargo test -p agentj-core -- smoke`
- `.github/workflows/e2e.yml` — E2E（main 合并后 + 手动）：
  `cargo test --release --test e2e_cli --test e2e_permission`
- 本地校验：`scripts/validate-workflow.sh <workflow-file> <jobs...>`

## Architecture

> 全部 17 个 crate 位于 `crates/`（`crates/*`），前端 monorepo 位于
> `agentj-ui/`（npm workspaces，frontend stays in-repo，不拆独立仓），
> 下文 `agentj-*` 均为 `crates/agentj-*`，`@agentj/*` 为 `agentj-ui` 包。

- `agentj-protocol`（leaf）：IPC 协议权威 wire 类型（JSON-RPC 2.0、
  错误码、21 方法 params/result、流式/订阅/权限/提问/文件列举类型），
  serde + schemars，零内核耦合。v0.6 加性 CancelParams/PermissionRequestParams
  session_id；v0.8 加性 UsageInfo/SessionRunResult.usage；v0.13 加性
  `file/list`、`$/progress` todo/file_change、`$/askQuestion` + `question/respond`、
  `SessionNewParams.project_root`、`-32807`（21 方法 / 51 schemas）。
- `agentj-core`（内核）：Kernel 混合总线 + 模块 trait 注册表
  + ServiceRegistry（同步/异步） + TurnRunner 总线化 turn 编排
  + `ToolCallContext`（会话根上下文，工具相对路径重定基）
  + `TurnArtifact`（FileChange/Todo 产出 + 自研 unified diff）
  + 事件总线重入防护（EventGuard thread_local + MAX_EVENT_DEPTH=16）。
- `agentj-config`：TOML 配置（[model]/[permission]/[session]/[license]/[[mcp.servers]]）。
- `agentj-session`：会话生命周期（InMemoryStore/FileBackedStore + 过期清理
  + 会话级 `project_root` 持久化，serde default 兼容旧快照）。
- `agentj-permission`：权限系统（决策引擎 + 60s 超时 + allow_always）。
- `agentj-question`：提问通道（`QuestionModule` + `AskTool`，镜像 permission 的
  pending/owner 归属；`ask_blocking` 阻塞等待、`respond` 幂等 `accepted:false`、
  `timeout_s` 可空、`set_notifier`/`set_settle_hook` 统一结算一次）。
- `agentj-model`：模型提供方（三线 wire-format + opencode-go + Mock + Ollama）。
- `agentj-tools`：内置工具（bash/glob/grep/read/write + `todo`，共 6 个）。
- `agentj-context`：上下文管线。
- `agentj-stats`：用量统计 UsageCollector。
- `agentj-license`：Ed25519 许可证校验（entitlements + free mode）。
- `agentj-mcp`：MCP 客户端 + McpConnectionManager 长驻连接
  （McpServerSpec + close_all）。
- `agentj-skills`：skills 加载器。
- `agentj-plugin`：插件系统（manifest + 状态机 + 依赖图 + libloading）。
- `agentj-ipc`：JSON-RPC IPC（stdio/TCP/WS + 同端口分类器
  + WsConnection + WsServerState/ws_router/serve_http + per-session 路由
  + `file/list` dispatch（沙箱 + git 状态）+ question notifier/settle hook 接线
  + `error.data` 透传 + 非法请求回显可确定 id（`-32600`；语法错误 `-32700`+null，v0.16.3））。
- `agentj-cli`：`agentj` 二进制（run/serve/dashboard，模块注册进 Kernel，
  agentj-question 的 stdio 与 `serve --port` 两条装配路径）。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具。
- `schemas/*.json`：51 JSON Schema（schemars 生成，提交件 + drift 门禁）。
- `protocol/methods.json`：方法注册表（method ↔ schema/方向/类型）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节）。
- `agentj-ui/packages/protocol`：协议包（原 `bindings` 迁入，
  `schemas/*.json` → `types.d.ts`，`json-schema-to-typescript` + `tsc --noEmit`，
  为 drift 门禁后第一门）。
- `agentj-ui/packages/sdk`：共享 SDK（`src/client.ts` RpcClient +
  `src/router.ts` ResponseRouter/RpcTransport + `src/config.ts`
   `VERSION="0.17.2"` + `PROTOCOL_VERSION="0.13.0"` + `resolveWsUrl` + `src/methods.ts` METHODS
  + `transports/websocket.ts` WebSocketTransport
  + `transports/tcp.ts` TcpTransport；`RpcClient.request` 拦截空/未定义 method（v0.16.3），
  被 Web/TUI/Desktop 同源消费）。
- `agentj-ui/packages/tokens`：共享语义 tokens（`palette.ts` 24 keys × dark/light + `SEMANTIC_KEYS` + `LAYOUT` 常量块（left/right 宽度/splitter/topbar/gap），`tokens.css/tokens.json/docs/ui-tokens.md` 生成，radius 类 keys kebab→camel 迁移，三端约束）。
- `agentj-ui/apps/web`：Web 前端（Vite + React 19，消费 `@agentj/sdk` + `@agentj/tokens` + `lucide-react`；
  **三栏工作台（真实协议驱动）**：`WorkspaceShell`（Sessions/Files 左栏 `SidebarTabs` +
  会话中栏 + 运行详情右栏） + `SplitPane`（拖拽/键盘/双击） +
  `RightPanel`（`TodoPanel` ← `$/progress` todo 全量快照 + revision 门控；
  `ChangeList` ← `file_change` upsert + unified diff） +
  `FileTree`（`file/list` 懒加载 + git 状态点） +
  `OptionPopover`/`ComposerOptions`（模型/Agent/熵变） +
  `AskQuestionCard`（`$/askQuestion` → `question/respond`） +
  `TopbarMenu`（v0.16.5 五项：项目/继续会话…/收藏会话/设置…/帮助；项目与收藏会话
  hover 左向飞出二级菜单 + 关闭延迟 + Esc 分层 + 键盘可达） +
  `ConnectionStatus`（状态灯 + hover 连接面板）；
  **v0.17.0 六项增强**：`CodeBlockCard`（`pre` 覆盖 + `copyText` 降级链 +
  `rehype-highlight`，`codeblock.css`）/ `displayPrefsStore` + `SettingsDisplayPanel` +
  `displayPrefs.css` 覆盖层 / `fileIcons.ts` 源码与 db 图标 / `SessionListMenu`
  （`过滤`/`排序` 向右飞出）/ `ProjectEditDialog` + `SettingsProjectPanel`（设置「项目」tab）+
  `projectStore` 扩展（`workDir/isDefault/defaultModel/defaultAgent` + update/delete/defaults）+
  `project_root` 随 `session/new` 打通；`SettingsDialog` 分类 = 连接/显示/项目/演示数据
  （排序/过滤迁入 `SessionListMenu`，`sidebarPrefsStore` 持久化 `agentj.sidebarPrefs`）；
  `ModalDialog` 模块级模态栈（嵌套弹窗仅栈顶响应 Esc/遮罩）；
  **v0.17.2 界面修复**：显示字号逐区 px（`FONT_SIZE_LIMITS` + 旧枚举迁移）+
  `SettingsDisplayPanel` 每区一行「滑杆 + number + px」/ `ModalDialog`
  `.aj-dialog-frame` + 8 向缩放手柄 + `dialogResize.ts`（对边锚定/夹取，
  设置下限 560px）；
  `layoutStore` 宽度/面板持久化，
  `todoState`/`changes` reducer，`deriveChanges` 半推导退役；
  布局/会话/Composer/对话/轨迹/Hero/对话框拆 `components/*.css`，vitest + vite build）。
- `agentj-ui/apps/tui`：TUI 前端（Bun + @opentui/react，全功能：
  connection/session/composer/runview/permission/palette/models/
  completion/theme/usage + runview 只读 TODO 区块（revision 门控） +
  mock WS e2e + sidebar 文本版 UnifiedSidebar 32char；`testRender` 快照）。
- `agentj-ui/apps/desktop`：Desktop 壳（Tauri v2，`frontendDist` →
    `../../web/dist`，与 workspace 版本同源 `0.17.2`，四件套打包）。
- `agentj-ui/` 根：`package.json` workspaces `["apps/*","packages/*"]`
  （frontend stays in-repo，前端不拆独立仓，与 Rust 共享 0.17.2）。
- **守护内核（v0.4 已实现）**：统一 Connection 抽象 + `serve --port`
  常驻多客户端 + `run --attach` + `dashboard`（见 daemon-kernel 设计文档）。

> Crate 边界：Rust 17 crate 保持 `crates/*` 边界纯净，
> 前端（TS/React/Bun/Tauri）全部收敛于 `agentj-ui/` 内，
> 通过 `protocol/methods.json` + `schemas/*.json` 与后端契约对齐，
> 不向 Rust workspace 注入前端构建产物。

## Conventions

- 协议 wire 字段一律 snake_case（`protocol_version`、`session_id` 等）。
- 协议版本：`initialize` 握手 + 语义化版本 + 加性纪律（只加不改；
  未知能力/枚举值容忍）。
- 值枚举带 `#[serde(other)]` + `#[schemars(skip)]` Unknown 变体（向前兼容）。

## Rules

- Commit messages in English. `docs/superpowers/specs/` docs in Chinese.
- Docs under `docs/` are tracked deliverables — commit them.
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`).
- Before planning/developing, read `.agents/summary/summary_and_next.md` and `docs/superpowers/` first.
- All diagrams in spec docs under `docs/superpowers/specs/` must use UML or Mermaid.
- Update `docs/superpowers/specs` target spec docs TODO list after each phase.
- `.agents/summary/summary_and_next.md` only retains currently developing feature:
  progress, remaining issues, next steps. Replace, don't append.
- After each plan completes, update summary with current feature's progress,
  remaining issues, next plan/next steps.
- After each feature phase completes AND `cargo test` + `clippy` pass, auto:
  - Update `AGENTS.md` / `README.md` / `.agents/summary/summary_and_next.md`
  - Commit as `docs: auto-sync AGENTS.md, README.md, summary after {phase}`.
  - Do NOT wait for user — mandatory post-phase step.
- Clean up temp files, Docker containers after each phase.
- When a command hangs >120s, cancel and retry.
- PR creation: create PR and share link — user handles merge.
- Rust function ≤50 lines, ≤120 chars per line.
