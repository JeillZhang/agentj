# 需求提示词（二）：后端协议扩展 —— 文件树 / TODO / 文件变更 / 模型提问作答

> 配套 `design.md`。用于把三栏工作台右栏、左栏「文件」tab 与 composer 提问卡片从**占位数据**升级为**真实数据**。
> 约束：必须遵循 AgentJ 既有协议纪律（加性、`@since`、schema 漂移门禁、稳定错误码），不得破坏既有 16 方法。

## 0. 解锁的 UI 能力

| UI 能力（见 `preview-*.png`） | 依赖的协议能力 |
|---|---|
| 左栏 · 文件 tab：项目目录树（懒加载、折叠、git 状态点） | `file/list`（请求） |
| 右栏 · 任务（TODO）：勾选列表 + 进行中/已完成 | `$/progress` 新增 `kind: "todo"`（通知） |
| 右栏 · 文件变更：变更列表 + 展开 unified diff | `$/progress` 新增 `kind: "file_change"`（通知） |
| 中栏 · 模型提问卡片（选项 + 作答 + 记录） | `$/askQuestion`（通知）+ `question/respond`（请求） |

## 1. 现状缺口（已核实）

- `protocol/methods.json`：16 方法，**无**目录列举 / 文件读取方法。
- `schemas/ProgressValue.json`：`oneOf` 仅 `text | tool_call | tool_result | usage | permission`，**无** TODO / 文件变更。
- **无**「模型提问 / 客户端作答」方法：`$/permissionRequest` 只承载**工具审批**（allow/deny/allow_always），无法表达带选项的结构化提问。
- 因此前端只能从 `tool_call`（`write`/`edit`）**半推导**变更列表，TODO 与提问卡完全无法获得。

## 2. 设计原则（沿用既有决策，不得偏离）

- **加性优先**：`$` 前缀通知的未知 `kind` 按既有约定可被消费者忽略（`ProgressValue` schema 注释 + 规范 §7.3/§9.2），故新增 `kind` 属**加性**变更；不新增错误码，不改既有字段含义。
- **wire 字段一律 snake_case**（规范 §11.2 命名归一化决策）。
- **判别联合**：沿用 `ProgressValue` 的 `kind` 判别模式，而非新建独立通知方法 —— 复用既有 `$/progress` 通道与 `progress_token`、per-session 路由、订阅广播语义（规范 §7）。
- **一请求一响应**：`file/list` 是请求，有严格响应；TODO/变更是有状态推送，走通知。
- **错误码稳定**：新增错误码 MUST 从 `-32800` 段向下（规范 §3.4：`-32807` 起）或 `-32000…-32099` 段内加性分配；已发布含义不可改。
- **安全**：`file/list` 的 `path` MUST 被限制在会话所属项目根内，拒绝路径穿越（`..`、绝对路径逃逸、符号链接逃逸）；越界 → 新增 `-32807 PathNotAllowed`（或复用 `-32602` 并带 `error.data`），须在规范中明确。

## 3. 契约草案

### 3.1 `file/list`（请求，client → server）

params `FileListParams`：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `session_id` | string | 是 | 用于解析项目根与权限归属 |
| `path` | string \| null | 否 | 相对项目根的目录路径；缺省 = 项目根 |
| `depth` | integer \| null | 否 | 列举深度，缺省 1（懒加载用） |

result `FileListResult`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `entries` | `FileEntry[]` | 目录项 |
| `truncated` | boolean | 是否因条目上限被截断（上限须在规范中定值，建议 2000） |

`FileEntry`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | string | 条目名（不含路径分隔符） |
| `path` | string | 相对项目根路径 |
| `kind` | `"file" \| "dir"` | `FileKind` 枚举 |
| `size` | integer \| null | 字节数，dir 为 null |
| `mtime_ms` | integer \| null | 最后修改时间（epoch ms） |
| `git_status` | `"M" \| "A" \| "U" \| "D" \| null` | 对齐 UI 的 M 橙 / A 绿 / U 灰 / D 红；非 git 仓库为 null |

### 3.2 `$/progress` 新增 `kind: "todo"`（通知，server → client）

value `TodoProgress`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `kind` | `"todo"`（const） | 判别符 |
| `session_id` | string | 多会话客户端路由用 |
| `items` | `TodoItem[]` | **全量快照**（非增量），保证客户端幂等覆盖 |
| `revision` | integer | 单调递增；客户端应丢弃比已见 revision 更旧的快照 |

`TodoItem`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | string | 稳定标识 |
| `content` | string | 任务描述 |
| `status` | `"pending" \| "in_progress" \| "completed" \| "cancelled"` | `TodoStatus` 枚举 |

