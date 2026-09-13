# agentj 工作台三栏布局 —— 界面设计

> 范围：**只做界面设计**（视觉 + 交互规格 + 可复用需求提示词）。
> 不含产品代码实现。右栏与协议相关数据在阶段一为占位/半数据；数据契约见 `protocol-extension-prompt.md`。

## 0. 预览

深色 / 浅色 × 4 个状态，共 8 张（每张另有 `@2x` 2880×1800 版本）。

| # | 状态 | 1x | 设计源文件 |
|---|---|---|---|
| 01 | 会话 tab + 右栏 TODO/变更 | `preview-01-main.png` | `mockup-main.html` |
| 02 | 文件 tab + 右栏展开 diff | `preview-02-files.png` | `mockup-files.html` |
| 03 | （同 01）浅色主题 | `preview-03-main-light.png` | `mockup-main-light.html` |
| 04 | （同 02）浅色主题 | `preview-04-files-light.png` | `mockup-files-light.html` |
| 05 | Composer 选项：模型 / Agent / 熵变（Agent 面板展开 + 熵变面板） | `preview-05-composer-options.png` | `mockup-options.html` |
| 06 | 模型提问卡片（输入框顶部，展开）+ 信息流中的问答记录 | `preview-06-question-card.png` | `mockup-question.html` |
| 07 | （同 05）浅色主题 | `preview-07-composer-options-light.png` | `mockup-options-light.html` |
| 08 | （同 06）浅色主题 | `preview-08-question-card-light.png` | `mockup-question-light.html` |

参考对象：DeepSeek Web UI 的**信息架构**（导航区 / 对话区 / 运行详情区三段式）。
视觉基调：**沿用 agentj 现有 `--aj-*` 设计 token**，不重做配色与字体。

## 1. 画布与布局规格

基准画布 1440×900（桌面）。顶栏 48px，其下为三栏 + 两条可拖拽分隔条。

| 区域 | 默认宽 | 最小 | 最大 | 说明 |
|---|---|---|---|---|
| 顶栏 | 全宽 | — | — | 48px；连接状态灯、模型 chip、主题/更多 |
| 左栏 | 256px | 200 | 420 | 顶部「会话 / 文件」分段切换 |
| 分隔条 A / B | 7px 命中区（1px 视觉线） | — | — | `cursor: col-resize`，含 3 点 grip |
| 中栏 | 自适应 | 480 | — | 内容区 `max-width: 720px` 居中；底部 composer |
| 右栏 | 320px | 260 | 480 | 「运行详情」：TODO + 文件变更；可折叠 |

- 中栏始终吸收剩余空间；左/右栏为固定基准 + 用户可调。
- 分隔条视觉用 `--aj-border` 的 1px 发丝线，不喧宾夺主；hover/拖拽态高亮为 `--aj-accent`。

## 2. 三栏职责与数据来源

| 区域 | 内容 | 阶段一数据来源 |
|---|---|---|
| 左栏 · 会话 | 搜索、`+ 新建会话`、会话列表（当前项高亮 + 左侧 accent 竖条）、项目分组、`+ 添加项目` | SDK `session/list` |
| 左栏 · 文件 | 当前项目目录树（懒加载、`▸/▾` 折叠、git 状态点 M/A/U） | **占位/mock**；待内核 `file/list` |
| 中栏 | 对话流：用户气泡 → AI 文本（含代码块）→ 工具卡（成功/运行中/失败）→ usage 结算 → 问答记录；底部 composer | `$/progress`（text / tool_call / tool_result / permission / usage） |
| 中栏 · composer | 项目选择器 + 文本域 + **选项选择器（模型 / Agent / 熵变）** + 发送；顶部可挂**模型提问卡片** | 阶段一占位（模型名来自 `kernel/status`） |
| 右栏 · 任务 | TODO 勾选列表（已完成 / 进行中 / 待办 + 状态 pill） | **占位**；待协议支持 |
| 右栏 · 文件变更 | 变更文件列表（`+N/-N` + `新增/修改` 徽标），可展开 unified diff | **占位/半数据**；可由 `write/edit` 类 `tool_call` 派生初版 |

