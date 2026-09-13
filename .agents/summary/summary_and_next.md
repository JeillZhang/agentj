# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**协议扩展（v0.15.0）—— `file/list` + `$/progress` todo/file_change + `$/askQuestion` / `question/respond`（三栏工作台真实数据源）**
> ——设计文档 `docs/superpowers/specs/2026-09-13-agentj-protocol-extension-design.md`
> + 实现计划 `.superpowers/sdd/2026-09-13-agentj-protocol-extension-plan/`
> （T0-T42 全部完成并提交，分支 `feature/agentj-protocol-extension`；工作区 0.14.6 → 0.15.0，
> PROTOCOL_VERSION 0.12.1 → 0.13.0，16 → 17 crates，21 方法 / 51 schemas）

## v0.15.0: Protocol Extension ✅ (本次完成)

- ✅ **T0-T2 协议 wire 类型 + 事件**：`FileListParams`/`FileListResult`、`FileEntry`/`FileKind`、`TodoItem`/`TodoStatus`、`FileChange`/`FileChangeKind`、`AskQuestionParams`/`AskOption`、`QuestionRespondParams`/`QuestionRespondResult`、`SessionNewParams`、新错误码 `-32807 PathNotAllowed`；`ProgressValue` 加 `todo`/`file_change` 分支，`TrajectoryValue` 加 `todo`/`file_change`/`ask_question`/`question_answer`（`@since 0.13.0`，加性纪律）。
- ✅ **T3/T17-T24 后端**：schemas 38 → 51；`Tool::execute_with_context` + `ToolCallContext`（会话根上下文，存量工具 default 委托零改动）；`AgentEvent::Todo`/`FileChange` → `dispatch_session_run` 统一转帧/广播/录迹；`todo` 内置工具（`register_builtins` 5 → 6）；新 crate `agentj-question`（`QuestionModule` + `AskTool`，`ask_blocking` 阻塞、respond 幂等 `accepted:false`、`timeout_s` 可空、notifier/settle hook 一次结算）；`file/list` dispatch（沙箱 + git 状态点 + 2000 条截断）；`question/respond` owner 门控（非 owner `-32806`）；`session/new` `project_root` 校验与持久化；`session/history` 回放重建 TODO/变更/问答。
- ✅ **T13/T14/T16 file_change 产出**：write 前读/后读 + 自研行级 LCS diff（≤64 KiB，超限 `diff:null` + `diff_truncated`）；bash git 前后采样（`rev-parse` + `status --porcelain -z --no-renames` + `diff HEAD --numstat -z`；tracked 走 `git diff HEAD`、untracked 仅计数；单次 ≤64 条）。
- ✅ **T25/T26 E2E**：mock-workbench provider（stdio `file/list` + todo/file_change 帧断言）；mock-ask provider（TCP 提问往返）。
- ✅ **T27-T37 前端**：SDK `parseProgress` 新 kind + 未知容忍 + `METHODS` 常量；`todoState` revision 门控 reducer、`changes` file_change upsert；`FileTree` `file/list` 懒加载 + git 点；`TodoPanel`/`ChangeList` 真实数据；`useAskQuestion`/`AskQuestionCard` → `question/respond`；回放重建 TODO/变更/Q&A；TUI runview 只读 TODO 区块；`deriveChanges` 半推导退役，demo 保留为整块覆盖的预览通道（demo 开 = fixtures，demo 关 = 真实流）。
- ✅ **T38 规范文档**：`docs/specs/ipc-protocol.md` 同步 0.13.0（21 方法 / 51 schemas / §8.18-§8.22 新方法小节 / §7.3 七个 progress kind / `-32807` / §11.3 changelog）。
- ✅ **T39-T41 版本门禁**：Cargo workspace + SDK `VERSION` + Desktop 壳 0.15.0；`PROTOCOL_VERSION` + `protocol/methods.json` 0.13.0；`check-version.sh` 扩展（含 methods.json 版本检查）。
- ✅ **T42 全量门禁 + 文档同步**：全部门禁通过（见下），`AGENTS.md` / `README.md` / 本 summary / 设计文档状态同步至 v0.15.0 / 0.13.0。