> 未知 `status` 按 §9.2 当作 `pending` 渲染，不报错。

### 3.3 `$/progress` 新增 `kind: "file_change"`（通知，server → client）

value `FileChangeProgress`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `kind` | `"file_change"`（const） | 判别符 |
| `session_id` | string | 路由 |
| `change` | `FileChange` | 单个文件的一次变更事件 |

`FileChange`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `path` | string | 相对项目根路径 |
| `op` | `"add" \| "modify" \| "delete"` | `FileChangeKind` 枚举 |
| `additions` | integer | 新增行数 |
| `deletions` | integer | 删除行数 |
| `tool_call_id` | string \| null | 触发它的工具调用，便于前端把变更挂到对应工具卡 |
| `diff` | string \| null | unified diff 文本；可为 null（仅计数） |

> 大 diff 截断策略须在规范中定值（建议单条 `diff` ≤ 64 KiB，超出置 null 并附 `diff_truncated: true`）。

### 3.4 `$/askQuestion`（通知，server → client）+ `question/respond`（请求，client → server）

`$/askQuestion` params `AskQuestionParams`：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `question_id` | string | 是 | 回答关联标识 |
| `session_id` | string | 是 | 多会话客户端路由用 |
| `question` | string | 是 | 问题正文 |
| `header` | string \| null | 否 | 短标题（建议 ≤ 12 字符） |
| `options` | `AskOption[]` | 是 | 2–8 项 |
| `allow_multiple` | boolean | 否 | 缺省 `false`（单选） |
| `allow_other` | boolean | 否 | 缺省 `true`（显示「其他…」自由输入） |
| `timeout_s` | integer \| null | 否 | 缺省 `null`（不超时）；超时按 skip 结算 |

`AskOption`：`{ id: string, label: string, hint?: string | null }`

`question/respond` params `QuestionRespondParams`：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `question_id` | string | 是 | 对应 `$/askQuestion` |
| `answers` | `string[]` | 是 | 选中的 `option.id`；`allow_other` 时自由文本以 `other:` 前缀承载 |
| `skipped` | boolean | 否 | 缺省 `false`；`true` 表示跳过 |

result `QuestionRespondResult`：`{ accepted: boolean }`

规则：
- **归属**：定向发送给发起该 run 的 owner 连接（复用规范 §4.5 权限归属语义）；非 owner 响应 → `-32806 NotAuthorized`。
- **幂等**：同一 `question_id` 只接受首次有效响应，重复响应返回 `accepted: false`（不报错）。
- **结算**：run 取消 / 会话终止 → 未答提问自动按 skip 结算；`skipped: true` 表示模型自行决定继续。
- **兼容**：不支持该通知的旧客户端因 `$` 前缀约定可忽略，属**加性**扩展。

## 4. 版本与门禁（MUST）

1. **单一版本来源**：`agentj-ui/packages/sdk/src/config.ts` 的 `PROTOCOL_VERSION` 是协议版本单一来源（README 明确）；按其策略抬升（本次为加性 → minor）。
2. `schemas/*.json` 由 `schemars` 生成：改 Rust wire 类型后运行 `cargo run -p agentj-protocol --bin gen_schemas`，然后 `cargo test -p agentj-protocol -- drift` 必须零漂移。
3. `protocol/methods.json` 注册新方法：`file/list`（`request` / `client_to_server` / result `FileListResult`）、`question/respond`（`request` / `client_to_server` / result `QuestionRespondResult`）、`$/askQuestion`（`notification` / `server_to_client` / params `AskQuestionParams`）。
4. `docs/specs/ipc-protocol.md`：§8 方法面新增 `8.18 file/list`、`8.19 $/askQuestion` + `8.20 question/respond`（LSP 六段式：能力声明 → 请求 → 响应 → 部分结果 → error.data），§7.3 更新 `ProgressValue` 判别表，§9.4 标注 `@since`，§11.3 变更日志新增条目。
5. TS 绑定：`npm --prefix agentj-ui run generate --workspace=@agentj/protocol` + `tsc`。
6. `./scripts/check-version.sh` 必须通过（Cargo.toml / sdk config.ts / desktop 版本一致）。
7. `cargo clippy --workspace --all-targets -- -D warnings` 零告警。

## 5. 内核实现要点

- **发射点**：TODO 与 file_change 由编排层（`TurnRunner` / 工具执行链）在状态变更时经 `EventBus` 发布，`$/progress` 通知按 `session/subscribe` 的订阅者集合广播（沿用 §4.5 的 per-session 路由与 owner 语义）。
- **工具接入**：`write` / `edit` / `bash` 等产生文件副作用的工具，在结果结算时发出 `file_change`；行数由写前/写后内容 diff 计算。
- **TODO 来源**：可从 agent 的计划文本解析或由插件/子代理显式上报；内核只负责承载与广播，不强制语义。
- **重入防护**：新增发射必须遵守既有 `EventGuard` 深度上限（MAX_EVENT_DEPTH=16），避免在事件回调里再触发事件导致的递归。
- **权限**：`file/list` 受 `[permission]` 现有规则体系约束；只读列举默认 `allow`，越界路径默认 `deny`。

