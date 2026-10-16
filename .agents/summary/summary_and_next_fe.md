# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.6 断连重连后的 `SessionBusy`（前端部分）**
> ——来源：用户报告「断连后，重新连接会话，任务中断，提示继续后，提示 sessionbusy」；
> 后端部分（owner 断连结算待审批）见 `summary_and_next_se.md`。
> 前端范围：`agentj-ui/apps/web/src/{App.tsx,App.test.tsx,RunPanel.tsx,RunPanel.test.tsx}`
> + `agentj-ui/packages/sdk` 两处陈旧版本断言的硬化。
> 工作区 0.29.5 → **0.29.6**（纯修复）；`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 /
> 78 schemas（无协议变更、无类型重生成、无 METHODS 变更）。

## v0.29.6（Web 重连即重新订阅；`SessionBusy` 友好提示）🚧

### 缺陷与根因

`docs/specs/ipc-protocol.md` 异常断开一节约定：断线客户端在内核里的 run **继续执行**，
重连者用 `session/subscribe` **观测**而非重试 `session/run`。Web 侧只做到前半句：

| 位置 | 缺陷 |
|---|---|
| `agentj-ui/apps/web/src/App.tsx` | 仅在**选中会话时**订阅（`setActiveSessionIdAndHydrate` → `session/status` + `session/subscribe`）；内核断开/重连后**不会**对当前会话重新订阅，仍在运行的 run 在界面上表现为「任务中断」 |
| `agentj-ui/apps/web/src/RunPanel.tsx` | `session/run` 失败时把 SDK 抛出的原文（`${code}: ${message}`，如 `-32805: SessionBusy`）直接显示在运行卡片上 |

对照：TUI 早已实现该模式（`apps/tui/src/connection.ts` 在 open 时 `resubscribe()`，
`connection.test.ts` 有对应用例）。

### 修复

- `agentj-ui/apps/web/src/App.tsx`：新增 effect（依赖 `[sharedClient, activeSessionId]`）——
  客户端（重）连通或切换会话时重新 `session/subscribe` 当前会话，重新接上仍在运行的
  `$/progress` 输出流；失败静默（`.catch(() => {})`），与既有订阅路径一致。
- `agentj-ui/apps/web/src/RunPanel.tsx`：新增导出 `sessionBusyMessage(text: string): string | null`
  （命中 `-32805` / `SessionBusy` 时返回友好文案，否则 `null`，与既有 `isQuotaLimitError` 同风格）；
  `submit` 的 catch 改为 `failRun(prev, sessionBusyMessage(message) ?? message)`。
- 未改 `@agentj/sdk`（未给抛出的 `Error` 附加 `code`）——保持前端单侧小改动，沿用文本匹配先例。

### 测试（RED → GREEN）

| 测试 | 文件 | RED 证据 |
|---|---|---|
| `re-subscribes the active session after a reconnect` | `agentj-ui/apps/web/src/App.test.tsx` | 先打开会话（1 次 `session/subscribe`），再从「设置 · 连接」重连（需先填 `Host`=127.0.0.1 / `Port`=4096，否则 `ConnectionBar.validate()` 直接返回），断言出现**第 2 次** `session/subscribe` —— 无 effect 时该断言失败 |
| `RunPanel session busy hint`（2 条：命中提示 / 非命中透传原文） | `agentj-ui/apps/web/src/RunPanel.test.tsx` | 无 `sessionBusyMessage` 时友好文案断言失败 |

顺带修复（本阶段门禁发现，非本次缺陷）：`packages/sdk` 两条测试把工作区版本**硬编码为
`0.29.1`**（0.29.4 / 0.29.5 两次 bump 后已静默失败，CI 不跑 npm 测试故未暴露）——
`src/__tests__/config.test.ts` 改为从仓库根 `Cargo.toml` 读版本并断言相等（真正的不变量），
`src/__tests__/init-default.test.ts` 删除重复的版本断言。

## 验证结果（0.29.6，2026-10-16）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | **655** passed（53 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | **55** passed（6 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（仅既有 chunk > 500 kB 提示） |
| `bun --cwd agentj-ui/apps/tui test` | 177 passed（21 files，未受本阶段改动影响） |
| `cargo test -p agentj-protocol -- drift` | 2 passed（协议面无漂移） |
| `./scripts/check-version.sh` | **version-ok（0.29.6 / 0.18.0）** |

> 后端侧门禁（`cargo test --workspace` 865 passed / clippy 0 warning）见 `summary_and_next_se.md`。

## 已完成（0.29.0 – 0.29.5，详见 `docs/changelog/v0.29.md`）

- ✅ **0.29.0 功能（前端）**：Web/TUI 链接 opencode-go API Key——`packages/protocol` 78 schemas
  类型重生成、`packages/sdk` `METHODS` 28 + 版本常量；Web `SubscriptionPanel`「链接方式」区块
  （遮蔽输入、三态回显、env 优先提示、「只保存在所连接内核主机」说明、解除链接回退目录）、
  账号目录合并与自动选中（T17–T19）、desktop 三清单版本同步；TUI `SubscriptionOverlay`
  （`i`/`Enter`/`c`，逐字符 `•` 遮罩）。Phase-5 评审修复 M1（解除链接立即丢弃目录）/ L3
  （改用 `@agentj/protocol` 生成类型）。
- ✅ **0.29.1**：验收 BUG-001（后端 `save_link` 在 env 遮蔽时用错凭据）修复，前端面零变更。
- ✅ **0.29.2 – 0.29.5**：纯后端修复阶段 + 纯版本号同步（工作区 0.29.2 → 0.29.5：
  `@agentj/sdk` `VERSION`、desktop 三清单与 lockfile、`check-version.sh`），**无前端功能变更**。

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
  没有自动重连/退避；本阶段只保证「重连后重新订阅」。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- v0.29.6 前端代码 + 测试已落地；全门禁绿。剩余：提交 → 推送**两个远端**
  （本机 `github` = GitHub 权威、`origin` = GitCode 镜像；注意 AGENTS.md 的远端命名与本机相反）
  → 在 GitHub 开 PR（用户负责合并）。
- 前端候选：③ Web 接入服务端搜索；或订阅状态常驻指示（需先定义新鲜度语义）；
  或清理 `TopbarMenu` / `SettingsDialog` 测试里的重复 stub helper。
- 后端候选见 `summary_and_next_se.md`。
