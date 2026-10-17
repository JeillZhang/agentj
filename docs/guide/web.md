# Web 工作台

`agentj-ui/apps/web`（Vite + React 19）是三栏工作台，消费 `@agentj/sdk` +
`@agentj/tokens` + `lucide-react`。既可作为开发 server，也可由内核同端口托管
（见 [HTTP 面与鉴权](http-and-auth.md)）。

## 连接与地址派生

首次打开或需要切换内核时，进 **设置 → 连接**：字段为 `Host` / `Port` / `Token`
（占位值 `127.0.0.1` / `4096` / 空）。`Host` 必填；`Port` 留空按 `4096`；`Token`
留空表示内核未配 token。点 `Connect` 立即重连，设置同时写入浏览器本地。

WebSocket 地址按以下优先级解析（前者生效则不再看后者）：

1. **手动设置**——对话框输入的值，或上次保存的值；只要填了 `Host` 或 `Port` 即生效。
2. **构建期环境变量** `VITE_AGENTJ_WS_URL`（仅在未手动指定时使用）。
3. **页面地址派生**——当前页面的 `hostname` + 端口 `4096`；https 页面自动用 `wss`。
4. **回环兜底** `ws://127.0.0.1:4096/ws`。

因此：内核跑在 `4096` 时，浏览器直接打开即连上（`npm run dev` 的 5173 dev server
同样适用，因为派生取的是页面 `hostname` 而非端口）。内核换端口、或跑在另一台机器时，
用「设置 → 连接」填 `Host` / `Port`，或构建时传入
`VITE_AGENTJ_WS_URL=ws://<host>:<port>/ws`。

- `tauri.localhost`（Desktop 壳）始终派生成明文回环 `ws://`，不受页面协议影响。
- 畸形地址（例如 `Host` 里误带端口）会被回退到派生地址，避免 `new WebSocket` 抛错。
- 重连（或切换会话）后，工作台会自动对当前会话重新 `session/subscribe`，接回仍在运行的
  输出流。断连不会中断内核里的 run（见 [IPC 协议](../specs/ipc-protocol.md) 异常断开）：
  若此时内核仍在执行上一轮任务，发送会被拒为 `-32805 SessionBusy`，输入区显示
  「该会话已有任务在运行…」提示而不是原始错误码。