## 6. 前端接入要点

- `packages/sdk`：`parseProgress` 扩展识别 `todo` / `file_change`；未知 `kind` 仍静默忽略（保持向后兼容）。
- `apps/web`：右栏 `TodoPanel` / `ChangeList` 从 `$/progress` 流式帧驱动；`FileTree` 调 `file/list` 懒加载；`AskQuestionCard` 由 `$/askQuestion` 驱动、作答经 `question/respond`；`session/history` 回放时，历史事件中的这些帧同样参与重建（含问答记录）。
- TUI 同步：`apps/tui` 至少不因新 `kind` 崩溃（未知即忽略），可选补齐 TODO 展示。

## 7. 验收

1. `file/list` 能列举项目根与子目录；越界路径被拒且错误码/`error.data` 符合规范。
2. 一次 `session/run` 中，TODO 变化与文件变更分别经 `$/progress` 到达订阅客户端；`revision` 单调。
3. 未知 `todo.status` / 未知 `kind` 时客户端不崩溃、按约定降级。
4. `$/askQuestion` → `question/respond` 往返可用；非 owner 响应被拒（`-32806`）；重复响应幂等返回 `accepted:false`；run 取消后未答提问按 skip 结算。
5. schema 漂移门禁、TS 生成 + tsc、clippy、check-version 全绿。
6. 协议规范文档与新 schema/methods.json 一致（人工核对 §8.18–§8.20 与 §7.3）。

---

## 8. 提示词（复制即用）

