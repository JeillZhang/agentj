# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **B9 批次（前端侧）**——后端同批次见 `summary_and_next_se.md`。
`VERSION` 仍 **0.1.0**；`PROTOCOL_VERSION` 随本批 T6c 升到 **0.21.0**（`packages/sdk` 的
`config.ts` 与 `progress.ts` 同步，`packages/protocol` 与 `methods.ts` 已重新生成）。

> 当前功能：**B9 可观测性 · 协议截断上报 · 用量面板（前端侧）** —— 分支
> `feature/b9-observability-config-hardening`，前端侧为 T6c / T7 / T9 / T9a / T10 / T11。

## B9 批次（前端侧） ✅

### 交付（每项一提交）

| 计划项 | 提交 | 内容 |
|--------|------|------|
| T6c | `6c2f387` | `@agentj/sdk`：`progress.ts` 解析 `truncated_chars`（`value.truncated`），`PROTOCOL_VERSION` 0.21.0；伪造值被忽略且不丢结果 |
| T7 | `6c2f387` | `@agentj/web` `RunPanel`：截断的工具结果渲染「结果已截断 50k 字符」（`role="status"`），未截断不带该计数 |
| T11 | `9af7c8b` | `UsageSparkline` + 新模块 `sparkBuckets.ts`：样本间距 < 8px 时按桶聚合，每桶保留 min/max 顶点（200 样本 / 120px → ≤30 段），tooltip 与键盘报「样本 N–M」区间 |
| T10 | `8ba2aa6` | `App.tsx` 增 shell 作用域 `u` / `Shift+U`（`useShortcutScope`），`SubscriptionStatus` 面板标出 `u 用量设置` |
| T9（替代形态） | `ee80d57` | `SettingsDialog` 分类由 5 个变 6 个（新增「用量」）；`SubscriptionPanel` 增 `section="usage" \| "subscription" \| "all"`，设置两个 tab 各渲染一半、共用同一份数据与轮询契约；凭据缺失时「订阅」tab 带装饰红点（`aria-hidden`）+ `title="订阅 · 待配置凭据"`（可访问名仍为「订阅」） |
| T9a（部分） | `a6353a2` | `deviceStateLabel` 移入 `subscriptionModel.ts`（`pending` / `slow_down` → `null`，未知状态回落「设备码登录未完成，请重试」） |

**与计划的偏差**：T9 未新建 `SubscriptionCredentialCard.tsx` / `SubscriptionHistory.tsx`——
改为同组件按 `section` 分段（理由与 AC 覆盖见 `_se.md` 同节）。T9a 的 `historyEmptyText` 未实现：
当前没有「暂无用量记录」空态（趋势图 <2 点整体不渲染），加了是死代码。

### 门禁状态（本批收尾实测）

- `npm --prefix agentj-ui test --workspace=@agentj/web`：**772 passed**（0 failed）。
- `npm --prefix agentj-ui run build --workspace=@agentj/web`：通过。
- `npm --prefix agentj-ui run lint --workspace=@agentj/web`：**0 error**（warning 仅打印）。
- `npm --prefix agentj-ui test --workspace=@agentj/sdk`：**91 passed**；
  `bun --cwd agentj-ui/apps/tui test`：**204 passed / 0 fail**。

### 本批新增测试要点

- `UsageSparkline.test.tsx`：密集序列 ≤30 段且峰值仍是顶点（按 `path` 的 `d` 断言坐标）、
  桶 tooltip 报样本区间、`width=0` 渲染空。
- `settingsDialog.test.tsx`：六个分类、每个分类解析到**唯一一个非空 tabpanel**（`aria-labelledby`
  与 tab id 对齐）、用量 tab 只有用量区（无 API Key 输入）、订阅 tab 只有凭据区（无「刷新用量」）、
  凭据缺失时红点 + title、`state: "ok"` 时无红点。
- `App.test.tsx`：`u` → 用量 tab、`U` → 订阅 tab（`it.each`），输入框内按键不触发。

## 遗留问题 / 已知限制（前端）

