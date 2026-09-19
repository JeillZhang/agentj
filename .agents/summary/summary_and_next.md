# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 界面修复 v0.17.2（用户直报 2 项）—— 开发 + 单测 + 实机验证完成，PR 待创建/合并**
> ——来源=用户反馈：① 设置「显示」字号设置应改为数字（单位像素）+ 滑杆横向放置，
> 避免设置弹窗纵向过度拉伸；② 设置等弹窗应可通过边框或四个角调整大小。
> 范围：仅 `agentj-ui/apps/web`（Desktop 复用 Web 产物）；**Rust / SDK / protocol 零改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.17.1 → 0.17.2。

## v0.17.2: 界面修复（显示设置 px 滑杆 + 可缩放弹窗）✅（开发 + 单测 + 实机验证完成）

- ✅ **(1) 显示设置字号 → 逐区 px 数字 + 滑杆（commit `1fecd14`）**：
  `displayPrefsStore` 字号字段由 `sm|md|lg|xl` 枚举改为逐区像素数值
  （`FONT_SIZE_LIMITS` 校验：对话 12–24 默认 15、左右栏 10–20 默认 13、代码 10–20
  默认 12；数值 clamp+取整；旧枚举名按区迁移保持原渲染、防丢偏好）；
  `SettingsDisplayPanel` 每区一行「标签 + range 滑杆 + number 输入 + px 单位」
  （draft 受控，blur/Enter 提交并夹取，清空还原），行高/密度选项横排（消除 16 个
  纵向单选堆叠导致的弹窗拉伸）；`WorkspaceShell` 直接注入 `${px}px` 到 `--aj-font-*`。
  CSS 契约（`.aj-settings-font-row` grid 单行 + `.aj-settings-options` flex 行）由测试锁定。
- ✅ **(2) 弹窗边框/四角缩放（commits `75dbcf6` + `c611812`）**：`ModalDialog` 新增
  `.aj-dialog-frame` 承载尺寸/位置（默认 flex 居中，首次拖拽冻结为 fixed 矩形、零跳动），
  8 个指针手柄（4 边 + 4 角，`data-handle` + `aria-hidden`，hover accentTint +
  方向光标，pointer capture）；新 `dialogResize.ts` 纯函数 `resizeRect`（对边锚定、
  全局最小 320×200、视口 8px 边距夹取、可选 per-dialog `minWidth/minHeight`）；
  `SettingsDialog` 下限 560px（实机验证发现的压扁问题：320px 时双栏布局会让滑杆归零）；
  `.aj-settings` 宽度修饰从 card 移到 frame，`.aj-dialog-card` 基类规则保持
  （`ApprovalDialog` 独立使用不受影响）；`data-resizing` 期间 `user-select: none`。
- ✅ **实机 Playwright 验证（2026-09-19）**：显示面板 4 行单行布局（label/range/number
  同行，滑杆 320px 可见）；滑杆→数字/变量联动（20→`20px`）、数字 99→夹取 24、
  17→`17px`；缩放 SE +120/+80 → 680×526→800×606 位置不变；W 拖拽 −100 → 右缘锚定
  （右缘 1100→1100）；夹取 → 560×200（设置下限）且滑杆仍 200px 可用；
  hover 手柄 accent tint + `nwse-resize` 光标（修正前 320px 下限会压扁面板，已修复）。
- ✅ **独立复核修复波（commit `e5ea9a4`，Oracle 评审 6 项全修）**：`resizeRect`
  per-dialog 下限以视口上限夹取（375px 窄屏不再一触手柄就横向溢出）+ 增长按锚定边
  可用空间夹取（拖过头原点零漂移）；新增 `clampDialogRect` + window resize 重夹取；
  `ModalDialog` 忽略非主键 + `hasPointerCapture` 守卫 release；`displayPrefsStore`
  旧枚举改 `hasOwnProperty`（`toString`/`constructor`/`__proto__` 不再冒充通过）；
  `.aj-dialog__resize` 手柄外移（边缘 2px / 角 6px 落入卡片）避免遮挡滚动条。
  实机复验：+120/+80 精确增长、上限 972×655 原点不动、700px 视口自动重夹取 684px。
- ✅ **门禁**：web **446 tests（44 files）**（新增/改写：dialogResize 11、modalDialog 15、
  显示面板 10、store 8、workspaceShell 9）/ sdk 46 / Rust 564 tests / clippy 零警告 /
  drift 2 passed / web build / tui 71 / check-version **0.17.2** / desktop-ui。
- ✅ **版本与文档同步**：0.17.1 → 0.17.2（Cargo workspace + `Cargo.lock` 17 crates +
  sdk `config.ts`(+version 断言) + desktop 三处 + `package-lock.json`）；
  `AGENTS.md` / `README.md` / 本文件已同步。`PROTOCOL_VERSION` 保持 0.13.0。

## 遗留问题 / 遗留 minors

- 弹窗缩放为会话内状态：关闭重开回默认尺寸（未持久化；如需可加 per-dialog
  localStorage 记忆，参照 `layoutStore` 模式）。
- 缩放手柄无键盘路径（候选增强：方向键调整 + `role=separator`/aria 语义）。
- e2e_permission 会话状态污染（预存在，非本特性引入）：
  `crates/agentj-cli/tests/e2e_permission.rs` 硬编码 `session_id="s1"`，依赖
  `FileBackedStore` 默认目录；一旦被标记 Archived 则 `read_json_line` 阻塞。
  建议改为消费 `session/new` 返回的真实 id + 隔离临时目录。
- T4 minor（parked）：modal backdrop 测试判别力；M5（add-dialog 相对路径）、
  M6（picker edit 嵌套交互）、M7（其余测试覆盖缺口）—— v0.17.0 遗留，deferred。

## 下一步

- 分支 `fix/display-slider-and-dialog-resize`（6 commits：2 修复 + 1 下限跟进 +
  1 版本/文档 + 1 复核修复 + 1 文档计数）已通过单测 + 实机验证 + 独立复核；
  待创建 PR（→ main），合并由用户负责。
- 后续特性候选（沿用前版结论）：内核直接托管 Web UI —— `serve` 静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程打开 `http://<host>:4096/`
  即用、WS 同源零配置；立项时覆盖 dist 路径发现/`--web-dist`、`/` 与 `/ws` 路由共存、
  token 下静态资源策略、测试与文档。