## 验证结果（T42 全量门禁，逐条记录）

1. `cargo fmt --check` — ✅ 通过（exit 0）。
2. `cargo test --workspace` — ✅ 545 passed / 0 failed（含 e2e_cli 4 / e2e_permission 1（60.24s）/ e2e_serve 3 / e2e_ws 1 / e2e_license 6 / e2e_mcp 4 / contract 14 / registry 4 / ws_classifier 4）。
3. `cargo clippy --workspace --all-targets -- -D warnings` — ✅ exit 0（零警告）。
4. `cargo test -p agentj-protocol -- drift` — ✅ 2 passed（`drift_registered_types_are_known` / `drift_committed_schemas_match_generated`，51 schemas 无漂移）。
5. `npm --prefix agentj-ui test --workspace=@agentj/sdk` — ✅ 6 files / 37 tests passed。
6. `npm --prefix agentj-ui test --workspace=@agentj/web` — ✅ 30 files / 219 tests passed。
7. `npm --prefix agentj-ui run build --workspace=@agentj/web` — ✅ `tsc -b && vite build`（1858 modules，dist 产出）。
8. `bun --cwd agentj-ui/apps/tui test` — ✅ 71 pass / 0 fail（14 files / 183 expect）。
9. `./scripts/check-version.sh` — ✅ `version-ok`（Cargo 0.15.0 + sdk VERSION 0.15.0 + PROTOCOL_VERSION 0.13.0 + methods.json 0.13.0 + desktop 0.15.0）。
10. `node agentj-ui/apps/desktop/check-desktop-ui.mjs` — ✅ `desktop-ui-ok`。

## 遗留问题 / 已知限制

- **T7 遗留 → final review 已修复（2026-09-14）**：glob 重定基会让 `find -name` 对 `*.rs` 等模式静默返回空（回归），已改为 **glob pattern 不再改写**、`GlobTool`/`GrepTool` 以会话根为 cwd 执行（新增回归测试）；含 `/` 的 glob 模式仍不支持（`find -name` basename 语义），见设计文档「实现已知限制（v0.15.0）」。
- **TODO revision 仅进程内单调（final review 记录，延迟项）**：守护进程重启后 revision 从 1 重新计数，而客户端以 `session/history` 最大 revision 门控 → 重启后早期帧被丢弃；正解 = 按会话持久化/播种 revision，推迟下一迭代。
- **SDK `ResponseRouter` 通知单槽（final review 记录，延迟项）**：Web `App.tsx` / TUI `permission.tsx` 以 monkey-patch `RpcClient.prototype` 扇出多处理器（功能可用、有测试）；原生多处理器路由推迟。
- **T35 遗留（minor）**：`RunPanel` 的 `pendingQuestion` 在 `activeSessionId` 切换 / demo 开关时不清理，可能残留上一条会话的提问卡（会话切换场景）。
- **Desktop `src-tauri/Cargo.lock` 历史遗留**：仍停留在旧版本 `agentj-ui 0.11.0`（非本次 bump 范围）。
- **E2E 本地预存在风险（环境性）**：`e2e_permission` 硬编码会话 id `s1` 且未设置 `[session] dir`，会写 `$HOME/.local/share/agentj/sessions/s1.json`；若该文件处于过期/归档态，本地运行会挂起（CI 全新 HOME 不受影响）。本次 T42 门禁工作区干净、测试 60.24s 内正常通过；如遇本地挂起，先清理该残留文件。
- 协议观察项（非本特性范围）：`session/status` 对未知会话返回 `-32601`；protocol-version banner 与 `PROTOCOL_VERSION` 展示一致性。

## 下一步

- push 分支 `feature/agentj-protocol-extension` + 创建 PR（用户负责合并）。
- 合并后打 `v0.15.0` tag 并清理 acceptance 产物与临时文件。
