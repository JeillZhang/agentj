# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web token 统计修复 v0.18.1（用户直报，纯前端）—— 开发 + 单测 + 实机验证 + Oracle 复核（PASS + 修复波）完成，PR #57 已更新（合并由用户负责）**
> ——来源=用户反馈：① `Σ` 会话 token 读出从 composer 底栏（模型选项旁）移至输入框右上角，
> 紧邻「deepseek-chat」模型 chip 右侧；② token 统计区分输入/输出。
> 范围：仅 `agentj-ui/apps/web`（Desktop 复用 Web 产物）；**Rust / SDK 协议零改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.18.0 → 0.18.1。

## v0.18.1: token 统计位置与拆分修复 ✅（开发 + 单测 + 实机验证 + 复核完成）

- ✅ **(1) 读出移至 composer header**：`RunPanel` 新增 `.aj-composer__header-meta`
  （模型 chip + 用量读出，`margin-left: auto` 右对齐、`flex-wrap` 内部换行兜底）；
  `ComposerOptions` 移除 `usageTotal` prop 与 `.comp-usage`（options.css 同步移除）；
  header 增加 `flex-wrap: wrap; row-gap: 6px`。
- ✅ **(2) 统计拆分输入/输出**：新 `tokenUsage.ts` —— `formatTokenTotal` 从
  ComposerOptions 迁出 + `formatUsageSummary`（`Σ 2.5k · 输入 2k · 输出 500`）+
  `formatUsageTitle`（精确 tooltip/aria-label：输入/输出/合计 `toLocaleString`）。
- ✅ **实机 Playwright 15/15（2026-09-19）**：读出位于 header、chip 右侧 8px、文本
  `Σ 2.5k · 输入 2k · 输出 500`、tooltip/aria-label 精确值 + 合计、底栏无残留、
  390px 换行无重叠/无横向溢出、无控制台错误；截图
  `.playwright-mcp/v0181-usage-header-{1440,390}.png`（临时 mock WS kernel :4097 +
  新 Vite dev :5174；未触碰既有 :4096 内核与 :5173 长驻 dev）。
- ✅ **Oracle 复核 PASS + 修复波**：aria-label 改为完整明细（原仅“本会话累计 token”会隐藏数值）；
  `.aj-composer__header-meta` 内部换行 + `justify-content: flex-end`（长模型名兜底）；
  total 只算一次；补测 4 项（0/0 隐藏、无 model 时读出仍在 header、aria-label 断言、
  CSS 契约扩展 font-size/tabular-nums/flex-wrap）。
- ✅ **门禁**：web **479 tests（46 files）**/ sdk 46 / Rust 564 tests / tui 71 /
  clippy 零警告 / drift 2 passed / web build / check-version **0.18.1** / desktop-ui。
- ✅ **版本与文档同步**：0.18.0 → 0.18.1（Cargo workspace + `Cargo.lock` 17 crates +
  sdk `config.ts`(+测试) + desktop 三处 + `package-lock.json` + `check-version.sh`）；
  `AGENTS.md` / `README.md` 已同步。`PROTOCOL_VERSION` 保持 0.13.0。

## 遗留问题 / 遗留 minors

- 分段独立取整：如 1049+1049 → `Σ 2.1k · 输入 1k · 输出 1k`，分段视觉相加 ≠ 合计 —— 属预期，
  精确值在 tooltip/aria-label（`formatUsageSummary` docstring 已说明）。
- 权限「自动允许」为纯客户端能力（浏览器偏好，收到请求时自动回 allow）；TUI/CLI 不感知；
  内核级会话权限模式（协议扩展 `permission_mode`）为候选增强，未立项。
- 项目选择器下拉（向下展开）在短卡片下仍会被 `.aj-card overflow:hidden` 裁剪 ~92px
  （既有几何，非本版引入；候选修复：改向上展开）。
- e2e_permission 会话状态污染（预存在，非本特性引入）。

## 下一步

- PR **#57**（`feature/composer-float-permission-usage` → main）含 v0.18.0 三项增强 +
  v0.18.1 修复提交；合并由用户负责。
- 后续特性候选（沿用前版结论）：内核直接托管 Web UI —— `serve` 静态文件服务
  （axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程打开 `http://<host>:4096/`
  即用、WS 同源零配置。
