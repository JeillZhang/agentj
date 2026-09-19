# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 三项增强 v0.18.0（用户直报，纯前端）—— 开发 + 单测 + 实机验证 + Oracle 复核（PASS）完成，PR #57 已创建（堆叠在 #56 之上，合并由用户负责）**
> ——来源=用户反馈：① 中栏底部输入框改为中栏 75% 宽、居中浮动于底部，对话内容可
> 延伸至输入框底部、向上滚动到底时内容底部停在输入框顶部；② 底部工具栏「均衡」
> 右侧新增权限选择（如 bypass），由用户选择开放权限避免频繁弹窗；③「deepseek-chat」
> 右侧补充当前会话 token 消耗总量。
> 范围：仅 `agentj-ui/apps/web`（Desktop 复用 Web 产物）；**Rust / SDK 协议零改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.17.2 → 0.18.0。

## v0.18.0: Web 三项增强（composer 浮动 + 权限选择 + token 总量）✅（开发 + 单测 + 实机验证 + 复核完成）

- ✅ **(1) composer 75% 居中浮动（commit `628f471`）**：`composer.css` `.aj-composer-card`
  改 `position:absolute; bottom:16px; left:0; right:0; width:75%; margin:0 auto;
  max-height:calc(100% - 32px)`；`layout.css` 拆分全宽规则（conv 保留 100%，composer
  75% + auto margins，特异性 (0,2,0)）；`conversation.css` `.aj-conv` `padding-bottom` 与
  `scroll-padding-bottom` = `calc(var(--aj-composer-h,152px) + 32px)`；`RunPanel.tsx`
  `useLayoutEffect` + `ResizeObserver`（`borderBoxSize` 全盒高）发布 `--aj-composer-h`；
  窄视口 `min-width: min(340px, calc(100% - 16px))` + `.comp-tools` `flex-wrap: wrap` 兜底。
- ✅ **(2) 权限选择器 + 客户端自动允许**：`layoutStore` `ComposerPermission`
  （ask/bypass，`agentj.composer.permissionMode`，readEnum 校验）；`ComposerOptions`
  第 4 选择器（`pop-perm`，逐次确认/自动允许，跨标签 `storage` 同步，bypass accent 边框）；
  `App.tsx useApproval`：bypass 时对当前会话 `$/permissionRequest` 自动
  `permission/respond allow`（失败 `console.debug`；内核 deny/路径越界仍强制拒绝）。
- ✅ **(3) 会话 token 总量**：`RunPanel` `useMemo` 聚合 usage entries（历史回放 +
  实时逐轮恰好一条）→ `ComposerOptions` 模型触发器右侧 `Σ` 读出（`formatTokenTotal`：
  k/M 一位小数、`.0` 修剪、`< 999.95k` 进位 M；title 输入/输出明细；无用量隐藏）。
- ✅ **实机 Playwright 验证（2026-09-19）**：75.0% 居中（偏差 0）/ 浮底 16px /
  内容穿透（滚动中态元素横跨浮层带）/ 滚底对齐（末条距输入框顶 16px + 8px flex gap）/
  权限选择器交互 + 跨标签双向同步（tab1 bypass → tab0 自动更新，反向同理）/
  1440px 工具栏单行无溢出、390px 两行换行（clientW == scrollW）。
- ✅ **复核修复波（Oracle PASS + 6 minor/nit 全修）**：跨标签 storage 同步 / 自动允许
  失败日志 / `formatTokenTotal` 边界（999999 → `1M`）/ 首帧测量 `useLayoutEffect` /
  RO 参数命名 / docstring；补测 2 项（usage 帧 + run result 双路径单条计数、
  实机式 live-toggle bypass）。
- ✅ **门禁**：web **468 tests（45 files）**/ sdk 46 / Rust 564 tests / tui 71 /
  clippy 零警告 / drift 2 passed / web build / check-version **0.18.0** / desktop-ui。
- ✅ **版本与文档同步**：0.17.2 → 0.18.0（Cargo workspace + `Cargo.lock` 17 crates +
  sdk `config.ts`(+测试) + desktop 三处 + `package-lock.json` + `check-version.sh`）；
  `AGENTS.md` / `README.md` 已同步。`PROTOCOL_VERSION` 保持 0.13.0。

## 遗留问题 / 遗留 minors

- 权限「自动允许」为纯客户端能力（浏览器偏好，收到请求时自动回 allow）；TUI/CLI
  不感知；内核级会话权限模式（协议扩展 `permission_mode`）为候选增强，未立项。
- `formatTokenTotal` 999.95k 进位阈值依赖浮点比较（测试已锁定 999949/999950/999999 边界）。
- 项目选择器下拉（向下展开）在短卡片下仍会被 `.aj-card overflow:hidden` 裁剪 ~92px
  （既有几何，非本版引入；候选修复：改向上展开）。
- e2e_permission 会话状态污染（预存在，非本特性引入）。

## 下一步

- PR **#57**（`feature/composer-float-permission-usage` → main）已创建：v0.18.0 提交 =
  `628f471`（功能 + 测试）+ `6e4c7bc`（bump/docs）；分支其上含 #56 的 v0.17.2 提交
  （先合 #56，本 PR diff 即收敛为 2 commits），合并由用户负责。
- 后续特性候选（沿用前版结论）：内核直接托管 Web UI —— `serve` 静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程打开 `http://<host>:4096/`
  即用、WS 同源零配置。
