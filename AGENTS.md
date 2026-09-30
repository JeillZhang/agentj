# AGENTS.md

> Updated 2026-09-29 — v0.24.2 右栏遮挡修复（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）：用户直报「拖拽右边栏运行详情展宽时，文字逐渐被中间栏右侧遮挡」→ 三层根因：① `.aj-shell__center { flex: 1 1 auto }` 基准取内容宽度，中栏长代码块/宽工具卡把 flex 行撑破；② 行溢出时 `.aj-shell__right { flex: 0 1 auto }` 被压缩到 `--aj-right-w` 以下；③ 内层 `.aj-right` 宽度固定 `--aj-right-w` + `align-self: flex-end` 右缘锚定，左侧溢出被 `overflow: hidden` 裁剪、又因中栏 `position: relative` 绘制在其上 → 文字被中栏盖住（遮挡量随拖拽宽度线性增长）。修复：中栏 `flex: 1 1 0`（基准 0，只取剩余空间）+ 右栏 `flex: 0 0 auto`（拖拽宽度不可压缩，flex-basis 仍 `auto`，v0.16.5 折叠揭示动画零回流不变）+ CSS 契约守卫 1 测；实机 Chromium 1024/1100/1280/1440 × 中栏 1600px 超宽内容拖拽 8 帧，遮挡帧 6 → 0；折叠/展开 16 帧内层宽度恒定；web 610 → 611 tests（52 files）；工作区 0.24.1 → 0.24.2。 v0.24.1 验收修复（BUG-016，bugfix，无协议变更）：1240–1280px 视口下收缩态发送按钮 X 位移 370–400px → 修复为「工具栏折行时按钮保持右缘锚定」（`.aj-button--send` `margin-left: auto` + CSS 契约守卫）；实机 Playwright 回归 dx/dy=0 @1240/1260/1280/1290 + 390px 窄视口无溢出；web 609 → 610 tests（52 files）；工作区 0.24.0 → 0.24.1（db7833a）。 v0.24.0 中栏 Composer 滚动自动收缩（纯前端，无协议变更，PROTOCOL_VERSION 保持 0.15.0）：上滚离开底部（距底 >48px）自动平滑收缩为发送按钮原位「展开输入框」按钮（`data-collapsed` + `ChevronsUp` 图标 + `aria-label`/`aria-expanded`/`aria-controls` 同步；按钮 X/Y 零位移），下滚回底（<40px）或点击按钮平滑展开（点击路径聚焦 textarea，`preventScroll`；自动展开不抢焦点）；草稿全程保留；待答提问不收缩且强制展开；收缩期冻结 `--aj-composer-h`（滚动几何稳定，无 scrollTop 钳制/振荡）；程序化流式跟随（`scrollIntoView` 自标记）不触发收缩；260ms 动画锁 + 8px 迟滞带（48/40）；收缩同时关闭 composer 弹层；reduced-motion 复用既有双层机制。实现四件套：`composerCollapse.ts` 纯状态机（7 测）+ `ConversationFeed` 用户意图双判（wheel/touchmove + 非程序化 scroll 方向）与程序化排除（5 测）+ `RunPanel` 状态接线（10 测）+ `composer.css` 分段收拢契约（4 测）+ 评审修复波 3 测（真实浏览器点击展开焦点 / 展开期 --aj-composer-h 保持 / 锁过期回底重判）。门禁全绿：cargo fmt / 660 Rust tests / clippy 零警告 / drift 2 passed / sdk 54 / web 609 tests（52 files）/ tui 146 tests（19 files）/ web build / check-version 0.24.0 / desktop-ui；工作区 0.23.1 → 0.24.0。 v0.23.1 补齐 v0.23.0 计划遗漏（Web changes 状态拆分 + 右栏分组接线 + 缺失测试补齐 + 死代码清理，bugfix，无协议变更）+ v0.23.0 右边栏运行详情（run_id 维度 + FileChangeSource + total_bytes / PROTOCOL 0.15.0）+ v0.15.1 协议扩展（三栏工作台真实数据源）+ v0.15.2 session/status dispatch 修复 + v0.16.0 Web 工作台体验修复 + v0.16.1 跟随修复 + v0.16.2 远程访问修复 + v0.16.3 Web 工作台修复（项目菜单 + 文件树）+ v0.16.4 Web 工作台体验修复（状态灯 hover 面板 + 右栏手风琴 + 滚动条）+ v0.16.5 Web 工作台四项修复（右栏右→左展开动画 + 任务/文件变更高度可调 + 顶栏菜单重构/设置弹窗 + 文件树图标）+ v0.17.0 Web 六项增强（代码卡片 + 显示设置 + 文件树图标 + 会话列表菜单 + 项目编辑 + 设置项目面板）+ v0.17.1 验收修复（BUG-008 窄视口菜单/飞出层夹取）+ v0.17.2 界面修复（显示设置字号 px 滑杆/数字输入 + 弹窗边框/四角缩放）+ v0.18.0 Web 三项增强（composer 75% 居中浮动 + 权限选择自动允许 + 会话 token 总量）+ v0.18.1 token 统计修复（读出移至 header 模型 chip 右侧 + 输入/输出 拆分）+ v0.19.0 内核托管 Web UI + 文件树状态补强（同端口静态托管 + 混合鉴权登录门 + WS cookie 通道 + 文件树目录聚合状态）+ v0.20.0 Web 加固 + TUI Files 视图（四项，无 wire 变更）+ v0.20.1 验收修复（BUG-009 焦点模型 + BUG-010 提示词）+ v0.21.0 限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（四项，无 wire 变更）+ v0.21.1 验收修复（BUG-011 视图切换焦点 + BUG-012 树刷新焦点）+ v0.22.0 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口（四项，三处加性 wire，PROTOCOL_VERSION 0.13.0 → 0.14.0）
> `file/list` 沙箱化目录懒加载 + git 状态点；`$/progress` 新增 `kind:"todo"`
> （全量快照 + 会话内单调 `revision`）与 `kind:"file_change"`（write 自研行级
> diff / bash git 前后采样，unified diff ≤64 KiB）；`$/askQuestion` +
> `question/respond` owner 定向提问往返（幂等 + 超时/取消/断开按 skip 结算）；
> 会话级 `project_root`（路径基准五处同源）+ 新错误码 `-32807 PathNotAllowed`
> + `error.data`；`session/history` 回放重建 TODO/变更/Q&A；新 crate
> `agentj-question`（17 crates）；21 方法 / 52 schemas / PROTOCOL_VERSION 0.13.0；
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
> 最小尺寸 560×200（滑杆仍 200px 可用）、hover 手柄反馈。复核修复波（Oracle 评审 6 项）：
> `resizeRect` per-dialog 下限以视口上限夹取 + 增长按锚定边可用空间夹取（原点零漂移）、
> 新增 `clampDialogRect` + window resize 重夹取、非主键/pointer capture 守卫、
> 旧枚举 `hasOwnProperty` 校验、手柄外移避让滚动条。web 417 → 446 tests（44 files）/
> sdk 46 / Rust 564 tests；clippy 零警告 / drift 2 passed / web build / tui 71 /
> check-version 0.17.2 / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.1 → 0.17.2。
> v0.18.0 Web 三项增强（纯前端，无协议变更，用户直报）：(1) composer 75% 居中浮动 ——
> `.aj-composer-card` `position:absolute; bottom:16px; width:75%; margin:0 auto;
> max-height:calc(100% - 32px)`（layout.css 拆分全宽规则，特异性 (0,2,0) 承接 75%），
> `--aj-composer-h`（RunPanel `useLayoutEffect` + `ResizeObserver` `borderBoxSize`
> 全盒高）驱动 `.aj-conv` `padding-bottom` 与 `scroll-padding-bottom`（= 高度 + 32px）：
> 内容延伸至卡片底部（滚动穿透浮层）、滚动到底时末条消息停在输入框顶上方 ~16px；
> 窄视口 `min-width: min(340px, calc(100% - 16px))` + `.comp-tools` `flex-wrap: wrap`
> 两行兜底。(2) 权限选择器（逐次确认/自动允许）：`ComposerPrefs.permissionMode` +
> `agentj.composer.permissionMode`（readEnum 校验 + 跨标签 `storage` 同步 + bypass
> accent 边框提示）；bypass 时 `useApproval` 对当前会话 `$/permissionRequest` 自动
> `permission/respond allow`（不再弹窗；内核配置 deny / 路径越界仍强制拒绝；纯客户端
> 能力，协议未变）。(3) 会话 token 总量：`RunPanel` 聚合 usage entries（历史回放 +
> 实时逐轮恰好一条）→ `ComposerOptions` 模型触发器右侧渲染 `Σ 5.2k`
> （`formatTokenTotal`：k/M 一位小数、`.0` 修剪、999.95k 起进位 M；title 展示输入/输出
> 明细；无用量隐藏）。
> 实机 Playwright 验证：75.0% 居中（左右偏差 0）/ 浮底 16px / 内容穿透 / 滚底对齐
> （末条距输入框顶 16px + 8px flex gap）/ 权限选择器交互 + 跨标签双向同步 / 390px
> 两行工具栏无溢出；Oracle 复核 PASS（窄屏溢出 major 已修 + 6 minor/nit 修复波：
> 跨标签同步 / `console.debug` / token 边界 / `useLayoutEffect` / 命名 / docstring +
> 2 补测）。web 446 → 468 tests（45 files）/ sdk 46 / Rust 564 tests / tui 71 /
> clippy 零警告 / drift 2 passed / web build / check-version 0.18.0 / desktop-ui；
> PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.2 → 0.18.0。
> v0.18.1 token 统计修复（纯前端，无协议变更，用户直报）：(1) `Σ` 会话 token 读出从
> composer 底栏（模型选项旁）移至输入框右上角「deepseek-chat」模型 chip 右侧 ——
> `RunPanel` 新增 `.aj-composer__header-meta`（chip + 读出右对齐 + 内部换行兜底，
> header `flex-wrap`），`ComposerOptions` 移除 `usageTotal` prop 与 `.comp-usage`
> （options.css 同步移除）；(2) 统计拆分输入/输出：`Σ 2.5k · 输入 2k · 输出 500`
> （新 `tokenUsage.ts`：`formatTokenTotal` 迁出 + `formatUsageSummary` 摘要 +
> `formatUsageTitle` 精确 tooltip/aria-label）。实机 Playwright 15/15（位置/chip 右侧/
> 拆分/精确值/390px 读出换行无溢出/无控制台错误）+ Oracle 复核 PASS（a11y aria-label
> 明细 / meta 内部换行 / total 去重 / 4 补测）。web 468 → 479 tests（46 files）/
> sdk 46 / Rust 564 tests / tui 71 / clippy 零警告 / drift 2 passed / web build /
> check-version 0.18.1 / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.18.0 → 0.18.1。
> v0.19.0 内核托管 Web UI + 文件树状态补强（无 wire 变更，PROTOCOL_VERSION 保持 0.13.0）：
> (1) 内核同端口静态托管——`agentj-ipc` 新 `web` 模块（`WebConfig` + `build_router`）在
> `ws_router` 上 `nest_service("/assets", ServeDir)`（`Cache-Control: public,
> max-age=31536000, immutable`）+ `fallback_service(ServeDir::fallback(ServeFile(index.html)))`
> （根 `no-cache`、未知页面 200；`/ws`、`/login`、`/logout` 显式路由优先且不在门后，
> `Router::layer` 会波及 `/ws` 故两服务各自包裹）；(2) 混合鉴权登录门——回环 peer /
> 未配置 token 直出，非回环 + token 且无有效 cookie → `302 /login?next=<同源相对路径>`
> （`safe_next` 只放行 `/` 开头且非 `//`，防开放重定向；错拼 `/assets/*` 仍 404）；
> 内嵌登录页（无 JS）POST 校验后颁发不透明会话（`uuid`，内存 `RwLock<HashSet>`，重启即失效）
> 经 `Set-Cookie: agentj_session=<id>; HttpOnly; SameSite=Lax; Path=/`（会话 cookie 无
> Max-Age），`/logout` 撤销 + `Max-Age=0`；(3) WS 第三鉴权通道——有效会话 cookie +
> 同源 `Origin`（`origin_matches_host` 去 scheme 比 Host）免 `?token=`/`initialize.token`；
> `serve_connection` 的 token 参数已属遗留，服务端为 cookie 通道装配 initialize 参数时
> 注入 server token（`WsConnection::with_init_token` + `supply_initialize_token`）；
> (4) CLI `serve`/`dashboard --web-dist <PATH>` 四级发现（显式 flag/env 无效即停并告警 →
> cwd `agentj-ui/apps/web/dist` → exe 相对两档）+ 启动打印 `http://<host>:<port>/`；
> (5) 文件树目录聚合——`file/list` 目录 `git_status` 取后代最高优先级字母
> （`U>D>M>A>Unknown`，前缀 `"{dir}/"` 严格匹配，`srcx/` 不误伤 `src/`），Web `FileTree`
> 目录行 `StatusCell` 渲染状态字母、移除冗余圆点。测试补充：agentj-ipc `web` 单测
> （静态/缓存头/SPA 落回/登录门/cookie 放行/伪造 cookie/dist 缺失/无 token 直出）+
> `e2e_http_static`（断言 `/assets/*` 状态码并**持有 stdout reader** 防子进程 stdout 提前关闭）。
> cargo fmt / **591 Rust tests** / clippy 零警告 / drift 2 passed / sdk 46 /
> **web 480 tests（46 files）** / web build / tui 71 / check-version **0.19.0** / desktop-ui。
> PROTOCOL_VERSION 保持 0.13.0；工作区 0.18.1 → 0.19.0。
> v0.20.0 Web 加固 + TUI Files 视图（四项，无 wire 变更，PROTOCOL_VERSION 保持 0.13.0）：
> (1) `serve_connection` 遗留 token 参数清理（`TcpIpcServer` 死 token 字段一并移除；
> `WsServerState.token`/`WsQuery.token`/`?token=`/`initialize.token` 语义不变）；
> (2) 登录限流——新 `agentj-ipc` `login_limiter` 模块（按来源 IP 计数，第 5 次失败起锁定
> 30s、×2、封顶 900s；`429 + Retry-After`（秒，向上取整 ≥1）；锁定期不校验 token 且新尝试
> 升级锁定；成功清零；idle_ttl 1800s 懒淘汰；max_tracked 1024 淘汰；回环同样计数）+
> `/login` POST 接线（`auth_routes` 透传 peer）；(3) 版本探测与 SRI——`apps/web/build/
> version-sri.ts` + `version-sri-plugin.ts`（`writeBundle` 生成 `dist/version.json`
> （product/protocol/build_time，单一真源 = sdk config）+ 入口 HTML 同源 script/stylesheet/
> modulepreload 注入 `sha384` `integrity`，本地引用缺失即构建失败；零新依赖）；内核
> `WebConfig` 读取 `DistVersion` + `startup_warnings()`（缺失/不可解析/不一致三类启动警告，
> 不阻断服务）+ 未鉴权 `GET /version`（200 JSON + `Cache-Control: no-store`，显式路由优先于
> SPA fallback）+ `TcpIpcServer::web_config()`；CLI `serve` 启动打印 dist 版本警告 +
> `e2e_http_static` 覆盖；Web `ConnectionStatus` 版本行「版本 UI x · 内核 y」（不一致
> `data-state="warn"` + `⚠` + warning 色）、`App` 消费 `initialize.server_info.version`、
> SDK barrel 导出 `VERSION`；(4) TUI Files 视图——`files.ts` 纯函数（flatten/format/color/
> updateNode 等）+ `files.tsx` `FilesView` + `useFileTree`（懒加载 `file/list` depth=1、
> stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取、不持久化）+
> `sidebar.tsx` Threads/Files 切换（`f`）+ `app.tsx` 键位（`f` 切换 / `j,k,↑,↓` 移动 /
> `Enter,l,→` 目录开合 / `h,←` 收起或跳父级 / `r` 刷新）+ mock kernel `file/list` + e2e；
> 目录聚合状态字母色义 M→warning、A→success、U→textCaption、D→error。Oracle 全量复核
> PASS（无 must-fix）+ 修复波（TUI stale-reply 守卫 + 父级跳转 clamp + Web warn 色 CSS
> 契约测试）。cargo fmt / **604 Rust tests** / clippy 零警告 / drift 2 passed / sdk **47** /
> **web 491 tests（47 files）** / web build / tui **85 tests（17 files）** / check-version
> **0.20.0** / desktop-ui；无第三方新增依赖；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.19.0 → 0.20.0。
> v0.20.1 验收修复（bugfix，无协议变更）：实机/HTTP 验收 v0.20.0 四项 PASS（登录限流
> 第 5 次失败 429 + Retry-After 30s、锁定升级、到期 302 + cookie、成功清零、开放重定向
> 守卫；SRI sha384 复算匹配 + 缺失/不一致启动警告 + `GET /version` 未鉴权 no-store 且
> 不走 SPA fallback；`?token=` 403/101 + cookie+Origin 通道 + 回环直通 + `serve_connection`
> 签名清理；TUI Files 视图）；发现并修复 2 bugs——**BUG-009（high）** 新增全局 `f`
> 快捷键劫持 composer 输入 → `composer ⇄ sidebar` 焦点模型（Tab 切换焦点、单字母快捷键
> 仅 sidebar 焦点生效，`5eea70e`）；**BUG-010（low）** sidebar 提示词在焦点模型下宣传
> 失效键位（`12ec55a`）；均回归 verified、零新增 bug。cargo fmt / 604 Rust tests /
> clippy 零警告 / drift 2 passed / sdk 47 / web 491（47 files）/ web build /
> tui **88 tests（17 files）** / check-version **0.20.1** / desktop-ui；PROTOCOL_VERSION
> 保持 0.13.0；工作区 0.20.0 → 0.20.1。
> v0.21.0 限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（四项，无 wire 变更，
> PROTOCOL_VERSION 保持 0.13.0）：(1) 登录限流可观测性——`agentj-ipc` `login_limiter`
> 在状态变化点发 tracing 事件（锁定新触发 `warn`、成功登录 `info`、逐次失败/锁定期拒绝
> `debug`），`AtomicU64` 聚合计数经 `GET /version` 加性 `login_limiter` 块暴露
> （`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，无 per-IP 明细），
> CLI 日志订阅器改 `EnvFilter`（`RUST_LOG` 可配，未设默认 `info`，行为与 v0.20.0 逐字一致）；
> (2) 资产完整性——构建期 `apps/web/build/version-sri*` 生成 `dist/asset-manifest.json`
> （`sha384` 覆盖全部 Rollup bundle 产物含动态 chunk + SRI 注入后的 `index.html`，单遍哈希
> 与 SRI `integrity` 逐字节一致；排除 `version.json`/自身/`public/` 透传），内核 `WebConfig`
> 启动逐文件校验并并入 `startup_warnings()`（缺失/不可解析/不符/文件缺失，警告不阻断服务），
> 摘要进 `GET /version` 加性 `integrity` 块（`manifest_present`/`checked`/`mismatched`/`missing`）；
> (3) TUI Files 搜索——`s` 进入搜索（`file/list {path:null, depth:8}` 单请求扁平索引 ≤2000 +
> 客户端过滤），可打印字符过滤（含 `f`/`n`，吞键）、`Esc` 非空清空/空退出、`Enter` 跳转并展开
> 祖先、`r` 重建、`truncated` 显示 `· partial`，展开状态经 `filetree.json` 按本地项目键持久化
> （per-key ≤100、恢复 ≤50、失效路径静默跳过、陈旧键清理）；(4) Web 焦点作用域——新
> `useShortcutScope`（焦点在侧栏内、目标非可编辑、非 IME、无修饰键、无模态/菜单才生效）提供
> `f`/`n`/`r`/`j`/`k`/`Esc`，文件树/会话列表 roving tabindex 键盘导航，`ModalDialog` 开关焦点
> 恢复，**Tab 保持原生**且 composer 输入零误触。cargo fmt / 622 Rust tests / clippy 零警告 /
> drift 2 passed / sdk 47 / web **537 tests（50 files）** / tui **130 tests（19 files）** /
> web build / check-version **0.21.0** / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；
> 工作区 0.20.1 → 0.21.0。
> v0.21.1 验收修复（bugfix，无协议变更）：实机验收（Playwright chromium headless / HTTP /
> TUI）PASS；发现并修复 2 bugs——**BUG-011（low）** `f` 切换侧栏视图后焦点掉出侧栏（`<body>`，
> 后续 `j`/`k`/`r`/`Esc` 失效直至重新点击/Tab）→ `SidebarTabs` 活动 tab 按钮加 `data-tab`、
> `UnifiedSidebar.toggleTab` 切换后把焦点 park 到始终挂载的活动 tab 按钮（`89396f3`）；
> **BUG-012（low）** 焦点在文件树行（`j`/`k` 移动后）按 `r` 刷新 → 树卸载重挂、焦点掉出侧栏
> （BUG-011 同类、`r` 路径未覆盖）→ `r` handler 在 `refresh()` 前把焦点 park 到 files tab 按钮
> （`71245d5`）；均回归 verified、零新增 Bug；web 537 → **539 tests（50 files）**；
> PROTOCOL_VERSION 保持 0.13.0；工作区 0.21.0 → 0.21.1。
> v0.22.0 限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口（四项，三处
> 加性 wire，PROTOCOL_VERSION 0.13.0 → 0.14.0）：① `agentj-ipc` `login_limiter` 新增只读
> `snapshot()` 投影（`ip`/`failures`/`locked`/`retry_after`/`last_seen`，排序 `locked desc →
> failures desc → ip asc`）；`web` 新增 `auth_required()` + 显式路由 `GET /metrics`
> （Prometheus 文本 `text/plain; version=0.0.4` + `Cache-Control: no-store`：聚合 counters/gauges
> + per-IP series Top-50 截断 + `agentj_login_ip_series_truncated` 指示 + `integrity` 摘要；
> 回环/未配 token/有效 cookie 直出，非回环 + token 无 cookie → `401` 不重定向）；`/version`
> 既有契约零变化。② 构建清单覆盖 dist 实目录全部部署文件（含 `public/` 透传，bundle 键优先，
> 排除 `version.json`/自身/隐藏文件）；`TcpIpcServer` 启动后 60s / 8 文件游标轮转后台抽检
> （`SpotCheckConfig` 可注入），不符/缺失 `warn!` 按路径去重；`/version` `integrity` 加性嵌套
> `runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`。③ `file/list` 加性参数
> `query`（`path` 子树、忽略 `depth`、大小写不敏感子串、访问预算 `SEARCH_VISIT_LIMIT=20000`）
> 与结果 `total`（预算受限为下界；rank 名称前缀 > 名称子串 > 路径子串、Top-100）；TUI 搜索改
> 防抖 150ms 服务端查询（旧内核无 `total` → 自动降级 depth-8 本地过滤）；展开态键改为会话
> 有效 `project_root`（`session/new`/`session/status` 加性 `project_root` 捕获，未知回退
> `__default__`）。④ Web 共享 `useRovingFocus`（`wrap`/Home-End）收口 TopbarMenu/
> SessionListMenu/SidebarTabs/SettingsDialog（ComposerOptions listbox 保留独立模式）+
> 侧栏底部快捷键提示行（三态文案组件；Web 当前两态可达，搜索态待 Web 搜索特性；
> 单一常量源、`aria-hidden`）。门禁全绿：cargo fmt / **646 Rust tests** / clippy 零警告 /
> drift 2 passed / sdk 47 / **web 552 tests（51 files）** / **tui 142 tests（19 files）** /
> web build / check-version **0.22.0** / desktop-ui；PROTOCOL_VERSION 0.15.0；
> v0.23.0 右边栏运行详情 — run_id 维度 + FileChangeSource + total_bytes（七处加性 wire，PROTOCOL_VERSION 0.15.0 → 0.15.0）：① 协议：FileChange {run_id/seq/source/total_bytes}（FileChangeSource=write|bash+Unknown，total_bytes 仅 diff_truncated:true 时）+ 外层 ProgressValue::Todo/FileChange {run_id}（路由提示）+ TrajectoryValue::Todo/FileChange {run_id} + SessionHistoryParams {run_id?}（非法 → -32602 + data{reason:"invalid_run_id"}；先过滤 Todo/FileChange 再 cursor/limit 分页，其余 kind 全量）+ SessionRunResult {run_id?}（去重键 path → (run_id,path)，revision 按 run 单调）；② 内核：TurnRunner per-run UUID（Uuid::new_v4）+ todo_revisions/file_change_seqs: HashMap<String,u64>（UUID 键全局唯一）+ Done {run_id} + FileChangeArtifact {run_id/seq/source/total_bytes}（write_change 截断 64 KiB 前缀 + total_bytes，bash via git_sample source=bash）；③ IPC：broadcast sanitize_run_id（非法 → None 不报错）+ file_change_into_wire 透传 run_id/seq/source/total_bytes + trajectory 落盘 run_id + dispatch_session_history run_id 过滤 + SessionRunResult.run_id 回显；④ SDK：parseProgress 容忍非法 run_id/未知 source/旧数据缺字段，config VERSION 0.23.0/PROTOCOL 0.15.0；⑤ Web：todoState 按 (run_id,revision) 丢旧帧 + byRun Map + changes 按 (run_id,path) upsert + sortBySeq + RightPanel 按 run 分组（当前置顶 / 8-char short id / 未分组（历史））+ ChangeList source 徽标 + 截断 hint 已截断（原始 N KiB）+ 历史抽屉按 seq；App 捕获 SessionRunResult.run_id + session/history {run_id} 过滤 + FileTree 跳转高亮；⑥ TUI：TodoBlock 兼容 run_id（新建 todo.ts 委托，per-run when runId else legacy）；⑦ 文档：docs/specs/ipc-protocol.md 同步 0.15.0 + 52 schemas + §7.3/8.7/8.19/9.1 更新。cargo fmt / 121 agentj-core / 85 protocol / 167 ipc / sdk 47 / web 552 / tui 142 / drift 52 schemas / check-version 0.23.0/0.15.0 / web build / desktop-ui 全绿；工作区 0.22.0 → 0.23.0。
> v0.23.1 补齐 v0.23.0 计划遗漏（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）：(a) Web `changes.ts` 归约状态拆分为 `ChangesState {summary, history}`——summary 按 `(run_id, path)` upsert、history 按设计决策 #3/#15 append-only（按 seq 排序 + 按 `tool_call_id` 分组），历史抽屉可达；(b) `ChangeList.tsx` 消费 `history` prop、`RightPanel.tsx` run 分组经 `App.tsx` 传入 `groups`/`history`/`highlightedPath`/`onSelectRun` 真正接线（run 组头可点击、每组独立空态，无分组时保留 children/手风琴回退）；(c) `rightpanel.css` 补齐 `run-group*`/`history-drawer`/`hist-*`/`trunc-hint`/`src-badge*` token 化样式（此前裸节点无样式）；(d) 补齐计划缺失测试：protocol T0（+5）/ SDK T2（+7）/ IPC T5（+4）/ TUI T9（+4）/ web（+21）；(e) 删除 `agentj-core` 死代码 `bash_file_changes` helper 及单测（−1），`git_sample` 测试改断言生产路径 `source == Some(Bash)`；(f) 用户直报修复：右栏空态文案 `待协议支持` → `暂无任务`/`暂无变更`（TodoPanel/ChangeList），`handleSelectRun` 改用 `todoState.focusRun` 保留非枚举 `byRun`（修复点击 run 组头后分组丢失回退旧空态）；web +2 回归测。门禁全绿：cargo fmt / **660 Rust tests** / clippy 零警告 / drift 2 passed / sdk **54** / web **580 tests（51 files）** / tui **146 tests（19 files）** / web build（dist/version.json 0.23.1 / 0.15.0）/ check-version **0.23.1** / desktop-ui；PROTOCOL_VERSION 保持 0.15.0；工作区 0.23.0 → 0.23.1。
> (17 crates + agentj-ui monorepo：Web / TUI / Desktop + SDK + tokens，
> npm workspaces `apps/*`+`packages/*`，共享 SDK + protocol + tokens，
> WebSocket 网关同端口复用，后端零改造，frontend stays in-repo)

## Project Status

AgentJ — AI 编码代理工具（Rust），Linux 内核式「内核调度 + 可插拔模块」架构。
v0.24.2 已落地：**右栏遮挡修复（bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）**（用户直报「拖拽右边栏运行详情展宽时文字逐渐被中栏右侧遮挡」→ 三层根因：`.aj-shell__center { flex: 1 1 auto }` 基准取内容宽度，长代码块/宽工具卡撑破 flex 行；行溢出时 `.aj-shell__right { flex: 0 1 auto }` 被压缩到 `--aj-right-w` 以下；内层 `.aj-right` 宽度固定 `--aj-right-w` + `align-self: flex-end` 右缘锚定，左侧溢出被 `overflow: hidden` 裁剪且因中栏 `position: relative` 绘制其上 → 文字被中栏盖住，遮挡量随拖拽宽度线性增长。修复：中栏 `flex: 1 1 0`（基准 0，只取剩余空间）+ 右栏 `flex: 0 0 auto`（拖拽宽度权威，flex-basis 仍 `auto`，v0.16.5 折叠揭示动画零回流不变）+ CSS 契约守卫 1 测；实机 Chromium 1024/1100/1280/1440 × 中栏 1600px 超宽内容拖拽 8 帧：遮挡帧 6 → 0；折叠/展开 16 帧内层宽度恒定；web 610 → 611 tests（52 files）；工作区 0.24.1 → 0.24.2）。
v0.24.1 已落地：**验收修复（BUG-016，bugfix，无协议变更）**（1240–1280px 视口收缩态发送按钮 X 位移修复：`.aj-button--send` `margin-left: auto` 右缘锚定；实机 Playwright 回归 dx/dy=0；web 610 tests（52 files）；工作区 0.24.0 → 0.24.1）。
v0.24.0 已落地：**中栏 Composer 滚动自动收缩（纯前端，无协议变更，PROTOCOL_VERSION 保持 0.15.0）**（上滚离开底部（>48px）自动平滑收缩为发送按钮原位「展开输入框」按钮（X/Y 零位移 + aria 同步），下滚回底（<40px）或点击平滑展开（点击聚焦 textarea、自动展开不抢焦点）；草稿保留、待答提问强制展开、收缩期冻结 `--aj-composer-h`、程序化跟随不收缩、260ms 锁 + 8px 迟滞；`composerCollapse.ts` 纯状态机 + `ConversationFeed` 意图双判 + `RunPanel` 接线 + `composer.css` 分段收拢；web 580 → 609 tests（52 files）（含评审修复波 3 测：真实浏览器点击展开焦点 / 展开期 --aj-composer-h 保持 / 锁过期回底重判）。门禁全绿：cargo fmt / 660 Rust tests / clippy 零警告 / drift 2 passed / sdk 54 / web 609 tests（52 files）/ tui 146 tests（19 files）/ web build / check-version 0.24.0 / desktop-ui；工作区 0.23.1 → 0.24.0）。
v0.23.1 已落地：**补齐 v0.23.0 计划遗漏（Web changes 状态拆分 + 右栏分组接线 + 缺失测试补齐 + 死代码清理，bugfix，无协议变更，PROTOCOL_VERSION 保持 0.15.0）**
（(a) Web `changes.ts` 归约状态拆分 `ChangesState {summary, history}`——summary 按 `(run_id, path)` upsert、history 按设计决策 #3/#15 append-only（按 seq 排序 + 按 `tool_call_id` 分组），`ChangeList` 历史抽屉可达；(b) `ChangeList.tsx` 消费 `history` prop、`RightPanel.tsx` run 分组经 `App.tsx` 传入 `groups`/`history`/`highlightedPath`/`onSelectRun` 真正接线（run 组头可点击、每组独立空态，无分组时保留 children/手风琴回退）；(c) `rightpanel.css` 补齐 `run-group*`/`history-drawer`/`hist-*`/`trunc-hint`/`src-badge*` token 化样式；(d) 补齐计划缺失测试 protocol T0（+5）/ SDK T2（+7）/ IPC T5（+4）/ TUI T9（+4）/ web（+21）；(e) 删除 `agentj-core` 死代码 `bash_file_changes` 及单测（−1），`git_sample` 测试改断言生产路径 `source == Some(Bash)`；(f) 用户直报修复：右栏空态文案 `待协议支持` → `暂无任务`/`暂无变更`（TodoPanel/ChangeList），`handleSelectRun` 改用 `todoState.focusRun` 保留非枚举 `byRun`（修复点击 run 组头后分组丢失回退旧空态）；web +2 回归测。门禁全绿：cargo fmt / **660 Rust tests** / clippy 零警告 / drift 2 passed / sdk **54** / web **580 tests（51 files）** / tui **146 tests（19 files）** / web build / check-version **0.23.1** / desktop-ui；PROTOCOL_VERSION 保持 0.15.0；工作区 0.23.0 → 0.23.1）。
v0.23.0 已落地：**右边栏运行详情 — run_id 维度 + FileChangeSource + total_bytes（七处加性 wire，PROTOCOL_VERSION 0.15.0 → 0.15.0）**
（① 协议：FileChange {run_id/seq/source/total_bytes}（FileChangeSource=write|bash+Unknown）+ ProgressValue::Todo/FileChange {run_id} + TrajectoryValue::Todo/FileChange {run_id} + SessionHistoryParams {run_id?}（非法→-32602）+ SessionRunResult {run_id?}；② 内核 TurnRunner per-run UUID + HashMap<RunId,u64> + artifact source/total_bytes；③ IPC sanitize + broadcast + history 过滤；④ SDK parseProgress 容忍 + 0.23.0/0.15.0；⑤ Web reducers (run_id,revision)/(run_id,path) + RightPanel 分组 + ChangeList 徽标/截断/抽屉 + App run_id 捕获与跳转；⑥ TUI todo.ts 兼容；⑦ docs/specs/ipc-protocol.md 0.15.0 + 52 schemas。门禁全绿见下表；工作区 0.22.0 → 0.23.0）。
v0.22.0 已落地：**限流指标导出 + 资产清单扩展/抽检 + TUI 服务端搜索 + Web 键位收口（四项，三处加性 wire）**
（① `agentj-ipc` `login_limiter` 只读 `snapshot()` + 鉴权门控 `GET /metrics`（Prometheus 文本：聚合计数 +
per-IP 序列 Top-50 截断 + `agentj_login_ip_series_truncated` + integrity 摘要；回环/未配 token/有效 cookie
直出，远程无 cookie → `401` 不重定向），`/version` 既有契约零变化；② `asset-manifest.json` 覆盖 dist 实目录
全部部署文件（含 `public/` 透传，bundle 键优先）+ `TcpIpcServer` 60s / 8 文件游标轮转抽检（不符 `warn!` 按路径
去重）+ `/version` `integrity.runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`；③ `file/list` 加性
`query`（`path` 子树、忽略 `depth`、访问预算 20000、rank 前缀 > 子串 > 路径、Top-100）与 `total` + TUI 防抖 150ms
服务端搜索（旧内核无 `total` 自动降级）+ 展开态键 = 会话有效 `project_root`（`session/new`/`session/status` 加性
`project_root` 捕获）；④ Web 共享 `useRovingFocus`（wrap/Home-End）收口 TopbarMenu/SessionListMenu/SidebarTabs/
SettingsDialog + 侧栏底部快捷键提示行（三态文案组件；Web 当前两态可达，搜索态待 Web 搜索特性）。
门禁全绿：cargo fmt / **646 Rust tests** / clippy 零警告 / drift 2 passed / sdk 47 /
web **552 tests（51 files）** / tui **142 tests（19 files）** / web build / check-version **0.22.0** /
desktop-ui；PROTOCOL_VERSION 0.15.0；工作区 0.21.1 → 0.22.0）。
v0.21.1 已落地：**验收修复（BUG-011/BUG-012，bugfix，无协议变更）**
（实机验收（Playwright chromium headless / HTTP / TUI）PASS；BUG-011（low）：`f` 切换侧栏视图
后焦点掉出侧栏（`<body>`，后续 `j`/`k`/`r`/`Esc` 失效直至重新点击/Tab）→ `SidebarTabs` 活动
tab 按钮加 `data-tab`、`UnifiedSidebar.toggleTab` 切换后把焦点 park 到始终挂载的活动 tab 按钮
（`89396f3`）；BUG-012（low）：焦点在文件树行（`j`/`k` 移动后）按 `r` 刷新 → 树卸载重挂、焦点
掉出侧栏（BUG-011 同类、`r` 路径未覆盖）→ `r` handler 在 `refresh()` 前把焦点 park 到 files
tab 按钮（`71245d5`）；均回归 verified、零新增 Bug。cargo fmt / 622 Rust tests / clippy 零警告 /
drift 2 passed / sdk 47 / web 537 → **539 tests（50 files）** / tui 130 tests（19 files） /
web build / check-version **0.21.1** / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；
工作区 0.21.0 → 0.21.1）。
v0.21.0 已落地：**限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域（四项，无 wire 变更）**
（① 登录限流可观测性——`agentj-ipc` `login_limiter` 在状态变化点发 tracing 事件（锁定**新触发**
`warn`、成功登录 `info`、逐次失败/锁定期拒绝 `debug`），`AtomicU64` 聚合计数经 `GET /version`
加性 `login_limiter` 块暴露（`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，
无 per-IP 明细），CLI 日志订阅器改 `EnvFilter`（`RUST_LOG` 可配，未设默认 `info`，行为与 v0.20.0
逐字一致），限流策略语义（429/Retry-After/锁定升级/清零）零变化；② 资产完整性——构建期
`apps/web/build/version-sri*` 生成 `dist/asset-manifest.json`（`sha384` 覆盖全部 Rollup bundle
产物含动态 chunk + SRI 注入后的 `index.html`，单遍哈希与 SRI `integrity` 逐字节一致；排除
`version.json`/自身/`public/` 透传），内核 `WebConfig` 启动逐文件校验并并入 `startup_warnings()`
（缺失/不可解析/不符/文件缺失，警告不阻断服务），摘要进 `GET /version` 加性 `integrity` 块
（`manifest_present`/`checked`/`mismatched`/`missing`）；③ TUI Files 搜索——`s` 进入搜索
（`file/list {path:null, depth:8}` 单请求扁平索引 ≤2000 + 客户端过滤），可打印字符过滤
（含 `f`/`n`，吞键）、`Esc` 非空清空/空退出、`Enter` 跳转并展开祖先、`r` 重建、`truncated`
显示 `· partial`，展开状态经新 `filetree.json` 按本地项目键持久化（per-key ≤100、恢复 ≤50、
失效路径静默跳过、陈旧键清理）；④ Web 焦点作用域——新 `useShortcutScope`（焦点在侧栏内、
目标非可编辑、非 IME、无修饰键、无模态/菜单才生效）提供 `f`/`n`/`r`/`j`/`k`/`Esc`，文件树/
会话列表 roving tabindex 键盘导航，`ModalDialog` 开关焦点恢复，**Tab 保持原生**且 composer
输入零误触。门禁全绿：cargo fmt / 622 Rust tests / clippy 零警告 / drift 2 passed / sdk 47 /
web 537 tests（50 files）/ tui 130 tests（19 files）/ web build / check-version 0.21.0 /
desktop-ui；PROTOCOL_VERSION 保持 0.13.0；工作区 0.20.1 → 0.21.0）。
v0.20.1 已落地：**验收修复（BUG-009/BUG-010，bugfix，无协议变更）**
（实机/HTTP 验收 v0.20.0 四项全过：登录限流（第 5 次失败 429 + Retry-After 30s、锁定
升级、到期 302 + cookie、成功清零、开放重定向守卫）/ 版本探测与 SRI（sha384 复算匹配、
缺失与不一致启动警告、`GET /version` 未鉴权 + no-store + 不走 SPA fallback）/
`serve_connection` 清理（`?token=` 403/101、cookie+Origin 通道、回环直通）/ TUI Files
视图。BUG-009（high）：新增全局 `f` 快捷键劫持 composer 输入 → 修复为
`composer ⇄ sidebar` 焦点模型（Tab 切换焦点、单字母快捷键仅 sidebar 焦点生效，
`5eea70e`）；BUG-010（low）：sidebar 提示词在焦点模型下宣传失效键位（`12ec55a`）；
均回归 verified、零新增 bug。cargo fmt / 604 Rust tests / clippy 零警告 / drift 2 passed /
sdk 47 / web 491（47 files）/ web build / tui **88 tests（17 files）** / check-version
**0.20.1** / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；工作区 0.20.0 → 0.20.1）。
v0.20.0 已落地：**Web 加固 + TUI Files 视图（四项，无 wire 变更）**
（(1) `serve_connection` 遗留 token 参数清理（`TcpIpcServer` 死 token 字段一并移除），
`WsServerState.token`/`WsQuery.token`/`?token=`/`initialize.token` 语义不变；
(2) 登录限流：新 `agentj-ipc` `login_limiter`（按来源 IP 计数，第 5 次失败起锁定 30s、
×2、封顶 900s；`429 + Retry-After`（秒，ceil ≥1）；锁定期不校验 token 且新尝试升级锁定；
成功清零；idle_ttl 1800s 懒淘汰；max_tracked 1024 淘汰；回环同样计数）+ `/login` POST
接线（`auth_routes` 透传 peer）；(3) 版本探测与 SRI：`apps/web/build/version-sri.ts` +
`version-sri-plugin.ts`（`writeBundle` 生成 `dist/version.json` + 入口 HTML 同源引用注入
`sha384` `integrity`，缺失即构建失败；零新依赖）+ 内核 `WebConfig` 读 `DistVersion` +
`startup_warnings()`（缺失/不可解析/不一致）+ 未鉴权 `GET /version`（200 JSON + `no-store`，
显式路由优先于 SPA fallback）+ `TcpIpcServer::web_config()` + CLI 启动警告 + Web
`ConnectionStatus` 版本行（UI/内核不一致 warn）+ SDK barrel 导出 `VERSION`；
(4) TUI Files 视图：`files.ts` 纯函数 + `FilesView` + `useFileTree`（懒加载 `file/list`
depth=1、stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取、
不持久化）+ Threads/Files 切换（`f`）+ 键位（j/k/Enter/h/r 等）+ mock kernel e2e。
Oracle 全量复核 PASS（无 must-fix）+ 修复波（TUI stale-reply 守卫 + 父级跳转 clamp +
Web warn 色 CSS 契约测试）。cargo fmt / **604 Rust tests** / clippy 零警告 / drift 2 passed /
sdk **47** / **web 491 tests（47 files）** / web build / tui **85 tests（17 files）** /
check-version **0.20.0** / desktop-ui；无第三方新增依赖；PROTOCOL_VERSION 保持 0.13.0；
工作区 0.19.0 → 0.20.0）。
v0.19.0 已落地：**内核托管 Web UI + 文件树状态补强（无 wire 变更）**
（(1) `agentj-ipc` 新 `web` 模块：同端口静态托管内置 Web UI（`/assets` immutable 长缓存 /
根 `no-cache` / SPA fallback 未知页面 200；`/ws` 与 `/login`/`/logout` 不在门后）+
混合鉴权登录门（回环或未配 token 直出；非回环 + token 无有效 cookie → `/login` 登录页 +
不透明 `agentj_session` cookie（`HttpOnly; SameSite=Lax`，内存态）/ `/logout` 撤销）+
WS cookie 第三通道（有效会话 + 同源 Origin 免 `initialize.token`）；
(2) CLI `serve`/`dashboard --web-dist <PATH>` 四级发现 + 启动打印 `http://<host>:<port>/`；
(3) `file/list` 目录 `git_status` 聚合后代最高优先级字母（`U>D>M>A`）+ Web `FileTree`
目录行状态字母、移除冗余圆点。cargo fmt / **591 Rust tests** / clippy 零警告 /
drift 2 passed / sdk 46 / **web 480 tests（46 files）** / web build / tui 71 /
check-version **0.19.0** / desktop-ui；PROTOCOL_VERSION 保持 0.13.0；工作区 0.18.1 → 0.19.0）。
v0.18.1 已落地：**token 统计位置与拆分修复（用户直报，纯前端无协议变更）**
（(1) `Σ` 会话 token 读出移至 composer header、紧邻「deepseek-chat」chip 右侧
（`RunPanel` `.aj-composer__header-meta`；`ComposerOptions` 移除 `usageTotal`/`.comp-usage`）；
(2) 拆分输入/输出：`Σ 2.5k · 输入 2k · 输出 500`（新 `tokenUsage.ts`
`formatUsageSummary` + `formatUsageTitle`，tooltip 与 aria-label 精确值 + 合计）；
实机 Playwright 15/15 + Oracle 复核 PASS + 修复波；web 468 → 479 tests（46 files）/
sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0；工作区 0.18.0 → 0.18.1）。
v0.18.0 已落地：**Web 三项增强（composer 浮动布局 + 权限选择 + token 总量，纯前端无协议变更）**
（(1) composer 75% 居中浮动 + `--aj-composer-h` 动态底部预留（内容延伸至卡片底部、
滚动到底末条消息停在输入框顶上方）；(2) 权限选择器（逐次确认/自动允许 + 客户端自动允许 +
跨标签同步，内核强制拒绝不受影响）；(3) 模型触发器右侧会话 token 总量 `Σ 5.2k`；
窄视口 min-width + 工具栏换行兜底；Oracle 复核 PASS + 修复波。web 446 → 468 tests（45 files）/
sdk 46 / Rust 564 tests / tui 71；PROTOCOL_VERSION 保持 0.13.0；工作区 0.17.2 → 0.18.0）。
v0.17.2 已落地：**界面修复（显示设置 px 滑杆 + 可缩放弹窗，无协议变更）**
（(1) 显示字号：`displayPrefsStore` 转像素数值 + 旧枚举迁移，`SettingsDisplayPanel`
每区一行滑杆/数字/px + 行高密度横排，消除弹窗纵向拉伸；WorkspaceShell 注入 px；
(2) 弹窗缩放：`ModalDialog` `.aj-dialog-frame` + 8 向手柄 + `dialogResize.ts`
几何（对边锚定/最小 320×200/视口夹取/可覆盖下限，设置 560px）；实机 Playwright 验证。
web 417 → 446 tests（44 files）/ sdk 46 / Rust 564 tests / tui 71；
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
TUI runview 只读 TODO 区块；协议 0.13.0（21 方法 / 52 schemas），
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
  （660 passed / 0 failed；含 bus 密封 + FileBackedStore/Config/CLI 单测
  + e2e_cli/e2e_permission/e2e_serve/e2e_ws/e2e_http_static 密封测试）
- `cargo clippy --workspace --all-targets -- -D warnings` — lint 门禁（零警告）
- `cargo bench -p agentj-model` — 模型流式基准（criterion）
- `cargo run -p agentj-protocol --bin gen_schemas` — 重新生成 `schemas/*.json`
- `cargo test -p agentj-protocol -- drift` — 漂移门禁（52 schemas 无漂移）
- `npm --prefix agentj-ui run generate --workspace=@agentj/protocol` — 生成 protocol 类型
  （`schemas/*.json` → `agentj-ui/packages/protocol/types.d.ts`）
- `npm --prefix agentj-ui run build --workspace=@agentj/protocol` — `tsc --noEmit` 校验
- `npm --prefix agentj-ui test --workspace=@agentj/sdk` — SDK 单测（client/router/transports/config，
  54 tests）
- `npm --prefix agentj-ui run generate --workspace=@agentj/sdk` — 生成 `METHODS` 常量
  （`protocol/methods.json` → `agentj-ui/packages/sdk/src/methods.ts`）
- `npm --prefix agentj-ui test --workspace=@agentj/web` — Web 单测（vitest，610 tests / 52 files）
- `npm --prefix agentj-ui run build --workspace=@agentj/web` — Web 构建（`tsc -b && vite build`）
  > 注意：Vite dev server 不监听 root 之外的 `packages/sdk` 源（alias 直连源码）；
  > 改动 SDK 后须重启 dev server，否则运行陈旧的 `METHODS`（v0.16.3「加载中」根因）。
  > Web 新依赖 `rehype-highlight`（hljs common 语言集，随 `npm --prefix agentj-ui install`
  > + lockfile 落地，v0.17.0）。
- `bun --cwd agentj-ui/apps/tui test` — TUI 单测（含 mock WS e2e，146 tests / 19 files）
- `./agentj-ui/apps/desktop/check-artifacts.sh linux` — Desktop 产物校验
- `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — Desktop 前端产物/配置一致性校验
- `./scripts/check-version.sh` — 版本一致性（Cargo + sdk VERSION + desktop 均为 0.24.2，
  PROTOCOL_VERSION 0.15.0 + `protocol/methods.json` 版本 0.15.0）

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
  `SessionNewParams.project_root`、`-32807`；v0.14 加性 `file/list` 参数 `query` / 结果 `total`
  与 `SessionNewResult`/`SessionStatusResult.project_root`（21 方法 / 52 schemas，
  PROTOCOL_VERSION 0.15.0）。
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
  + `file/list` dispatch（沙箱 + git 状态；目录聚合后代最高优先级字母，v0.19.0）
  + **`web` 模块（v0.19.0）**：`WebConfig` + `build_router`（`/ws` + `/login`/`/logout`
  + 静态 `ServeDir`/SPA fallback；两静态服务各自包裹 cache 层与登录门，避免
  `Router::layer` 波及 `/ws`）+ 不透明会话 cookie / Origin 同源 / `next` 白名单 +
  WS cookie 第三通道（`WsConnection::with_init_token` + `supply_initialize_token`
  为 cookie 通道注入 server token）
  + **v0.20.0 加固**：`login_limiter` 按来源 IP 失败计数/指数锁定（第 5 次起 30s、
  ×2、封顶 900s；`429 + Retry-After`；idle_ttl 1800s / max_tracked 1024 淘汰；
  回环同样计数）+ `DistVersion` 读取 + `startup_warnings()`（缺失/不可解析/不一致
  三类启动警告）+ 未鉴权 `GET /version`（200 JSON + `no-store`，显式路由优先于
  SPA fallback）+ `web_config()` 访问器；`serve_connection`/`TcpIpcServer` 遗留
  token 参数与死字段清理
  + question notifier/settle hook 接线
  + `error.data` 透传 + 非法请求回显可确定 id（`-32600`；语法错误 `-32700`+null，v0.16.3）
  + **v0.22.0**：`login_limiter::snapshot()` 只读投影（`ip`/`failures`/`locked`/`retry_after`/`last_seen`）
  + `auth_required()` + 鉴权门控 `GET /metrics`（Prometheus 文本；`/version` 契约零变化）
  + 清单解析结果保留与 60s / 8 文件运行期抽检（`SpotCheckConfig` + `RwLock<SpotCheckReport>`，
  结果经 `/version` `integrity.runtime` 与 `/metrics` 双面暴露）+ `file/list` query-aware 遍历
  （独立访问预算 20000 + rank + Top-100 + `total`）；
  v0.19.0 新增依赖 `tower-http`（fs/set-header）+ `uuid`（v4）；v0.20.0 / v0.22.0 无新依赖）。
- `agentj-cli`：`agentj` 二进制（run/serve/dashboard，模块注册进 Kernel，
  agentj-question 的 stdio 与 `serve --port` 两条装配路径；v0.19.0 `serve`/`dashboard`
  新增 `--web-dist <PATH>` 四级发现 + 启动打印 `http://<host>:<port>/` + 缺失告警；
  v0.20.0 经 `web_config().startup_warnings()` 启动打印 dist 版本警告，不阻断服务）。
- `agentj-plugin-fixture`（dev-only）：插件 cdylib 测试夹具。
- `schemas/*.json`：52 JSON Schema（schemars 生成，提交件 + drift 门禁）。
- `protocol/methods.json`：方法注册表（method ↔ schema/方向/类型；21 方法，protocol_version 0.15.0）。
- `docs/specs/ipc-protocol.md`：人类可读协议规范（11 节）。
- `agentj-ui/packages/protocol`：协议包（原 `bindings` 迁入，
  `schemas/*.json` → `types.d.ts`，`json-schema-to-typescript` + `tsc --noEmit`，
  为 drift 门禁后第一门）。
- `agentj-ui/packages/sdk`：共享 SDK（`src/client.ts` RpcClient +
  `src/router.ts` ResponseRouter/RpcTransport + `src/config.ts`
  `VERSION="0.24.2"` + `PROTOCOL_VERSION="0.15.0"` + `resolveWsUrl` + `src/methods.ts` METHODS
  + `transports/websocket.ts` WebSocketTransport
  + `transports/tcp.ts` TcpTransport；`RpcClient.request` 拦截空/未定义 method（v0.16.3）；
  v0.20.0 `index.ts` barrel 导出 `VERSION`（Web `ConnectionStatus` 消费），
  被 Web/TUI/Desktop 同源消费）。
- `agentj-ui/packages/tokens`：共享语义 tokens（`palette.ts` 24 keys × dark/light + `SEMANTIC_KEYS` + `LAYOUT` 常量块（left/right 宽度/splitter/topbar/gap），`tokens.css/tokens.json/docs/ui-tokens.md` 生成，radius 类 keys kebab→camel 迁移，三端约束）。
- `agentj-ui/apps/web`：Web 前端（Vite + React 19，消费 `@agentj/sdk` + `@agentj/tokens` + `lucide-react`；
  **三栏工作台（真实协议驱动）**：`WorkspaceShell`（Sessions/Files 左栏 `SidebarTabs` +
  会话中栏 + 运行详情右栏） + `SplitPane`（拖拽/键盘/双击） +
  `RightPanel`（`TodoPanel` ← `$/progress` todo 全量快照 + revision 门控；
  `ChangeList` ← `file_change` upsert + unified diff） +
  `FileTree`（`file/list` 懒加载 + git 状态字母；v0.19.0 目录行聚合后代最高优先级
  字母、移除冗余圆点） +
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
  **v0.18.0 三项增强**：composer 75% 居中浮动（`--aj-composer-h` 动态预留 +
  `scroll-padding-bottom` 滚底对齐 + 窄屏 min-width/换行兜底）/ 权限选择器
  （`逐次确认`/`自动允许` + 跨标签 storage 同步 + bypass 客户端自动允许）/
  会话 token 总量 `Σ` 读出（`formatTokenTotal`；v0.18.1 起移至 composer header
  模型 chip 右侧并拆分 输入/输出 —— `tokenUsage.ts` `formatUsageSummary`/
  `formatUsageTitle` + `.aj-composer__header-meta`）；
  **v0.20.0 版本/SRI**：`build/version-sri.ts` + `version-sri-plugin.ts`
  （`writeBundle` 生成 `dist/version.json`（单一真源 = sdk config）+ 入口 HTML
  同源 script/stylesheet/modulepreload 注入 `sha384` `integrity`，缺失即构建失败）+
  `ConnectionStatus` 版本行「版本 UI x · 内核 y」（`App` 消费
  `initialize.server_info.version`；不一致 `data-state="warn"` + `⚠`）；
  **v0.22.0 键位收口**：共享 `useRovingFocus`（`wrap`/Home-End）迁移
  TopbarMenu / SessionListMenu / SidebarTabs / SettingsDialog
  （ComposerOptions listbox 保留独立模式）+ 侧栏底部快捷键提示行
  （三态文案组件；Web 当前两态可达，搜索态待 Web 搜索特性）；
  `layoutStore` 宽度/面板持久化，
  `todoState`/`changes` reducer，`deriveChanges` 半推导退役；
  布局/会话/Composer/对话/轨迹/Hero/对话框拆 `components/*.css`，vitest + vite build；
  **v0.24.0 滚动收缩**：`composerCollapse.ts` 纯状态机（48/40 迟滞）+ `RunPanel`
  `data-collapsed`/双职责按钮/`--aj-composer-h` 冻结 + `composer.css` 分段收拢）。
- `agentj-ui/apps/tui`：TUI 前端（Bun + @opentui/react，全功能：
  connection/session/composer/runview/permission/palette/models/
  completion/theme/usage + runview 只读 TODO 区块（revision 门控） +
  mock WS e2e + sidebar 文本版 UnifiedSidebar 32char；`testRender` 快照；
  v0.20.0 Files 视图：`files.ts` 纯函数（flatten/format/color/updateNode）+
  `files.tsx` `FilesView` + `useFileTree`（懒加载 `file/list` depth=1、
  stale-reply 代际守卫、collapse 保留子树、refresh 根+已展开、会话切换清空重取、
  不持久化）+ Threads/Files 切换（`f`）+ 键位 `j,k,↑,↓`/`Enter,l,→`/`h,←`/`r`；
  v0.22.0 搜索改防抖 150ms 服务端查询（`file/list {path:null,depth:8,query}`，rank + `total`；
  旧内核无 `total` → 自动降级 depth-8 本地过滤）+ 展开态键 = 会话有效 `project_root`
  （`session/new`/`session/status` 捕获，未知回退 `__default__`））。
- `agentj-ui/apps/desktop`：Desktop 壳（Tauri v2，`frontendDist` →
    `../../web/dist`，与 workspace 版本同源 `0.24.2`，四件套打包）。
- `agentj-ui/` 根：`package.json` workspaces `["apps/*","packages/*"]`
  （frontend stays in-repo，前端不拆独立仓，与 Rust 共享 0.24.2）。
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
- **Doc placement** — `README.md` 只保留项目简介、高层特性概览、结构、快速上手、协议摘要与
  开发命令；**不记录详细版本变更与逐版本特性**。
  - 版本变更记录 → `docs/changelog/`（每个次版本一个文件 `v0.<minor>.md`，补丁号作为同文件
    小节；Keep a Changelog 风格 `Added`/`Changed`/`Fixed`/`Removed`；新次版本另建文件并更新
    `docs/changelog/README.md` 索引）。
  - 用户可见特性 → `docs/guide/`（按主题，去版本号叙事）。
  - `README.md` 的版本行只同步「当前版本 + 指向 `docs/changelog/` 的链接」。
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`).
- Before planning/developing, read both summaries — `.agents/summary/summary_and_next_se.md`
  (backend) and `.agents/summary/summary_and_next_fe.md` (frontend) — and `docs/superpowers/` first.
- All diagrams in spec docs under `docs/superpowers/specs/` must use UML or Mermaid.
- Update `docs/superpowers/specs` target spec docs TODO list after each phase.
- Summary files are split by side — frontend / backend isolated, never cross-write:
  backend → `.agents/summary/summary_and_next_se.md`（crates/ · schemas/ · protocol/）,
  frontend → `.agents/summary/summary_and_next_fe.md`（agentj-ui/）。Each file only retains
  that side's currently developing feature: progress, remaining issues, pending (待实施),
  next steps. Replace, don't append — EXCEPT the `## 待实施` table: entries not yet implemented
  are retained as 待实施 across phases and MUST NOT be deleted until actually implemented.
- After each plan completes, update the summary file(s) for the touched side(s) with the
  current feature's progress, remaining issues, next plan/next steps.
- After each feature phase completes AND `cargo test` + `clippy` pass, auto:
  - Append the phase's change record to the matching `docs/changelog/v0.<minor>.md`
    （新建次版本文件时更新 `docs/changelog/README.md` 索引）；用户特性写入 `docs/guide/`。
  - Update `AGENTS.md` / `README.md`（仅当前版本行）/ `.agents/summary/summary_and_next_se.md` /
    `.agents/summary/summary_and_next_fe.md`（按阶段触及侧更新，可两文件同更）
  - Commit as `docs: auto-sync AGENTS.md, README.md, summary (fe/se) after {phase}`.
  - Do NOT wait for user — mandatory post-phase step.
- Clean up temp files, Docker containers after each phase.
- When a command hangs >120s, cancel and retry.
- PR creation: create PR on `origin` only and share link — user handles merge.
  **Never open a GitCode MR for the same branch**（见下方 Remote & Merge Policy）。
- Rust function ≤50 lines, ≤120 chars per line.

## Remote & Merge Policy

> 起因：2026-09-30 发现 `origin/main`(870) 与 `gitcode/main`(872) 提交数不一致。
> 根因是**同一条 feature 分支在两个远端各被合并一次**，每个远端各自生成一个 merge
> commit，导致两条 `main` 历史累积的 merge commit 数量不同——而两侧 tree 完全相同
> （`5b5126b`，`git diff --stat origin/main gitcode/main` 为空）。**纯记账差异，非内容问题。**
> 已 double-merge 的分支：`fix/right-panel-occlusion`（GitHub #66 / GitCode !44）、
> `chore/agent-model-deepseek-v4-1-flash`（#67 / !45）、
> `docs/readme-changelog-guide-split`（#68 / !46）。

- **单一权威远端**：`origin`（GitHub `JeillZhang/agentj`）是 `main` 的**唯一真源**；
  `gitcode`（GitCode `jeill/agentj`）是**只读镜像**，不接受 MR。
- **只合并一次**：PR 只在 `origin` 创建与合并。合并后**不得**再在 `gitcode` 建同名 MR。
- **镜像用快进推送同步，禁止再次合并**：

  ```bash
  git pull --ff-only origin main
  git push gitcode main:main        # 复制 commit，不新增 merge commit
  ```

  若 `gitcode/main` 落后，这是快进；若已分叉，`push` 会被拒绝——**不要用 `--force`**。
- **feature 分支可推两个远端**（同一 SHA，无害）；**合并只能在一个远端进行**。
- **健康检查看 tree，不看提交数**：

  ```bash
  git fetch origin gitcode
  git diff --stat origin/main gitcode/main              # 期望为空
  git rev-parse origin/main^{tree} gitcode/main^{tree}   # 期望相等
  ```

  提交数不同但 tree 相同 = 旧的双合并遗留记账差异，**不是内容问题**；
  tree 不同才是真分歧，需立即排查。
- **分叉后不要自动 force-push 收敛**（改写已发布 `main` 属高风险操作）——
  报告给用户，由用户决定是否重置镜像。
