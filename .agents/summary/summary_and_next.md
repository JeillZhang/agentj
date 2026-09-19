# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台六项增强（v0.17.0）→ 验收修复（v0.17.1，BUG-008）—— 已完成并验收通过**
> ——来源=用户反馈六项：① 对话中栏代码输出应以独立卡片呈现（左上展开折叠 + 右上复制）；
> ② 设置提供显示设置（对话/轨迹、左栏、右栏字号独立 + 其他必要项）；③ 文件树为 rust 等
> 源码文件提供图标；④ 左栏清理按钮改为下拉菜单（设置中「会话列表」项迁入 + 悬停自动展开
> 二级菜单选过滤/排序）；⑤ composer 项目下拉每项提供编辑按钮（工作目录、默认项目等）；
> ⑥ 设置弹窗提供项目公共配置。
> 范围：仅 `agentj-ui/apps/web`（Desktop 复用 Web 产物）；**Rust / SDK / protocol 零改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.16.5 → 0.17.0 → 0.17.1。

## v0.17.0: Web 工作台六项增强 + v0.17.1 验收修复 ✅（已完成 / 已验收）

- ✅ **(1) 对话代码块卡片化（G1）**：`RunPanel` 唯一 ReactMarkdown 渲染点接
  `rehypePlugins={[rehypeHighlight]}`（hljs common 语言集）+ `components={{ pre: CodeBlockCard }}`；
  新 `CodeBlockCard`（`figure > figcaption + pre`，header 左 = 折叠/展开 + 语言标签、
  右 = 复制按钮）+ 新 `copyText`（`navigator.clipboard` → textarea/`execCommand` 降级链
  + 失败瞬态，http 局域网非安全上下文可用）；折叠不卸载 body（复制 `textContent` 不受影响），
  流式追加按位置保持折叠态；`.hljs-*` 映射现有语义色变量（不新增 tokens key）；
  行内 code 与 GFM 不变。
- ✅ **(2) 显示设置（G2）**：新 `displayPrefsStore`（`agentj.displayPrefs`：对话/左栏/右栏/代码
  四档字号 小/中/大/特大 + 行高 紧凑 1.4 / 默认 1.55 / 宽松 1.8 + 密度 + 减弱动画，
  字段级枚举校验 + 同 tab `CustomEvent` + 跨 tab `storage`）；新 `SettingsDisplayPanel`
  （设置「显示」tab，改动即时生效 + 「恢复默认」）；新 `displayPrefs.css` 覆盖层
  （最后导入、每个 `var()` 带现值 fallback 保值）；`WorkspaceShell` 注入 inline
  `--aj-font-conv/-left/-right/-code`、`--aj-lh-conv` + `data-density`；
  `App` effect 写 `documentElement[data-reduced-motion]`（镜像主题机制，覆盖 portal）。
- ✅ **(3) 文件树图标（G3）**：`fileIcons.ts` 扩展主流源码扩展名（rs/go/py/java/c/cpp/cs/rb/
  php/kt/swift/scala/lua/dart 等）+ 新 db 类（`Database`）+ 文件名特例
  （Dockerfile/Makefile/CMakeLists.txt → code）+ `.file-icon--db` token 配色；
  分类图标测试扩列。
- ✅ **(4) 会话列表菜单（G4）**：新 `SessionListMenu`（左栏清理按钮改为 ⋯ 菜单：
  `清理所有会话…` 打开既有确认弹窗、无可删禁用；`过滤 ›`/`排序 ›` 悬停向右飞出二级
  `menuitemradio`，镜像 TopbarMenu 的 120ms 延迟/桥接/Esc 分层/键盘，新增
  `.aj-menu__flyout--right`；直写 `sidebarPrefsStore` 即时生效）；`UnifiedSidebar`
  接入，删除旧 `.aj-sidebar__cleanup`；设置弹窗「会话列表」tab 整体移除。
- ✅ **(5) 项目编辑 + `project_root` 打通（G5）**：`projectStore` 扩展可选字段
  `workDir/isDefault/defaultModel/defaultAgent`（零迁移）+ `updateProject`
  （名称去重、`isDefault` 全局互斥、修复 `renameProject` 不通知）/
  `deleteProject`（Inbox 守卫 + 会话归属回退 + 受影响计数）+
  `loadProjectDefaults/saveProjectDefaults`（`agentj.projectDefaults`）+ 回退链；
  新 `ProjectEditDialog`（名称 / 工作目录绝对路径校验 / 默认项目 / 默认模型 / Agent，
  候选同源 `ComposerOptions.MODEL_LIST` / `AGENT_LIST`）+ composer picker 每行编辑按钮；
  `UnifiedSidebar.handleNewSession` 首次携带 `session/new { project_root: workDir }`
  （工作目录打通，后端 v0.13.0 已支持）；项目切换应用默认模型/Agent（
  `agentj:current-project-updated`）。