> 协议缺口（明确记录，不在本设计内改造）：当前 `protocol/methods.json` 无 TODO 事件、无文件 diff 事件、无目录列举方法，也**没有「模型提问/作答」方法**。右栏、文件树、提问卡片均标注「待协议支持」，契约见 `protocol-extension-prompt.md`。

## 3. 视觉规格（Dark / Light）

来源 `agentj-ui/packages/tokens/src/palette.ts`，实现时**改动 palette.ts 后走 generate**，不手改 `tokens.css`。

| token | Dark | Light | 用途 |
|---|---|---|---|
| `--aj-bg` | `#151517` | `#FFFFFF` | 应用/中栏底 |
| `--aj-sidebar` | `#1B1B1C` | `#F9FAFB` | 左/右栏底 |
| `--aj-panel` | `#232324` | `#FFFFFF` | 卡片、选中行、composer、popover |
| `--aj-raised` | `#2C2C2E` | `#F9FAFB` | 工具卡、chips、次级按钮、滑块底 |
| `--aj-text` | `#F9FAFB` | `#0F1115` | 主文本 |
| `--aj-textMuted` | `#CFD3D6` | `#61666B` | 次文本、参数 |
| `--aj-textCaption` | `#8B8F98` | `#8A8F98` | 最弱文本、grip 点、说明行 |
| `--aj-accent` | `#679EFE` | `#4176E6` | 主行动、选中、运行中、滑块填充 |
| `--aj-accentOnAccent` | `#0A0C12` | `#FFFFFF` | accent 上的文字 |
| `--aj-userBubble` | `#2C2C2E` | `#EAF1FE` | 用户气泡底 |
| `--aj-border` | `#2E2E31` | `#E4E7EA` | 发丝分隔线 |
| `--aj-codeBg` | `#1B1B1C` | `#F9FAFB` | 代码块底 |
| `--aj-inlineCodeBg` | `#292929` | `#FAFAFA` | 行内 code |
| `--aj-success` | `#22C55E` | `#22C55E` | 成功、新增 |
| `--aj-error` | `#F25A5A` | `#EC1313` | 失败、删除 |
| `--aj-warning` | `#F59E0B` | `#F59E0B` | 修改、警告 |
| `radius` | `ctl 8 / card 12 / xl 22` | 同 | 控件 / 卡片 / 气泡与 composer |
| `--aj-gap` | `12px` | 同 | 栏间距、区块间距 |

**浅色特别注意**：`panel` 与 `bg` 同为白、`raised` 与 `sidebar` 同为 `#F9FAFB`，因此**所有卡片/行必须依赖 `--aj-border` 发丝线**区分，不能只靠底色差。
accent 选中态用 `rgba(65,118,230,.10)` 叠加；diff 条纹浅色用 `rgba(34,197,94,.14)` / `rgba(236,19,19,.10)`。

## 4. 交互规格

### 4.1 布局
- **拖拽**：拖动分隔条实时改宽；松手写入 `localStorage`。
  - `agentj.layout.leftWidth` / `agentj.layout.rightWidth`
  - `agentj.layout.leftTab`（`sessions` / `files`）
  - `agentj.layout.rightCollapsed`（`0` / `1`）
- **复位**：双击分隔条 → 恢复默认宽（左 256 / 右 320）。
- **键盘可达**：分隔条 `role="separator"` + `tabindex=0` + `aria-valuenow/min/max`；
  `←/→` 每次 8px，`Shift+←/→` 每次 32px，`Home` 复位；`Enter/Space` 切换右栏折叠。
- **折叠**：右栏折叠为约 40px 图标栏（保留「运行详情」入口），不丢失当前滚动位置。
- **左栏 tab**：分段控件；切换只换面板内容，不改宽度。

