# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台六项增强（v0.17.0）—— 代码卡片 + 显示设置 + 文件树图标 + 会话列表菜单 + 项目编辑 + 设置项目面板**
> ——来源=用户反馈六项：① 对话中栏代码输出应以独立卡片呈现（左上展开折叠 + 右上复制）；
> ② 设置提供显示设置（对话/轨迹、左栏、右栏字号独立 + 其他必要项）；③ 文件树为 rust 等
> 源码文件提供图标；④ 左栏清理按钮改为下拉菜单（设置中「会话列表」项迁入 + 悬停自动展开
> 二级菜单选过滤/排序）；⑤ composer 项目下拉每项提供编辑按钮（工作目录、默认项目等）；
> ⑥ 设置弹窗提供项目公共配置。
> 范围：仅 `agentj-ui/apps/web`（Desktop 复用 Web 产物）；**Rust / SDK / protocol 零改动**；
> `PROTOCOL_VERSION` 保持 `0.13.0`；工作区 0.16.5 → 0.17.0。

## v0.17.0: Web 工作台六项增强 ✅（本次完成）

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
- ✅ **门禁全绿（T23 全量，2026-09-19）**：web **401 tests（43 files）** / sdk **46** /
  Rust **564 tests**（0 failed）/ clippy 零警告 / drift **2 passed** / web build /
  tui **71** / check-version **0.17.0** / desktop-ui。新依赖 `rehype-highlight`
  （web workspace + lockfile）；`PROTOCOL_VERSION` 保持 0.13.0。
- ✅ **版本与文档同步**：Cargo workspace + `sdk/src/config.ts VERSION` + desktop 三处
  0.17.0；`AGENTS.md` / `README.md` / 本文件 / 设计 spec TODO 清单（G0-G6 勾选 +
  D4 行高 1.55 与候选来源修订）已同步，提交见 `docs: auto-sync ...`。
- ✅ **最终整支复核修复波 + mini round（2026-09-19）**：Critical C1 = 左栏会话菜单
  `createPortal` 到 `document.body` + `position: fixed`（触发点矩形定位、resize 重算、
  scroll 关闭），逃逸 `.aj-sidebar` / `.aj-sidebar__body` / `.aj-shell__left` 裁剪层级
  （几何裁剪属 jsdom 不可见项，留待实机 Playwright 验收）；Important I1 = 删除 TopbarMenu
  的 dialog 级 Escape `useEffect`，嵌套弹窗仅栈顶关闭；M1 = 项目默认值仅在实际切换时应用
  （同 id 重派发不再覆盖显式 model/agent 选择）；M2 = composer 输入默认字号回到 15px；
  M3 = `.aj-ai pre` legacy padding 收窄为 `:not(.aj-code__body)`（卡片 10 12 生效）；
  M4 = `package-lock.json` desktop workspace 刷新至 0.17.0；mini = 菜单打开时聚焦首个
  `menuitem`（恢复键盘进入，`preventScroll`）。web **401 → 405 tests** / build 通过
  （43 files）。

## 遗留问题 / 遗留 minors

- **实机 Playwright 验收尚未执行**：本次 T23 只跑全量门禁 + 文档同步；六项 + 深浅主题 +
  键盘链路的实机验收在 post-dev `/jskill-test-acceptance` 流程执行（见下一步）。
- **e2e_permission 会话状态污染（本次发现并清理，需长期修复）**：
  `crates/agentj-cli/tests/e2e_permission.rs` 硬编码 `session_id="s1"`，依赖
  `FileBackedStore` 默认目录（`~/.local/share/agentj/sessions`）。一旦 `s1.json` 被
  过期清理标记为 `Archived`，`session/run` 返回 `session 's1' is inactive`，测试在
  `read_json_line` 阻塞（无 `$/permissionRequest`）→ `cargo test --workspace` 挂起。
  本次删除陈旧 `s1.json`/`s1.trajectory` 后 564 tests 全绿。建议后续把测试改为消费
  `session/new` 返回的真实 id（并隔离临时 session 目录），避免跨运行状态依赖。
  该问题在 `e68258b`（分支基线）即存在（`crates/` 与基线逐字节相同），非本特性引入。
- T4 minor（parked）：新增的 modal backdrop 测试无判别力（未断言「下层弹窗 backdrop
  点击不应关闭下层」）；建议补一条。
- T5 parked（plan-mandated，验收视觉确认即可）：紧凑 `.aj-sess` min-height 56 → 48px；
  CSS 横幅注释 >120 字符（仓库既有风格，非门禁）。composer input 默认档 14px 偏差已在
  最终复核修复波修正为 15px（见上）。
- T7 minor（optional）：代码卡片二次点击恢复默认（展开）未加测试；`languageOf` 浅扫描
  已够用。
- T9 minor（optional）：显示面板 3 条测试覆盖建议（非阻塞）。
- T2/T12 minor：`sqlite3`（含版本号的扩展名）映射未单测；db 配色在 T12 已补。
- 观察：移动端抽屉 + 桌面折叠偏好共存沿用 v0.16.4 行为；`.aj-right__body` 逐面板
  内部滚动（双开各 pane 独立 scrollbar）。

## 下一步

- **post-dev `/jskill-test-acceptance`**：实机 Playwright 验收六项（代码卡片复制成功/降级
  路径、显示设置即时生效 + 刷新保持 + 恢复默认、`.rs/.go/.py` code 图标 + `.sql` db 图标、
  左栏菜单清理/过滤/排序飞出、composer 项目编辑 + WS 帧携带 `project_root`、设置「项目」
  列表/删除回退）+ 深浅主题 + 键盘链路；发现 Bug 走 bug-fix 循环，回归通过后
  `/jskill-test-acceptance` 自动创建 PR（用户负责合并）。
- **下一特性候选（沿用 v0.16.5 summary 结论，方案 2）：内核直接托管 Web UI** ——
  `serve` 增加静态文件服务（axum `ServeDir` 托管 `agentj-ui/apps/web/dist`），远程直接
  打开 `http://<host>:4096/` 即用、WS 同源零配置；立项时需覆盖：dist 路径发现/
  `--web-dist` 参数、`/` 与 `/ws` 路由共存、token 下静态资源策略、测试与文档。