- **`SubscriptionPanel.tsx` 仍是单文件 300+ 行**：本批只做了「按 `section` 分段」，没有拆成
  `SubscriptionCredentialCard` / `SubscriptionHistory` 两个文件；数据获取（auth/status、
  provider/usage、usageHistory、设备码轮询）仍全部在这一个组件里。
- **`u` / `Shift+U` 只在 shell 作用域内生效**：焦点在 `body`（没有任何元素获得焦点）时
  `useShortcutScope` 的 `contains(activeElement)` 判定失败，快捷键不触发；需要先 Tab 一次或点一下界面。
- **趋势密度降级是「固定 8px + min/max」**：桶内只保留最小与最大两个顶点，介于两者之间的波动被
  抹平；极窄宽度（< 40px）仍会压成极少的桶。
- **`packages/sdk` 版本断言的守护范围**：`config.test.ts` 断言「`VERSION` == 仓库根 `Cargo.toml`
  版本」，desktop 三清单 / `agentj-ui/package-lock.json` 根条目仍靠 `scripts/check-version.sh`。
- **`sessionBusyMessage` 用文本匹配**：SDK 抛错只保留 `message` 文本（未附 `code`），故用
  `includes("-32805")`；若给 SDK `Error` 加 `code` 字段，应改为结构化判断。
- **无 `session/unsubscribe`**：会话切换后旧会话订阅仍在（服务端按连接归属，重连即释放）。
- **Web 连接为手动重连**：`Connect` 才重建 transport/client（`connectSeq`），无自动重连 / 退避。
- **标签回退仍是「我们请求的」模型**：仅当内核省略 `model` 时才回退选择器取值。
- **工具摘要只覆盖已知工具**：`toolSummary` 对未识别 `name` 返回空串，MCP / 插件工具可能无摘要。
- **`cmd` / `command` 字段名不一致（既有）**：`toolSummary` 以 `command` 取值，而
  `crates/agentj-protocol/src/permission.rs` / `schemas/PermissionRequestParams.json` 文档与
  Web `ApprovalDialog.tsx:22` 标签仍写 `cmd`；属独立遗留项。
- **TUI 仍会为视觉空白增量留一个空行（BUG-017 同源盲区）**：`agentj-ui/apps/tui/src/runview.tsx:126`
  的 `case "text"` 无条件 push，零宽帧多出一个空白文本行；收敛方案见上一批记录（上提 `isBlankText`
  到 SDK，或内核广播侧丢弃）。
- **窗口标签只在有 `context_window` 时出现**：目录快照里仍有 7 个模型无窗口（不猜），不显示容量标签。
- **趋势依赖服务端取样密度**：`history_enabled = false` 或保留期不一致时趋势变短 / 为空。

## 待实施（跨阶段保留：未实施不得删除）

> 本批把「设置分类拆分 / 用量面板键盘可达 / 趋势图密度降级 / 截断上报显示」实施完毕（本表无对应
> 条目）；当前表中暂无前端条目——下一特性由 `jskill-feature-next` 合并 `_se.md` 待实施表选取。

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本批次（B9 前端侧 T6c / T7 / T9 / T9a / T10 / T11）已收尾并过门禁；分支推 `origin` 与
  `gitcode`，PR 开在 `origin`（用户负责合并）。
- 表外可选改进：`SubscriptionPanel.tsx` 拆成凭据卡 / 历史两个文件（上一批与本批都提到，仍未做）；
  趋势桶策略改为「保留 min/max + 中位」或按可用宽度自适应阈值；shell 作用域快捷键在
  `document.activeElement === body` 时也生效（需给 shell 一个可聚焦容器）。
- 后端候选见 `summary_and_next_se.md`（⑰⁺ 真 BPE 词表、㉟⁺ 产物签名与公证）。

> 远端：`origin`（GitHub `JeillZhang/agentj`）是唯一权威远端，PR 只在此创建与合并；
> `gitcode` 是只读镜像（快进推送同步，不再开 MR）。feature 分支两远端同推同一 SHA。
