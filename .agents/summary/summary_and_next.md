# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台四项修复（v0.16.5）—— 右栏右→左展开动画 + 成对面板高度调节 + 顶栏菜单重构/设置弹窗 + 文件树图标**
> ——来源=用户反馈：① 右栏展开应右→左平滑（勿「瞬间展开后列表再向右扩展」）；
> ②「...」菜单仅保留 项目/继续会话/收藏会话/设置/帮助，排序/过滤/演示数据/连接设置
> 并入「设置」弹窗（左设置菜单 + 右面板；项目/收藏会话悬停自动向合适方向展开二级菜单）；
> ③ 任务/文件变更单开铺满页面底部、双开可自行调节高度并记录（刷新不丢）；
> ④ 文件树 icon 美化（目录 + 按分类的文件 icon）

## v0.16.5: Web 工作台四项修复 ✅（本次完成）

- ✅ **(1) 右栏折叠/展开单段右→左平滑动画（无协议变更）**：根因 = 外层
  `.aj-shell__right` 的 flex-basis/min/max 钳制不可过渡（瞬间到位），内层
  `.aj-right` width 过渡导致列表内容向右回流扩展（两段式）。修复 = 外层
  `flex: 0 1 auto; width: var(--aj-right-w); min-width: 0; max-width: none`
  + ≥1024 `transition: width`（折叠态仅 `width: 40px`，去除钳制）；内层
  `align-self: flex-end` 固定宽右缘锚定（内容零回流、仅揭示；collapsed 头部
  `justify-content: flex-end; padding-right: 4px` 使 « 落 40px 可视窗）；抽屉
  <1024 内层 `width: 100%`；border-left 移到外层；拖拽期间
  `.aj-splitter[data-side="right"][data-dragging="true"] ~ .aj-shell__right
  { transition: none }`（sibling 选择器，无 `:has()`）。Oracle 评审纠偏 3 处
  （`margin-left:auto` 溢出无效 → `align-self`；拖拽比例精确逆映射零漂移；
  抽屉宽度 100%）。
- ✅ **(2) 顶栏「...」五项菜单 + 设置弹窗 + 悬停二级菜单**：一级仅
  项目/继续会话…/收藏会话/设置…/帮助；项目与收藏会话 hover/focus 左向飞出
  （`aria-haspopup=menu` + aria-expanded、120ms 关闭延迟防桥接缝、Esc 先关
  飞出层、`suppressFocusOpenRef` 修复焦点归还重开、↑↓ 键盘导航、点击兼容触屏）；
  排序/过滤/演示数据/连接设置迁入新 `SettingsDialog`（ModalDialog 加宽
  `.aj-settings` 卡 + 左 `role=tablist` 连接/会话列表/演示数据 + 右 tabpanel，
  ↑↓/Home/End 漫游焦点）；新 `sidebarPrefsStore`（`agentj.sidebarPrefs` JSON +
  `agentj:sidebar-prefs-updated` 同 tab 事件 + storage 跨 tab），UnifiedSidebar
  迁移至 store（删除 legacy 事件与本地 state）；ModalDialog 加性
  `cardClassName` prop。
- ✅ **(3) 右栏成对面板（单开铺满 / 双开可调高 + 记忆）**：`.aj-right__body`
  overflow hidden + `.right-section` flex 列（`data-open=true` → `flex:1 1 auto`；
  单开铺满至底部，内容内部滚动 `sec-body { flex:1; min-height:0; overflow-y:auto }`）；
  双开时 RightPanel 在子节点间插入 `role=separator` 水平分隔条（`Children.toArray`
  首两个 child 视作任务/文件变更）：拖拽比例 = `(clientY − body.top − 3) / body.height`
  （与 CSS `flex-basis: calc(var(--aj-split) * 100%)` 精确逆映射，零漂移；含 jsdom
  零矩形 guard）、↑↓ ±2%（Shift ±8%）、双击/Home 复位为自动；持久化
  `agentj.layout.rightSectionSplit`（0.15–0.85，null=内容自适应）；
  `useRightSection` 改 `useSyncExternalStore` + `setRightSection` 派发
  `agentj:right-sections-updated`，RightPanel 订阅两侧状态渲染 `data-both-open/
  data-split` 与 `--aj-split`；TodoPanel/ChangeList 根加 `data-section/data-open`。
- ✅ **(4) 文件树图标 + 美化**：新 `fileIcons.ts` 纯映射（code/json/doc/style/
  config/shell/image/archive/default；大小写不敏感、支持 dotfile；21 单测）；
  FileTree 目录 Folder/FolderOpen（`dir-icon--open/closed`，accent/muted），文件
  分类 lucide 图标（`file-icon--*`，aria-hidden 保持可访问名）；`.chev` 改
  lucide ChevronRight（CSS 旋转）；固定 4 级缩进改内联 `paddingLeft = depth*14+2`
  （任意深度）；filetree.css token 化行高/hover/focus-visible/分类配色/省略号 +
  reduced-motion；8 旧测试保留 + 2 新增。
- ✅ **门禁全绿**：cargo fmt / 564 Rust tests / clippy 零警告 / drift /
  sdk 46 / web 263 → 308（35 files）/ web build / tui 71 / check-version 0.16.5 /
  desktop-ui。
- ✅ **实机验收（Playwright，:5173 + :4096 全栈，demo 模式）4 项全过**：
  ① 折叠/展开 rAF 采样 13 帧单调过渡（320→40 / 40→320），内层宽恒 320 零回流，
  computed `transition: width 0.2s` + `align-self: flex-end`；
  ② 一级菜单恰 5 项、旧项（连接设置/排序/过滤/演示数据）不在其中；项目飞出层
  当前项 ✓ + 「＋ 添加项目」、收藏飞出层空态；左向定位（飞出右缘 ≤ 菜单左缘）；
  设置弹窗分类切换、排序/过滤写 `agentj.sidebarPrefs`、连接表单就位、关闭焦点
  归还「更多」；离开 320ms 内飞出层关闭；
  ③ 基线双开铺满（gapBottom 0）、拖拽 +120px 时 todo 509/changes 288（1:1）、
  刷新后逐像素保留（0.6333670610211706）、单开 todo 766px 铺满、重开恢复比例、
  ↑↓ −2% / Home 复位（键清除 + data-split 移除）；
  ④ 目录开合图标切换、`file-icon--doc`（README.md）、30px 深缩进、footer
  「10 个文件 · 3 处变更」不变；console 0 errors。

## 遗留 / 下一步

- **下一特性（已确认，方案 2）：内核直接托管 Web UI** —— `serve` 增加静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接打开
  `http://<host>:4096/` 即用、WS 同源零配置；立项时需覆盖：dist 路径发现/
  `--web-dist` 参数、`/` 与 `/ws` 路由共存、token 下静态资源策略、测试与文档。
- 观察：移动端抽屉 + 桌面折叠偏好共存时沿用 v0.16.4 行为（body `visibility:
  hidden` 保持 aria 一致；未改 App 传参）。
- 观察：`.aj-right__body` 不再整体滚动，改为逐面板内部滚动（双开时各 pane 独立
  scrollbar）；两区块间原 `right-section + right-section` 边框在插入分隔条时由
  分隔条取代。
