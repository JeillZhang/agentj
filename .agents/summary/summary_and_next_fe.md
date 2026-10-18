# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**对话工具调用卡片可读性**（工具名旁显示命令 / 文件 / 模式 + 卡片铺满中栏），
> 本阶段已完成（TDD RED→GREEN，Web **666 passed**）。版本按约定保持 **0.1.0** 不递增；
> `PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas（**无协议变更**，纯前端）。

## 工具调用卡片可读性（工具名旁显示调用目标 + 铺满中栏）✅

### 需求

用户（m00002）：对话时模型执行 bash 的「结果卡片」，顶部 `bash` 命令旁把命令显示出来；
读取文件等也要显示读了什么文件；卡片宽度调整为中间栏宽度。

### 变更

- `agentj-ui/apps/web/src/RunPanel.tsx`：新增导出 `toolSummary(entry)` —— 按 `entry.name`
  从 `entry.args` 取一行目标：`bash`→`command`、`read` / `write`→`path`、`glob`→`pattern`、
  `grep`→`pattern`（`include` 存在且非 `*` 时拼 `"pattern · include"`）、`todo`→`"N 项"`；
  `args` 为 null / 非对象或工具未知时返回 `""`。`ToolCard` 在 `.aj-tool-name` 之后渲染
  `<span className="aj-tool-summary" title={summary}>`，仅当 summary 非空。
- `agentj-ui/apps/web/src/components/conversation.css`：`.aj-tool-card` 的
  `align-self: flex-start` → `align-self: stretch` + `width: 100%`（铺满 `.aj-conv` 中栏宽度）；
  新增 `.aj-tool-summary`（`flex: 1 1 0` / `min-width: 0` / `white-space: nowrap` /
  `text-overflow: ellipsis` / `font-family: var(--aj-mono)` / `color: var(--aj-textMuted)`）。
- `agentj-ui/apps/web/src/components/displayPrefs.css`：`.aj-tool-summary` 字号跟随
  `var(--aj-font-code, 12px)`，与 `.aj-tool-output` 一致。

### 测试（RED → GREEN）

| 测试 | 文件 |
|---|---|
| `shows the bash command beside the tool name` | `agentj-ui/apps/web/src/RunPanel.test.tsx` |
| `shows the target path for read and write tools` | 同上 |
| `summarizes grep as pattern + include and glob as pattern` | 同上 |
| `omits the summary when args are absent or the tool is unknown` | 同上 |
| `stretches the tool card across the conversation column` | `agentj-ui/apps/web/src/components/toolCardCss.test.ts` |
| `truncates a long command summary instead of wrapping` | 同上 |

RED 证据：实现前 6 条断言全部失败（RunPanel 4 条 + CSS 2 条），实现后全绿。

### 验证结果

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **666 passed**（54 files；660 → +6） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（仅既有 chunk > 500 kB 提示） |
| `cargo fmt --all -- --check` | clean |
| `cargo test --workspace` | 全绿（本阶段未改后端） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `./scripts/check-version.sh` | **version-ok（0.1.0 / 0.18.0）** |

## 遗留问题 / 已知限制（前端）

- **`packages/sdk` 版本断言的守护范围**：`config.test.ts` 现断言「`VERSION` == 仓库根
  `Cargo.toml` 版本」，可防 bump 遗漏；但 desktop 三清单 / `agentj-ui/package-lock.json`
  仍需 `scripts/check-version.sh` 覆盖（后者当前**未被覆盖**，其根条目仍记着陈旧的 `0.29.1`，
  因 `agentj-ui/package.json` 无 `version` 字段，npm 侧无实际影响）。
- **`sessionBusyMessage` 用文本匹配**：SDK 抛错只保留 `message` 文本（未附 `code`），故用
  `includes("-32805")`；若后续给 SDK `Error` 加 `code` 字段，应改为结构化判断。
- **无 `session/unsubscribe`**：会话切换后旧会话的订阅仍在（服务端按连接归属，重连即随连接
  释放）；当前不做前端侧退订。
- **Web 连接为手动重连**：「设置 · 连接」点 `Connect` 才重建 transport/client（`connectSeq`），
  没有自动重连/退避；只保证「重连后重新订阅」。
- **标签只能显示「我们请求的」模型**：内核 `session/run` 不回显生效模型（见 `_se.md` 待实施 ⑧），
  上游把请求静默改路由时 UI 无从得知；当前把标签与 request 参数绑在同一处取值，只能保证
  「显示 == 我们发出的」。
- **工具摘要只覆盖已知工具**：`toolSummary` 对未识别的 `name` 返回空串（不显示摘要），
  MCP / 插件工具若不带已知字段则不显示目标；如需通用回退（如 `path`/`pattern`/`command`
  任意键）可后续再议。
- **`cmd` / `command` 字段名不一致（既有）**：`toolSummary` 以规范键 `command` 取值
  （`crates/agentj-tools/src/builtins.rs` 的 JSON Schema 与内核文本工具调用还原均用 `command`）；
  但 `crates/agentj-protocol/src/permission.rs` / `schemas/PermissionRequestParams.json` 的文档与
  Web `ApprovalDialog.tsx:22` 标签仍写 `cmd`。若将来把权限原始 args 直接喂给工具卡片，摘要会静默为空；
  属独立遗留项，与本阶段无关。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本阶段改动待提交 → 推送**两个远端**（本机 `github` = GitHub 权威、`origin` = GitCode 镜像；
  与 AGENTS.md 的远端命名相反）→ 在 GitHub 建 PR（用户负责合并）。
- 前端候选：③ Web 接入服务端搜索；或订阅状态常驻指示（需先定义新鲜度语义）；
  或清理 `TopbarMenu` / `SettingsDialog` 测试里的重复 stub helper；
  或 TUI `/models` 与 Web 选择语义对齐（TUI 仍是内存态、不跨进程持久化）。
- 后端候选见 `summary_and_next_se.md`。