### 4.2 Composer 选择器
- 同一时刻**只允许一个 popover 展开**；点击外部 / `Esc` 关闭；触发按钮 `aria-expanded` + `aria-haspopup="listbox"`。
- 键盘：`↑/↓` 移动高亮，`Enter` 确认，`Esc` 关闭并回到触发按钮。
- 选中即更新触发按钮上的标签；持久化 `agentj.composer.model` / `.agent` / `.entropy`。

### 4.3 提问卡片
- 卡片在 composer 内部、文本域**之上**，与 composer 头部以发丝线分隔；可折叠。
- 折叠态为单行摘要（`✓ 已选择：X`），点击展开；持久化 `agentj.ask.collapsed`。
- 单选即选（点击行选中）；若为多选则用 checkbox；提交后卡片从 composer 移除，问答以记录卡进入信息流。

## 5. Composer 选项选择器（模型 / Agent / 熵变）

位置：composer 卡片底部的工具栏行，三个可展开选择器（左对齐）+ 右侧圆形发送按钮。

| 选择器 | 触发标签（示例） | 展开面板内容 |
|---|---|---|
| **模型** | `◆ deepseek-chat ⌄` | 模型列表：名称（13px）+ 能力说明（11px muted，如 `128k 上下文 · 工具调用`）+ 当前项 `✓` 与 accent 淡底 |
| **Agent** | `⚙ Build ⌄` | `Build`（默认 · 读写工具全开）/ `Plan`（只读 · 先出方案不落盘）/ `Explore`（只读 · 大范围检索）；底注 `Agent 决定可用工具与权限范围。` |
| **熵变** | `∿ 均衡 ⌄` | 三档预设：`低温 · 精确 0.2` / `均衡 · 推荐 0.7`（默认）/ `高温 · 发散 1.2`；分隔线；`自定义` 滑块（0–2，显示数值）；说明 `数值越高输出越发散，越低越确定。` |

规格：
- 触发按钮：26px 高、胶囊/圆角 8、`--aj-border` 发丝边、`--aj-panel` 底、12px `--aj-textMuted`，左 icon + 标签 + `⌄`。
- 面板：宽约 288px、`--aj-panel` 底、发丝边、圆角 12、`--aj-shadowRaised`、padding 6px；**向上展开**（composer 在底部），以绝对定位浮层渲染，**不得被 composer 或视口裁剪**。
- 选中行：`--aj-accent` 的 `✓` + accent 淡底（深 `rgba(103,158,254,.12)` / 浅 `rgba(65,118,230,.10)`）。

数据映射（阶段一占位）：
- **模型** ← `kernel/status.default_model`（阶段一静态列表）。
- **Agent** ← 子代理 / skills 注册表（阶段一静态三项）。
- **熵变** ← **采样随机性，映射 `temperature`**（仓库 `agentj-core/src/types.rs` 的 `ModelOptions.temperature: Option<f32>`，默认 0.7；三种 provider 线格式均发送 `temperature`）。档位 0.2 / 0.7 / 1.2 为预设值。**若产品对「熵变」有别的定义，只需更换标签与档位映射，控件形态不变。**

## 6. 模型提问卡片与回答（可折叠）

### 6.1 展开态（composer 内、文本域之上）
- 头部：22px 圆角方 `?` 徽标（accent 淡底 + accent 字形）+ 问题文本（13px/600）+ 右侧折叠 chevron。
- 说明行（11px `--aj-textCaption`）：如 `模型需要你确认后再继续。`
- 选项列表：每行 = 16px 单选圈 + 13px 标签 + 右侧 11px muted 提示（如 `推荐` / `范围较大`）；选中行 accent 填充 + accent 淡底。
- 底部行：`其他…` 自定义输入（flex-1，32px，发丝边）+ `跳过`（ghost）+ `提交`（accent，`--aj-accentOnAccent` 文字）。

### 6.2 折叠态
单行摘要：`✓ 已选择：三栏 + 协议扩展` + chevron，12px `--aj-textMuted`；点击重新展开。

