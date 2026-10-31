# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **订阅用量错误归类修复（缺陷修复，前端侧）**——后端同批见
`summary_and_next_se.md`。`VERSION` 仍 **0.1.0**，`PROTOCOL_VERSION` 仍 **0.21.0**
（纯前端展示层，不改 wire）。

> 当前功能：**「用量暂时无法获取（上游响应无法识别）」文案误导修复** —— 分支
> `fix/subscription-usage-default-provider`。

## 问题与根因 ✅

- 用户看到的两条提示同源（后端 `provider: null` → `-32602`，见 `_se.md`），但**文案把原因说错了**：
  `useSubscriptionStatus` 的探针（`auth/status`）失败时套用 `errorKind: "unavailable"`，而
  `unavailable` 在内核侧是 `UsageError::Unavailable`（上游响应体不可解析）的专有判词，于是
  连接 / 参数类失败被读作「上游响应无法识别」；左上角则直接渲染裸 RPC 文本 `-32602: InvalidParams`。

## 修复 ✅（`agentj-ui/apps/web`）

| 文件 | 改动 |
|------|------|
| `src/hooks/useSubscriptionStatus.ts` | 探针失败改用新 kind `probe_failed`（仍归 `error`，不进 `invalid`；不顺延成 `unknown`，否则会谎称「未链接凭据」并弹出链接 CTA） |
| `src/components/subscriptionModel.ts` | `ERROR_TEXT` 增 `probe_failed: "无法从内核读取订阅状态：请检查连接后重试"`；`unavailable` 保持「上游响应无法识别」不动 |
| `src/components/useSubscriptionData.ts` | `usageFromStatus` 的兜底 kind `unavailable` → `probe_failed`（防御分支，语义一致） |
| `src/components/SubscriptionStatus.tsx` | 悬浮面板先显示映射文案（`ERROR_TEXT[errorKind]`），再以弱化样式附原始 RPC 文本，便于定位 |
| `src/components/subscriptionPanel.css` | 新增 `.aj-sub-status__error--raw` 与 `.aj-sub__error--raw`（`--aj-textCaption` + 11px，本文件既有小字惯例） |
| `src/components/subscriptionModel.ts` | 新增 `errorText(kind)`：`Object.hasOwn` 守卫的查表（线上 kind 可能叫 `"toString"`，裸查会拿到 `Object.prototype` 成员并把函数交给 React 渲染）；四处查表统一改走它 |
| `src/hooks/useSubscriptionStatus.ts` | `provider/usage` 的 catch 也从 `errorKind: "network"` 改为 `probe_failed`：被拒的请求可能已被内核作答（如 `-32602`），`network` 只留给内核自己上报的 `usage.error.kind` |
| `src/components/SubscriptionUsage.tsx` | 设置面板的用量错误在映射文案下补一行 `.aj-sub__error--raw` 原始 RPC 文本（与顶栏面板对齐） |

测试：`useSubscriptionStatus.test.ts` 的探针失败/重取失败用例均断言 `errorKind === "probe_failed"`；
`SubscriptionStatus.test.tsx` 新增「映射文案 + 原始 RPC 文本同时出现」用例；新增
`SubscriptionUsage.test.tsx`（映射文案、原始文本行、未知 kind 只显原始文本、`"toString"` kind 不渲染函数）；
`useSubscriptionData.test.tsx` 新增「status 带 error 但 errorKind 为 null → `probe_failed`」用例。

## 实测门禁（本批，2026-10-31）

| 门禁 | 结果 |
|------|------|
| `npm test --workspace=@agentj/web` | 66 文件 / **791 passed** |
| `npm run lint --workspace=@agentj/web` | 0 error（仅既有 warning） |
| `npm run build --workspace=@agentj/web` | 通过 |
| `bun --cwd agentj-ui/apps/tui test` | 205 passed（未改 TUI，回归确认） |

## 遗留问题 / 已知限制（前端）

- **`probe_failed` 是客户端专有 kind**：内核的 `ProviderUsageError.kind` 枚举
  （`docs/specs/ipc-protocol.md`）不会出现该值，只有 `useSubscriptionStatus` 会产出它；
  该表是 `Record<string, string>`，未知 kind 一律返回 `undefined`（面板只渲染文案行，不崩）。
- **TUI 未跟随**：`agentj-ui/apps/tui/src/subscription.tsx` 的 `refreshAll` 对失败一律
  `catch {}`，既不显示错误也不误报文案；如需对齐，得先给 TUI 一个错误状态位（独立小批）。
- **设置面板内 `subscriptionPanel` 的 `loadUsage` 仍吞掉失败**：失败时保留旧快照（首次为 `null`），
  面板显示空状态而非错误句；真正的错误句来自壳层快照经 `usageFromStatus` 注入的那条路径。
  两条路径现在都会带上原始 RPC 文本（`SubscriptionUsage` 新增的弱化行）。
- **`SubscriptionPanel` 仍自己发起四条 RPC**：无连接级共享缓存，挂载即拉
  `provider/usage` + `provider/usageHistory`（注入 `status` 时省掉 usage）。
- **`sessionBusyMessage` 用文本匹配**：SDK 抛错只保留 `message`（未附 `code`）。
- **Web 连接为手动重连**：无自动重连 / 退避。

## 待实施（跨阶段保留：未实施不得删除）

> 当前表中暂无前端条目——下一特性由 `jskill-feature-next` 合并 `_se.md` 待实施表选取。

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本修复已收尾并过门禁；分支推 `origin` 与 `gitcode`，PR 开在 `origin`（用户负责合并）。
- 表外可选改进：给 TUI 订阅面板加错误状态位（与 Web 的 `probe_failed` 对齐）；
  `SubscriptionPanel` 的四条 RPC 改为连接级共享缓存。
- 后端候选见 `summary_and_next_se.md`（⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证）。

> 远端：`origin`（GitHub `JeillZhang/agentj`）是唯一权威远端，PR 只在此创建与合并；
> `gitcode` 是只读镜像（快进推送同步，不再开 MR）。feature 分支两远端同推同一 SHA。
