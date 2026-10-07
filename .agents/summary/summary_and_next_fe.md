# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.27.0 opencode-go 订阅二期（前端部分）**
> ——来源：设计 `docs/superpowers/specs/2026-10-07-agentj-opencode-go-phase2-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-07-agentj-opencode-go-phase2-plan.md`（T4/T5/T22–T27）。
> 前端范围：`packages/protocol`（类型重生成）+ `packages/sdk`（METHODS 26 + VERSION）+
> `apps/web`（订阅面板 / 设置 tab / 客户端贯通 / 配额提示）+ `apps/tui`（订阅 overlay / palette）
> + `apps/desktop`（版本清单）；工作区 0.26.0 → **0.27.0**。
> 后端部分见 `summary_and_next_se.md`。

## v0.27.0（opencode-go 订阅二期，前端部分）✅（本次完成）

- ✅ **`packages/protocol`**：`schemas/*.json`（72）重生成 `types.d.ts`，新增
  `ModelListResult` / `ModelEntry` / `ProviderUsageResult` / `AuthDevicePollResult` /
  `CredentialInfo` 等类型断言（`tsc --noEmit` 覆盖）。
- ✅ **`packages/sdk`**：`METHODS` 21 → **26**（`modelList` / `authStatus` / `providerUsage` /
  `authDeviceStart` / `authDevicePoll`）；`VERSION` 0.26.0 → 0.27.0；
  `PROTOCOL_VERSION` 0.16.0 → 0.17.0。
- ✅ **Web（`apps/web`）**：
  - 新增 `SubscriptionPanel`：三窗口行（5 小时 / 本周 / 本月，`used` 百分比）、
    `rate-limited` / 超阈值 `aj-window--warn` 标记、凭据行（api_key 显示 `key_hint`、
    oauth 显示 org/expiry）、`stale` 缓存提示、按 `error.kind` 的分句文案（**不编造百分比**）、
    设备码登录（显示 `user_code` + 验证 URI，按上游 `interval` 轮询，`slow_down` 用回包间隔，
    `authorized` 后刷新 auth/status + model/list(refresh) + provider/usage 并停止；卸载清定时器）。
  - `SettingsDialog` 新增「订阅」tab（`连接/显示/项目/演示数据/订阅`，键盘可达），
    无 client 时显示「先连接内核」提示而非崩溃；`TopbarMenu` 透传可选 `client`；
    `App` 把 App 级 `RpcClient` 贯通到设置弹窗；`App.test.tsx` 的 METHODS mock 补齐 5 个键。
  - `RunPanel` 一次性配额提示：仅匹配上游 `GoUsageLimitError` 文案（不看 429 状态码），
    每会话最多显示一次。
- ✅ **TUI（`apps/tui`）**：新增 `SubscriptionOverlay`（文本版三窗口行 + `已限流` 标记 +
  同一套 `ERROR_TEXT` 文案 + 可注入 `schedule` 的设备码轮询状态机 + 卸载 `cancelled`/清定时器），
  palette 新增 `subscription` 命令，`app.tsx` 挂载 overlay 并在其打开时早退键盘处理（Esc 关闭）。
- ✅ **版本同步**：`@agentj/sdk` `VERSION` → 0.27.0；desktop 三清单
  （`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml`）与
  `scripts/check-version.sh` 同步 0.27.0（`PROTOCOL_VERSION` **0.17.0**）；
  `agentj-ui/package-lock.json`、`bun.lock`、desktop `Cargo.lock` 版本元数据刷新。
- ✅ **文档（前端相关）**：`docs/specs/ipc-protocol.md`（§8.23–8.27 等）、
  `docs/changelog/v0.27.md` + 索引、`docs/guide/subscription.md` 重写
  （设备码登录 / 三窗口用量 / 三层目录 / `console_base_url` / 排障文案）。

## 验证结果（全量门禁清扫，2026-10-07）

> 本表由编排器在本阶段最终树上执行；前端门禁如下，后端门禁见 `summary_and_next_se.md`。

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 637 passed（53 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 54 passed（6 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean |
| `bun --cwd agentj-ui/apps/tui test` | 169 passed（21 files） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |
| `./scripts/check-version.sh` | version-ok（**0.27.0** / `PROTOCOL_VERSION` **0.17.0**） |

## 代码评审（Phase 5，`ajteam-task-reviewer` 分组只读审查）

- **前端（T22–T27）：首次 VERDICT FAIL → 已修复**（commit `99385c6`，同一阶段内闭合）。
  - 偏差：TUI `subscription.tsx` 的 `windowLine` 把缺失的 `UsageWindow.percent`
    （wire 合法值：上游省略或非整数）默认成 `0%`，等于编造读数；Web 面板已正确守卫。
  - 修复：缺失百分比改渲染显式未知标记 `—`（并补一条 TUI 用例断言不出现 `0%`），
    同时在卸载时清除挂起的轮询定时器，而非仅依赖 `cancelled` 标志。
  - 复审结论：原偏差已消除，Web/TUI 口径一致。

## 遗留问题 / 已知限制（前端）

- **预存在的 TUI 工具链问题（非本阶段回归）**：`bunx tsc --noEmit` 在
  `src/e2e/mock-frames.test.ts`、`src/e2e/mock-kernel.ts`（缺 `ws` 类型声明）、
  `src/permission.test.tsx`、`src/runview.tsx`（`ScrollBoxRenderable.scrollY`）上报错；
  TUI 门禁以 `bun test` 为准（本阶段 169 passed / 0 fail）。
- **订阅面板为按需拉取（设计 D6/D10）**：无常驻轮询、无徽标；打开面板才取数，
  usage 服务端缓存 300s、model 目录 60s，`refresh:true` 可绕过。
- **Web `AgentJ` 尚无「订阅」入口的热键/常驻指示**：仅设置弹窗内的 tab 与 429 后的一次性提示。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- v0.27.0 opencode-go 二期（前端部分）完成：Web 637 / SDK 54 / TUI 169 全绿，
  Web + protocol 构建 clean，check-version / desktop-ui 通过；待 PR 由用户合并 main。
- 前端候选：③ Web 接入服务端搜索；或在弹层重新打开时从 `agentj.composer.model` 回灌选择；
  或订阅状态常驻指示（需先定义新鲜度语义，见设计 D10 的非目标）。
- 后端候选见 `summary_and_next_se.md`。