- 连接设置**不会**跨标签页同步，其他偏好会（见[本地持久化](#本地持久化)）。
- 服务端侧的 token / 登录门 / 会话 cookie / WS 三通道鉴权见
  [HTTP 面与鉴权](http-and-auth.md)。

## 布局

三栏：**左栏**（会话 / 文件）· **中栏**（对话）· **右栏**（运行详情）。

- 栏宽可拖拽 / 键盘调节 / 双击复位，持久化到本地。
- 视口 `<1024` 时右栏变为抽屉。
- 右栏折叠/展开为单段平滑动画（内容零回流）。

## 左栏

- **会话列表**：新建 / 继续 / 切换；收藏（星标）；逐行删除与一键清理。
  ⋯ 菜单含「清理所有会话…」「过滤」「排序」（悬停飞出二级菜单）。
- **文件树**（Files tab）：懒加载 `file/list`；目录按扩展名分类图标
  （Folder/FolderOpen 开合）；git 状态字母（M/A/U/D；目录行聚合后代最高优先级）；
  点击跳转高亮。
- 底部快捷键提示行。

## 中栏（对话）

- 流式渲染 `$/progress`；工具调用卡片、Markdown、审批行。
- **代码块卡片**：语法高亮（`rehype-highlight`）；左上折叠/展开、右上复制
  （clipboard → `execCommand` 降级）。
- 顶部模型 chip 右侧显示会话 token 总量 `Σ`，并拆分「输入 / 输出」明细。

## Composer（输入区）

- 居中浮动卡片（75% 宽），内容延伸至卡片底部、滚底对齐。
- **模型 / Agent / 熵变** 三选择器（逐触发器锚定弹层，互斥，持久化）。
  - 模型行全部来自内核（`kernel/status.providers[].models` + 账号目录合并）。内核**没给**
    模型列表时弹层只显示「内核未返回模型列表」——不虚构模型名，因为凭虚构的名字发起的运行
    会被内核直接拒绝（`unknown model … for provider …`）。
  - 本地存的旧选择（`agentj.composer.model`）只用于显示，且只在它仍在内核列表里时才显示；
    真正发给内核的模型永远来自选择器选中的那一行。
- **权限选择器**：逐次确认 / 自动允许（自动允许时客户端对 `$/permissionRequest`
  自动 allow；内核 deny / 路径越界仍强制拒绝）；跨标签同步。
- **滚动自动收缩**：上滚离开底部自动收缩为「展开输入框」按钮（原位，零位移），
  下滚回底或点击平滑展开；草稿保留；待答提问强制展开。

## 右栏（运行详情）

- **任务**（`$/progress kind:"todo"` 全量快照 + `revision` 门控）。
- **文件变更**（`file_change` upsert）：unified diff 展开、来源徽标
  （write/bash）、截断提示「已截断（原始 N KiB）」、按 `seq` 的历史抽屉。
- 按 **run** 分组（当前 run 置顶 / 8-char short id / 未分组历史）；点击 run 组头聚焦。
- 两个面板可折叠/展开；双开时可拖拽分隔条（比例持久化，↑↓ ±2% / Shift ±8%、
  双击 / Home 复位）。

## 提问卡片

模型经 `$/askQuestion` 提问 → 卡片展示可折叠选项与问答记录 → `question/respond` 回传。

## 设置

顶栏「设置…」对话框（可缩放）：

- **连接**：`Host` / `Port` / `Token`，手动指定内核地址（见[连接与地址派生](#连接与地址派生)）。
- **显示**：对话 / 左栏 / 右栏 / 代码四区字号（px 滑杆 + 数字）、行高、密度、
  减弱动画。
- **项目**：全局默认项目、新项目工作目录预填、项目列表（默认徽标 / 编辑 / 删除，
  Inbox 不可删）。
- **演示数据**：`demoData` 开关驱动 TODO / 文件树 / 变更 / 提问卡预览数据。

## 顶栏

`agentj` + 连接状态灯（hover/focus 面板显示 ws / 模型 / 模块状态 / UI 与内核版本）+
「...」菜单（项目 / 继续会话… / 收藏会话 / 设置… / 帮助）。

## 本地持久化

Web 端不用账号系统，偏好全部保存在浏览器 `localStorage`（同源）。清空浏览器数据
或换浏览器即回到默认值；`localStorage` 不可用时静默降级为「不持久化」而非报错。

| 键 | 内容 |
| --- | --- |
| `agentj.connection` | 手动连接设置（`host` / `port` / `token`） |
| `agentj.theme-mode` | 主题：`dark` / `light` / `system` |
| `agentj.layout.leftWidth` / `rightWidth` | 左 / 右栏宽度 |
| `agentj.layout.leftTab` | 左栏当前 tab（`sessions` / `files`） |
| `agentj.layout.rightCollapsed` | 右栏折叠态 |
| `agentj.layout.rightSections` | 右栏「任务」「文件变更」两面板的展开态 |
| `agentj.layout.rightSectionSplit` | 右栏双开时的上下分隔比例 |
| `agentj.composer.model` / `agent` / `entropy` | composer 的模型 / Agent / 熵变 |
| `agentj.composer.permissionMode` | 权限模式（`ask` / `bypass`） |
| `agentj.favorites` | 收藏（星标）的会话 id 列表 |
| `agentj.ask.collapsed` | 提问卡片的折叠态 |
| `agentj.sidebarPrefs` | 会话列表排序（`updated` / `created`）与过滤（`all` / `favorites`） |
| `agentj.displayPrefs` | 显示设置：四区字号（px）、行高、密度、减弱动画 |
| `agentj.projects` / `projectMap` / `projectDefaults` / `projectCollapsed` / `currentProject` | 项目列表、当前项目与项目默认值 |
| `agentj.demo` | 演示数据开关 |

默认值：左栏 `256px` / 右栏 `320px`（左限 200–420、右限 260–480）；字号对话 `15px`
（12–24）、左右栏 `13px`（10–20）、代码 `12px`（10–20）；行高与密度默认、减弱动画
关闭；排序按更新时间、过滤全部；权限模式「逐次确认」。

**跨标签同步**：布局与右栏面板、侧栏排序/过滤、显示设置、composer 选项与权限模式、
收藏与项目、演示数据开关会通过 `storage` 事件在其他标签页实时同步；
`agentj.connection` 与 `agentj.theme-mode` **不**同步（各标签页各自持有）。

## 主题与无障碍

- 深浅双主题；`@agentj/tokens` 语义色单源。
- roving tabindex 键盘导航（菜单 / 会话列表 / 文件树 / 设置）；ModalDialog 开关焦点
  恢复；焦点作用域保证 composer 输入不被单字母快捷键影响。
