# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台体验修复（v0.16.4）—— 状态灯 hover 面板 + 右栏手风琴 + 滚动条 + 弹层/收藏修复（六项）**
> ——来源=用户反馈：① 左上角仅 agentj + 状态灯；② 连接信息移至状态灯 hover 面板；
> ③ 右栏折叠=堆叠、展开=向下延展（VSCode 风格）；④ 滚动条美化；⑤ composer 弹层二次点击收起；
> ⑥ 新建会话不再自动收藏

## v0.16.4: Web 工作台体验修复（六项 + 验收 1 bug）✅（本次完成）

- ✅ **(1)(2) 顶栏状态灯 + hover 连接面板（无协议变更）**：左上角仅 `agentj` + 状态灯
  （移除 `connected` 状态文字）；新增 `ConnectionStatus` 组件——悬停/聚焦状态灯展开面板
  （`已连接 ws://…` / 模型 / 模块状态逐行；条件渲染，闭合不在 DOM；`aria-expanded`）；
  ⋯ 菜单状态/模型/模块区块与分隔线移除（TopbarMenu Props 精简）。
- ✅ **(3) 右栏 VSCode 手风琴（保留「运行详情」标题 + »）**：「任务」「文件变更」折叠 =
  36px 紧凑标题行自上而下堆叠、展开 = 内容向下延展；空数据保留标题行（占位符仅展开
  显示）；展开状态 localStorage 记忆（新 hook `useRightSection` + `agentj.layout.rightSections`
  读写容错，默认全展开）。
- ✅ **(4) 滚动条美化**：全局 token 化滚动条（webkit 10px 圆角 thumb `--aj-border`、
  hover `--aj-textCaption`、track 透明 + content-box 内缩；Firefox `scrollbar-width: thin`
  + `scrollbar-color`），light/dark 自动跟随；实机 computed 校验通过。
- ✅ **(5) composer 弹层二次点击收起**：根因 = OptionPopover 文档 `mousedown` 关闭
  （目标在 `.pop` 外）→ 随后 trigger `click` 重开（两个原生事件之间 React 已重渲染，
  `openPanel` 闭包读到的 `openId` 已为 null）。修复 = 新增可选 `anchorRef`，mousedown
  命中触发器时跳过关闭；回归测试用 `fireEvent.mouseDown + click` 复现真实浏览器时序。
- ✅ **(6) 新建会话不再自动收藏**：`UnifiedSidebar.handleNewSession` 移除
  `toggleFavorite(sid)`；仅星标按钮触发；App.test 反转为「becomes active but not
  favorited」。（SessionPanel 为未挂载遗留组件，未动。）
- ✅ **验收（实机 Playwright，:5173 全栈）发现并修复 1 bug（BUG-007）**：顶栏
  `z-index: 5` 低于侧栏 `z-index: 10` → 连接 hover 面板/顶栏下拉被侧栏遮挡
  （elementFromPoint 实测面板中心命中 `.aj-sidebar__body`）。修复 = `.aj-topbar`
  `z-index: 20`（> 侧栏 10、< dialog 100；移动端抽屉/scrim 均在顶栏下沿以下起始，
  无布局副作用）+ CSS 契约回归测试（topbar > sidebar）。其余 5 项实机全部通过：
  折叠堆叠 36px / 展开 / 刷新记忆；滚动条 computed；弹层二次点击 `popCount 0`；
  新建会话后 `agentj.favorites` 保持 null。
- ✅ 门禁全绿：cargo fmt / 564 Rust tests / clippy 零警告 / sdk 46 / web 246 → 263 /
  web build / tui 71 / desktop-ui / check-version 0.16.4。

## 遗留 / 下一步

- **下一特性（已确认，方案 2）：内核直接托管 Web UI** —— `serve` 增加静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接打开
  `http://<host>:4096/` 即用、WS 同源零配置；立项时需覆盖：dist 路径发现/
  `--web-dist` 参数、`/` 与 `/ws` 路由共存、token 下静态资源策略、测试与文档。
- 观察：`.aj-topbar` z-index 提升后，移动端抽屉 scrim（z:9）不再压暗顶栏
  （顶栏保持可点，属预期行为；jsdom 无法测 stacking，已用 CSS 契约锁定）。
- 观察（非本批改动）：Vite dev server 不监听 root 外 `packages/sdk`（alias 直连源码）
  —— 改 SDK 后需重启 dev server（已记入 AGENTS.md Commands 注记）。
