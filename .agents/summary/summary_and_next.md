# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**agentj-ui 三栏工作台（0.14.0）—— Sessions/Files 左栏 + 会话中栏 + 运行详情右栏 + composer 模型/Agent/熵变选择器 + 模型提问卡片 + demo 模式**
> ——设计文档 `docs/superpowers/specs/2026-09-13-agentj-three-column-workbench-design.md`
> + 实现计划 `.superpowers/sdd/2026-09-13-agentj-three-column-workbench-plan/`
> （T0-T28，后端零改造，纯 `agentj-ui` 增量，0.13.0 → 0.14.0 minor）

## 进度与摘要

- ✅ **T0-T28 全部完成并提交**（分支 `feature/agentj-three-column-workbench`）。
- ✅ **T0-T2 数据与 tokens**：`palette.ts` 24 keys（radiusCtl/radiusCard/accentTint/diffAddTint/diffDelTint）+ `LAYOUT` 常量块；`deriveChanges` 从 write tool_call 派生变更行；`demoData`（agentj.demo / ?demo=1）驱动 demo。
- ✅ **T1-T7 布局原语**：`layoutStore`（宽度/折叠/持久化）+ `SplitPane`（拖拽/键盘/双击/separator a11y）+ `demoData` + `index.css` tokens 对齐 + radius 迁移 batch A/B + `TopbarMenu`。
- ✅ **T8-T24 组件与接线**：`TodoPanel` + `ChangeList`（unified diff 展开）+ `FileTree`（懒展开 + git 状态点）+ `WorkspaceShell` 三栏 + `RightPanel`（折叠导轨）+ `SidebarTabs` + `UnifiedSidebar` 重构（最近会话平铺 + 项目分组）+ `OptionPopover` + `ComposerOptions`（模型/Agent/熵变互斥 + 持久化）+ `AskQuestionCard`（折叠 + 问答记录）+ `App.tsx` 集成 + `<1024` 右抽屉 + a11y 门禁。
- ✅ **T25-T27 版本同步**：Cargo workspace + SDK `VERSION` + Desktop 壳 + `check-version.sh` 全部 0.14.0（`PROTOCOL_VERSION` 保持 0.12.1）。
- ✅ **T28 全量门禁 + 文档同步**：本文件 + `AGENTS.md` + `README.md` + spec TODO 刷新（详见验证结果）。

## 验证结果（T28 全量门禁，逐条记录）

1. `npm --prefix agentj-ui test --workspace=@agentj/web` — ✅ Test Files 27 passed (27)、Tests 164 passed (164)。
2. `npm --prefix agentj-ui run build --workspace=@agentj/web` — ✅ `tsc -b && vite build` 通过（1856 modules，index js 428.91 kB / gzip 129.81 kB，361ms）。
3. `npm --prefix agentj-ui test --workspace=@agentj/sdk` — ✅ 6 files、30 tests passed。
4. `cargo test --workspace -- --skip permission_request_notified` — ⚠️ 首次全量运行命中已知 flake `e2e_serve_exits_gracefully_on_sigterm`（并行负载下超时，非回归）；隔离重跑通过后全量重跑 ✅ 全绿（EXIT 0，所有 test result ok）。
5. `cargo clippy --workspace --all-targets -- -D warnings` — ✅ Finished，零警告（exit 0）。
6. `cargo test -p agentj-protocol -- drift` — ✅ 2 passed（33 schemas 无漂移）。
7. `bun --cwd agentj-ui/apps/tui test` — ✅ 69 pass / 0 fail / 176 expect() / 14 files。
8. `./scripts/check-version.sh` — ✅ `version-ok`（Cargo + sdk + desktop 均 0.14.0，PROTOCOL_VERSION 0.12.1）。

## 遗留问题 / 已知限制

- **协议 gap（阶段二待协议支持）**：无 TODO 事件、无文件 diff 事件、无目录列举方法 → 阶段一以 demo/空态呈现（`TodoPanel`/`FileTree` 显示「待协议支持」，`ChangeList` 仅渲染真实派生数据，不伪造 `-N`/diff）。
- `$/askQuestion` + `question/respond` 未实现，`AskQuestionCard` 当前仅由 demo 驱动。
- T23 后「继续会话」不再自动收藏该会话，且连接失败时缺少错误反馈。
- `index.css` 中 `--aj-content-max`/`--aj-content-fluid` 变量已无引用（清理项）。
- demo fixture 数量与 mockup 存在少量不一致（如文件树 footer 计数/展开层级）。
- 已知 flake：`e2e_serve_exits_gracefully_on_sigterm` 在全量并行负载下偶发失败，隔离运行可过（历史问题，非本次回归）。

## 下一步

- 运行 `/jskill-test-acceptance` 自动验收（测试 agent 发现 Bug → Bug-fix 修复 → 回归）。
- 验收 PASS 后自动创建 PR（用户负责合并），合并后打 `v0.14.0` tag 并清理 acceptance 产物。
- 阶段二（独立 feature）：按 `protocol-extension-prompt.md` 落地 TODO 事件 / 文件变更事件 / `file/list` / `$/askQuestion` + `question/respond`，替换 demo 数据源。