- ✅ **(6) 设置「项目」面板 + 嵌套弹窗栈（G6）**：新 `SettingsProjectPanel`
  （全局区：默认项目 select + 新项目工作目录预填；项目列表：名称/工作目录/默认徽标/
  编辑/删除，Inbox 不可删、删除确认展示会话回退数）+ `SettingsProjectPanel.css`；
  `SettingsDialog` 分类终为 连接/显示/项目/演示数据；`ModalDialog` 引入模块级模态栈
  （嵌套弹窗仅栈顶响应 Esc/遮罩，现有单层用法行为不变）。
- ✅ **最终整支复核修复波 + mini round（2026-09-19）**：Critical C1 = 左栏会话菜单
  `createPortal` 到 `document.body` + `position: fixed`（触发点矩形定位、resize 重算、
  scroll 关闭），逃逸 `.aj-sidebar` / `.aj-sidebar__body` / `.aj-shell__left` 裁剪层级；
  Important I1 = 删除 TopbarMenu 的 dialog 级 Escape `useEffect`，嵌套弹窗仅栈顶关闭；
  M1 = 项目默认值仅在实际切换时应用；M2 = composer 输入默认字号回到 15px；
  M3 = `.aj-ai pre` legacy padding 收窄为 `:not(.aj-code__body)`；
  M4 = `package-lock.json` desktop workspace 刷新至 0.17.0；mini = 菜单打开时聚焦首个
  `menuitem`。web 401 → 405 tests / build 通过（43 files）。
- ✅ **验收（v0.17.1，实机 Playwright，2026-09-19）**：六项逐项通过 + 深浅主题 + 键盘链路；
  发现并修复 1 个 Bug —— **BUG-008（medium）**：窄视口（375–390px）左栏会话列表菜单
  超出右缘被 `.aj-sidebar`/`.aj-sidebar__body`/`.aj-shell__left` 裁剪，「过滤/排序」二级
  飞出层亦溢出视口不可点。修复（commit `89b3422`）：导出 `clampMenuLeft`
  （菜单左缘夹取到 `[8, innerWidth - menuWidth - 8]`）与 `chooseFlyoutSide`
  （行右缘 + `FLYOUT_GAP(6)` + 飞出宽 > 视口宽时向左展开，`.aj-menu__flyout--left`）；
  `updateMenuPos` 在 resize 时重算菜单位置与飞出方向；桌面 ≥1024 行为不变。
  回归测试 `sessionListMenu.test.tsx`（+139 行，覆盖夹取/方向选择/resize）；验收回归 verified。
- ✅ **门禁全绿（验收后，2026-09-19）**：web **417 tests（43 files）** / sdk **46** /
  Rust **564 tests**（0 failed）/ clippy 零警告 / drift **2 passed** / web build /
  tui **71** / check-version **0.17.1** / desktop-ui。新依赖 `rehype-highlight`
  （web workspace + lockfile）；`PROTOCOL_VERSION` 保持 0.13.0。
- ✅ **版本与文档同步**：Cargo workspace + `sdk/src/config.ts VERSION` + desktop 三处
  0.17.0 → 0.17.1（`Cargo.lock` / `agentj-ui/package-lock.json` 同步重生成）；
  `AGENTS.md` / `README.md` / 本文件已同步；提交
  `chore: bump 0.17.1 + docs sync after v0.17.0 acceptance (BUG-008)`。

## 遗留问题 / 遗留 minors

- **e2e_permission 会话状态污染（预存在，非本特性引入）**：
  `crates/agentj-cli/tests/e2e_permission.rs` 硬编码 `session_id="s1"`，依赖
  `FileBackedStore` 默认目录（`~/.local/share/agentj/sessions`）。一旦 `s1.json` 被
  过期清理标记为 `Archived`，`session/run` 返回 `session 's1' is inactive`，测试在
  `read_json_line` 阻塞（无 `$/permissionRequest`）→ `cargo test --workspace` 挂起。
  该问题在 `e68258b`（分支基线）即存在（`crates/` 与基线逐字节相同）。建议后续把测试
  改为消费 `session/new` 返回的真实 id（并隔离临时 session 目录），避免跨运行状态依赖。
- T4 minor（parked，deferred）：新增的 modal backdrop 测试无判别力（未断言「下层弹窗
  backdrop 点击不应关闭下层」）；建议补一条。
- M5（deferred）：add-dialog 相对路径处理（工作目录路径校验/展示）待补。
- M6（deferred）：picker edit 嵌套交互 + 焦点管理细节待收敛。
- M7（deferred）：其余 minor 测试覆盖缺口（代码卡片二次点击恢复默认、显示面板
  3 条覆盖、`sqlite3` 扩展名映射等）。

## 下一步

- **PR 已创建，待用户合并**：验收通过后 `/jskill-test-acceptance` 已自动创建 PR
  （`feature/web-workbench-enhancement` → main）；合并由用户负责。
- **后续特性候选（沿用 v0.16.5 summary 结论，方案 2）：内核直接托管 Web UI** ——
  `serve` 增加静态文件服务（axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接
  打开 `http://<host>:4096/` 即用、WS 同源零配置；立项时需覆盖：dist 路径发现/
  `--web-dist` 参数、`/` 与 `/ws` 路由共存、token 下静态资源策略、测试与文档。