```text
# 需求：AgentJ IPC 协议扩展 —— file/list + $/progress 的 todo / file_change + $/askQuestion / question/respond

## 背景
AgentJ 是 Rust 内核 + JSON-RPC 2.0 协议的 AI 编码代理。
协议契约来源：
  - schemas/*.json（33 个，schemars 生成，提交件 + 漂移门禁）
  - protocol/methods.json（16 方法注册表）
  - docs/specs/ipc-protocol.md（人类可读规范，11 节，LSP 六段式）
  - agentj-ui/packages/sdk/src/config.ts 的 PROTOCOL_VERSION 为版本单一来源
  - agentj-ui/packages/protocol 由 schemas 生成 TS 类型
现有 $/progress 通过 ProgressValue 的 kind 判别：text | tool_call | tool_result | usage | permission。
$ 前缀通知的未知 kind 可被消费者忽略（规范 §7.3/§9.2），故新增 kind 是加性变更。

## 目标
让前端三栏工作台的右栏（TODO / 文件变更）、左栏「文件」tab 与 composer 提问卡片拿到真实数据。

## 要做
1) 新增请求方法 file/list
   - params FileListParams: session_id(必填), path(可选，相对项目根，缺省=根), depth(可选，缺省 1)
   - result FileListResult: entries: FileEntry[], truncated: boolean
   - FileEntry: name, path(相对项目根), kind("file"|"dir"), size(int|null), mtime_ms(int|null),
                git_status("M"|"A"|"U"|"D"|null)
   - 安全：path 必须限制在会话所属项目根内，拒绝路径穿越(/.. 、绝对路径逃逸、符号链接逃逸)；
     越界返回新的稳定错误码 -32807 PathNotAllowed（在规范 §3.4 登记，附 error.data）
   - 条目上限 2000，超出置 truncated=true
2) 扩展 ProgressValue(oneOf)，新增两个 kind（加性）：
   - kind:"todo"  → { kind, session_id, revision:int(单调递增), items: TodoItem[]（全量快照） }
                    TodoItem = { id, content, status: "pending"|"in_progress"|"completed"|"cancelled" }
                    未知 status 按 pending 降级
   - kind:"file_change" → { kind, session_id, change: FileChange }
                    FileChange = { path, op: "add"|"modify"|"delete", additions:int, deletions:int,
                                   tool_call_id: string|null, diff: string|null }
                    单条 diff ≤ 64 KiB，超出置 null 并附 diff_truncated:true
3) 新增模型提问 / 作答能力
   - 通知 $/askQuestion（server_to_client） params AskQuestionParams:
     { question_id, session_id, question, header?, options: AskOption[],
       allow_multiple?(缺省 false), allow_other?(缺省 true), timeout_s?(缺省 null) }
     AskOption = { id, label, hint? }；options 2–8 项
   - 请求 question/respond（client_to_server） params QuestionRespondParams:
     { question_id, answers: string[], skipped?: bool } → result { accepted: boolean }
     自由文本回答以 "other:" 前缀承载
   - 定向发给发起该 run 的 owner 连接；非 owner 响应 → -32806 NotAuthorized
   - 同一 question_id 只接受首次有效响应，重复响应幂等返回 accepted:false（不报错）
   - run 取消 / 会话终止 → 未答提问按 skip 结算

## 协议纪律（必须遵守）
- wire 字段一律 snake_case
- 加性变更，不改既有字段含义；新增字段/类型标注 @since
- 新错误码从 -32800 段向下分配（-32807），已发布错误码含义不可更改
- 沿用 /progress 通道与 progress_token、per-session 路由、session/subscribe 广播语义；
  不要新建独立的 todo/change 通知方法

## 交付与门禁
1. 改 Rust wire 类型（agentj-protocol crate）→ cargo run -p agentj-protocol --bin gen_schemas
   → cargo test -p agentj-protocol -- drift 零漂移
2. protocol/methods.json 注册 file/list（request / client_to_server / result FileListResult）、
   question/respond（request / client_to_server / result QuestionRespondResult）、
   $/askQuestion（notification / server_to_client / params AskQuestionParams）
3. docs/specs/ipc-protocol.md：新增 §8.18 file/list、§8.19 $/askQuestion、§8.20 question/respond
   （LSP 六段式：能力声明→请求→响应→部分结果→error.data）；
   更新 §7.3 ProgressValue 判别表；§3.4 登记 -32807；§9.4 标 @since；§11.3 变更日志加条目
4. 抬升 PROTOCOL_VERSION（加性 → minor，改 agentj-ui/packages/sdk/src/config.ts）
5. npm --prefix agentj-ui run generate --workspace=@agentj/protocol 后 tsc 通过
6. ./scripts/check-version.sh 通过；cargo clippy --workspace --all-targets -- -D warnings 零告警

## 内核实现
- TODO 与 file_change 由编排层（TurnRunner / 工具执行链）在状态变化时经 EventBus 发布，
  按 session/subscribe 订阅者集合广播（沿用 per-session 路由与 owner 语义）
- write/edit/bash 等有文件副作用的工具在结果结算时发 file_change，行数由写前/写后 diff 计算
- TODO 来源不强制语义：可来自计划文本解析或插件/子代理显式上报，内核只承载与广播
- 新发射必须遵守既有 EventGuard 重入防护（MAX_EVENT_DEPTH=16），禁止事件回调内递归触发
- file/list 受现有 [permission] 规则约束：只读列举默认 allow，越界默认 deny
- $/askQuestion 由编排层在需要澄清时发出（携带 2–8 个选项），服务端等待 question/respond；
  与 $/permissionRequest 同属“阻塞式交互”，但走独立通道与独立归属校验：
  提问不占用 permission 的 60s 审批语义，超时按 timeout_s（缺省不超时），取消按 skip 结算

## 前端接入
- packages/sdk：parseProgress 识别 todo / file_change；未知 kind 仍静默忽略（保持向后兼容）
- apps/web：TodoPanel / ChangeList 由 $/progress 帧驱动；FileTree 调 file/list 懒加载；
  AskQuestionCard 由 $/askQuestion 驱动、作答经 question/respond；
  session/history 回放时同样重建这些状态（含问答记录）
- apps/tui：至少不因新 kind / 新通知崩溃（未知即忽略），可选补齐 TODO 展示

## 不做
- todo / file_change 不新增顶层通知方法（复用 $/progress 的 kind 判别）
- 不改既有 16 方法的字段含义；提问能力不塞进 permission 通道
- 不引入新的第三方依赖

## 验收
1. file/list 可列举根与子目录；越界路径被拒且错误码/error.data 符合规范
2. 一次 session/run 中 TODO 变化与文件变更分别经 $/progress 到达订阅客户端；revision 单调
3. 未知 todo.status / 未知 kind 时客户端不崩溃并降级
4. $/askQuestion → question/respond 往返可用；非 owner 响应被拒（-32806）；
   重复响应幂等返回 accepted:false；run 取消后未答提问按 skip 结算
5. schema 漂移门禁、TS 生成 + tsc、clippy、check-version 全绿
6. 规范文档与 schema/methods.json 一致（人工核对 §8.18–§8.20 与 §7.3）
```

## 9. 与界面设计的关系

本文档只定义**数据契约**，不改变 `design.md` 的布局与交互规格。落地顺序建议：
先按 `design.md` 用占位数据交付三栏 UI（可验收），再按本文档替换数据源；两段解耦，互不阻塞。