### 6.3 回答与记录
- 提交后：卡片从 composer 移除，composer 恢复常规形态；回答作为一条用户消息下行。
- 信息流中的**问答记录卡**（compact）：`? 模型提问` + `已折叠` + chevron / 问题文本 / `→ <用户选择>`（accent 淡底 pill）/ 可选 `已按此范围继续执行。`
- `跳过`：不产生用户回答，模型按自身判断继续。

### 6.4 可达性
- 卡片 `role="group"` + `aria-labelledby` 指向问题文本；选项为原生 `radio` / `checkbox`。
- 折叠按钮 `aria-expanded`；折叠态摘要本身是按钮，可键盘聚焦。

### 6.5 协议依赖
需要内核支持「模型发起结构化提问 + 客户端作答」，即 `$/askQuestion`（通知）+ `question/respond`（请求）；契约见 `protocol-extension-prompt.md`。

## 7. 组件与文件映射（贴合现有工程约定）

| 新增 | 说明 | 参考现有 |
|---|---|---|
| `apps/web/src/components/SplitPane.tsx` + `split.css` | 受控宽度 + 拖拽/键盘/双击复位 | `layout.css` 分隔线与断点 |
| `apps/web/src/components/WorkspaceShell.tsx` | 三栏 flex 容器，替换 `App.tsx` 内裸 `.aj-layout` | `layout.css` |
| `apps/web/src/components/SidebarTabs.tsx` | 「会话 / 文件」分段切换 | `sidebar.css` 工具栏 |
| `apps/web/src/components/FileTree.tsx` + `filetree.css` | 目录树，懒加载 + 折叠 + git 状态点 | mono 字号沿用 |
| `apps/web/src/components/RightPanel.tsx` + `rightpanel.css` | 右栏容器 + 折叠 | `layout.css` 抽屉 |
| `apps/web/src/components/TodoPanel.tsx` | TODO 列表与状态 pill | — |
| `apps/web/src/components/ChangeList.tsx` | 变更列表 + 迷你 diff | `conversation.css` 工具卡 |
| `apps/web/src/components/OptionPopover.tsx` + `options.css` | 通用向上展开选择面板（模型 / Agent / 熵变共用） | `layout.css` popover |
| `apps/web/src/components/ComposerOptions.tsx` | 三个选择器触发 + 状态与持久化 | `composer.css` |
| `apps/web/src/components/AskQuestionCard.tsx` + `ask.css` | 提问卡片（展开/折叠）+ 问答记录卡 | `composer.css` |
| `packages/tokens/src/palette.ts` | 新增布局常量与 accent 淡底 tint | 走 `npm run generate` |

不改：`packages/sdk`、`packages/protocol`、任何 Rust crate（阶段一）。

## 8. 响应式与可达性

- `≥1024px`：三栏常驻。
- `<1024px`：左栏退化为现有抽屉；右栏折叠为右侧抽屉（按钮 `aria-expanded` 同步）。
- Composer 选择器面板在窄屏改为向上满宽抽屉，避免超出视口。
- 所有切换/折叠按钮带 `aria-label`；焦点不被 scrim 遮挡；`prefers-reduced-motion` 下关闭动画。

## 9. 验收标准

1. 两条分隔条可拖拽，刷新后宽度保持；双击复位。
2. 左栏「会话 / 文件」切换正常且记忆；目录树可展开/折叠。
3. 右栏可折叠/展开，TODO 与文件变更在占位数据下渲染正常。
4. Composer 三个选择器可展开/收起，同一时刻仅一个展开，选中项持久化；面板不被裁剪。
5. 提问卡片可展开/折叠；提交后问答记录进入信息流；`跳过` 可用。
6. `<1024px` 左/右栏各自退化为抽屉，无内容溢出、无横向滚动。
7. 仅用键盘可完成「调宽 + 折叠右栏 + 切换左栏 tab + 展开选择器并选中 + 提交回答」。
8. `npm --prefix agentj-ui test --workspace=@agentj/web` 与 `tsc -b` 通过。

## 10. 如何重新出图（本机无 Chrome/Playwright 时）

设计源为静态 HTML，用 WKWebView 离屏快照渲染，输出 2x（`render.swift` 与本目录同在）：

```
# 编译离屏 WKWebView 渲染器（1440x900 视口）
R=/var/folders/g7/qp16jb0n4196_h16hx0871z40000gn/T/opencode/render/render
swiftc -O render.swift -o "$R"

$R mockup-main.html          preview-01-main@2x.png          1440 900   # → 2880x1800
$R mockup-files.html         preview-02-files@2x.png         1440 900
$R mockup-main-light.html    preview-03-main-light@2x.png    1440 900
$R mockup-files-light.html   preview-04-files-light@2x.png   1440 900
$R mockup-options.html       preview-05-composer-options@2x.png      1440 900
$R mockup-question.html      preview-06-question-card@2x.png         1440 900
$R mockup-options-light.html preview-07-composer-options-light@2x.png 1440 900
$R mockup-question-light.html preview-08-question-card-light@2x.png   1440 900

# 1x 视图版；@2x 必为 2880x1800，1x 必为 1440x900
for f in 01-main 02-files 03-main-light 04-files-light \
         05-composer-options 06-question-card 07-composer-options-light 08-question-card-light; do
  sips -Z 1440 "preview-$f@2x.png" --out "preview-$f.png"
done
```

> 注意：`qlmanage -t` 会强制使用自身布局视口（约 1014px）再缩放，会把 1440 宽的三栏横向裁掉右栏 —— 不要用它出图。

---

## 11. 需求提示词（复制即用）

以下整段可直接投喂 AI 编码工具（Cursor / Claude Code / v0 等）或作为评审 PRD。

```text
# 需求：agentj 工作台三栏布局 + Composer 选项 + 模型提问卡片（Web + Desktop）

## 背景
agentj 是 AI 编码代理工具（Rust 内核 + JSON-RPC 2.0 协议）。
前端 agentj-ui 为 npm workspaces monorepo：
  - apps/web      —— Vite + React 19 + TypeScript，组件级 CSS（*.css 与组件同目录），图标用 lucide-react
  - apps/desktop  —— Tauri v2 壳，直接套 apps/web 构建产物（一套 UI 两端复用）
  - packages/tokens —— 设计 token 单一来源 src/palette.ts，生成 tokens.css / tokens.json / docs/ui-tokens.md
  - packages/sdk  —— RpcClient / WebSocketTransport / METHODS
  - packages/protocol —— 协议类型生成
现有视觉语言：深色/浅色双主题、--aj-* CSS 变量、0.5px 发丝线、圆角气泡、
带状态点的工具卡、悬浮圆角 composer。
协议通过 $/progress 推送流式帧：text / tool_call / tool_result / permission / usage。

## 目标
1) 工作区改为三栏布局，参考 DeepSeek Web UI 的信息架构（导航区 / 对话区 / 运行详情区），
   保留 agentj 现有设计 token，不重做配色与字体。
2) composer 增加「模型 / Agent / 熵变」三个可展开选择器。
3) 增加「模型提问卡片」：贴合输入框顶部展示，可折叠，并设计回答的记录展示。

## A. 三栏布局
- 顶栏 48px：品牌 agentj + 连接状态灯 + 模型 chip + 主题/更多按钮。
- 左栏（默认 256px，范围 200–420）：
  · 顶部「会话 / 文件」分段切换（tab）。
  · 会话 tab：搜索框、+ 新建会话、会话列表（当前项高亮 + 左侧 2px accent 竖条）、项目分组、+ 添加项目。
  · 文件 tab：项目目录树，▸/▾ 折叠、懒加载、git 状态点（M 橙 / A 绿 / U 灰）。
- 中栏（自适应，最小 480）：对话流 content max-width 720px 居中。
  · 用户气泡右对齐（--aj-userBubble，圆角 22px，右下 4px）；
  · AI 文本左对齐（左侧 2px --aj-accent 竖条），支持行内 code 与代码块；
  · 工具卡：8px 状态点（成功 --aj-success / 运行中 --aj-accent 脉冲 / 失败 --aj-error）
    + mono 工具名 + 参数 + 展开；
  · usage 结算行（prompt / completion / 耗时，mono 12px muted）；
  · 底部 sticky composer。
- 右栏（默认 320px，范围 260–480）：标题「运行详情」+ 折叠按钮。
  · 任务（TODO）：勾选列表，已完成 / 进行中（脉冲）/ 待办 + 状态 pill；
  · 文件变更：图标 + mono 相对路径 + +N 绿 / -N 红 + 新增/修改 徽标，可展开 unified diff
    （新增行 rgba(34,197,94,.14) + '+'，删除行 rgba(236,19,19,.10) + '-'）。
- 左右边界为可拖拽分隔条：7px 命中区 + 1px 发丝线 + 垂直居中 3 点 grip，cursor: col-resize。

## B. Composer 选项选择器（模型 / Agent / 熵变）
- 位置：composer 卡片底部工具栏行，三个可展开选择器（左对齐）+ 右侧圆形发送按钮。
- 触发按钮：26px 高，发丝边，--aj-panel 底，12px --aj-textMuted，左 icon + 标签 + ⌄。
- 面板向上展开（composer 在底部），宽约 288px，--aj-panel 底，发丝边，圆角 12，
  --aj-shadowRaised，padding 6px；必须用绝对定位浮层，不得被 composer 或视口裁剪。
- 模型：模型列表，名称 + 11px muted 能力说明（如 “128k 上下文 · 工具调用”）+ 当前项 ✓ 与 accent 淡底。
- Agent：Build（默认 · 读写工具全开）/ Plan（只读 · 先出方案不落盘）/ Explore（只读 · 大范围检索），
  底注“Agent 决定可用工具与权限范围。”
- 熵变：三档预设 低温·精确 0.2 / 均衡·推荐 0.7（默认）/ 高温·发散 1.2；
  分隔线；自定义滑块（0–2，显示数值）；说明“数值越高输出越发散，越低越确定。”
  语义：熵变 = 采样随机性，映射 temperature（agentj-core/src/types.rs 的 ModelOptions.temperature: Option<f32>，默认 0.7）。
- 交互：同一时刻仅一个面板展开；点击外部 / Esc 关闭；↑↓ 移动、Enter 确认；
  选中更新触发标签；持久化 agentj.composer.model / .agent / .entropy。
- 阶段一数据：模型取 kernel/status.default_model（静态列表即可）；Agent 与熵变为静态占位。

## C. 模型提问卡片（可折叠）+ 回答记录
- 位置：composer 内部、文本域之上，与 composer 头部以发丝线分隔。
- 展开态：22px 圆角方 ? 徽标（accent 淡底 + accent 字形）+ 问题文本 13px/600 + 右侧折叠 chevron；
  说明行 11px --aj-textCaption；选项列表（16px 单选圈 + 13px 标签 + 右侧 11px muted 提示，
  选中行 accent 填充 + accent 淡底）；底部行 = “其他…” 输入（flex-1）+ 跳过（ghost）+ 提交（accent）。
- 折叠态：单行“✓ 已选择：<选项>” + chevron，12px --aj-textMuted，点击展开。
- 回答后：卡片从 composer 移除，composer 恢复；信息流新增问答记录卡：
  “? 模型提问” + “已折叠” + 问题 + “→ <用户选择>”（accent 淡底 pill）+ 可选“已按此范围继续执行。”
- 跳过：不产生用户回答，模型自行决定继续。
- 可达性：卡片 role="group" + aria-labelledby；选项用原生 radio/checkbox；折叠按钮 aria-expanded。
- 阶段一：卡片与记录用占位数据（协议无提问方法），数据契约见 protocol-extension-prompt.md。

## D. 交互
- 拖拽分隔条实时改宽；双击复位默认宽；松手写 localStorage。
- 持久化 key：agentj.layout.leftWidth / .rightWidth / .leftTab / .rightCollapsed
  + agentj.composer.model / .agent / .entropy + agentj.ask.collapsed。
- 分隔条 role="separator" + tabindex=0 + aria-valuenow/min/max；←/→ 调 8px，Shift+←/→ 调 32px，Home 复位。
- 右栏可折叠为约 40px 图标栏；左栏 tab 切换不改变宽度。

## E. 响应式
- ≥1024px 三栏常驻。
- <1024px 左栏沿用现有抽屉行为；右栏折叠为右侧抽屉；composer 选择器面板改为向上满宽抽屉；
  切换按钮 aria-expanded 同步；prefers-reduced-motion 下关闭动画。

## F. 设计 token
深色：bg #151517 / sidebar #1B1B1C / panel #232324 / raised #2C2C2E / text #F9FAFB / textMuted #CFD3D6
        accent #679EFE / accentOnAccent #0A0C12 / userBubble #2C2C2E / border #2E2E31
        codeBg #1B1B1C / inlineCodeBg #292929 / success #22C55E / error #F25A5A / warning #F59E0B / textCaption #8B8F98
浅色：bg #FFFFFF / sidebar #F9FAFB / panel #FFFFFF / raised #F9FAFB / text #0F1115 / textMuted #61666B
        accent #4176E6 / accentOnAccent #FFFFFF / userBubble #EAF1FE / border #E4E7EA
        codeBg #F9FAFB / inlineCodeBg #FAFAFA / success #22C55E / error #EC1313 / warning #F59E0B / textCaption #8A8F98
radius: ctl 8 / card 12 / xl 22；gap 12px；shadowRaised 0 12px 32px rgba(0,0,0,.12)
浅色注意：panel 与 bg 同为白、raised 与 sidebar 同为 #F9FAFB，因此卡片必须靠 --aj-border 发丝线区分，
不能只靠底色差；选中态用 accent 淡底 + ✓/填充。
新增布局常量与 accent tint 写进 palette.ts 并跑 generate，不手改 tokens.css。

## G. 建议组件拆分
SplitPane.tsx (+split.css)、WorkspaceShell.tsx、SidebarTabs.tsx、FileTree.tsx (+filetree.css)、
RightPanel.tsx (+rightpanel.css)、TodoPanel.tsx、ChangeList.tsx、
OptionPopover.tsx (+options.css)、ComposerOptions.tsx、AskQuestionCard.tsx (+ask.css)。

## H. 范围与约束
- 阶段一【不做】后端协议改造：协议现无 TODO 事件、无 diff 事件、无目录列举方法、无提问/作答方法。
  文件树、右栏两块、提问卡片均先用占位/半数据（文件变更可由 write/edit 类 tool_call 派生初版），
  并标注“待协议支持”。数据契约另见 protocol-extension-prompt.md。
- 不改 packages/sdk / packages/protocol / 任何 Rust crate。
- 不用 as any / @ts-ignore；不引第三方 UI 库；图标沿用 lucide-react。
- 不做第四栏；不重做主题配色（浅色为既有 token）；不动 TUI。

## I. 验收
1. 两条分隔条可拖拽且宽度持久化，双击复位。
2. 左栏 tab 切换正常且记忆；目录树可折叠。
3. 右栏可折叠/展开；TODO 与变更列表在占位数据下渲染正常。
4. Composer 三个选择器可展开/收起，同一时刻仅一个展开，选中项持久化，面板不被裁剪。
5. 提问卡片可展开/折叠；提交后问答记录进入信息流；跳过可用。
6. <1024px 左右栏各自退化为抽屉，无溢出、无横向滚动。
7. 仅键盘可完成调宽 / 折叠右栏 / 切换左栏 tab / 展开选择器并选中 / 提交回答。
8. npm --prefix agentj-ui test --workspace=@agentj/web 与 tsc -b 通过。
```

## 12. 设计稿对照

静态设计稿：`mockup-main.html` / `mockup-files.html` / `mockup-options.html` / `mockup-question.html`
及其 `-light` 版本（自包含、无外部依赖）。它们仅用于视觉评审，不是产品代码，不接入构建。
重出图见 §10。
