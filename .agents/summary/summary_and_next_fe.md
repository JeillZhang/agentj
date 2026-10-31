# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **B10 收尾债批次（前端侧）**——后端同批次见 `summary_and_next_se.md`。
`VERSION` 仍 **0.1.0**，`PROTOCOL_VERSION` 仍 **0.21.0**（本批不改 wire 结构，仅把协议文档与
schema 描述里的 `cmd` 更正为真实的 `command`）。

> 当前功能：**B10 收尾债批次（前端侧四项）** —— 分支
> `feature/b10-debt-observability-batch`，前端侧为 T7 / T8 / T9 / T10 / T12 / T13 / T14 / T15 / T16。

## B10 批次（前端侧） ✅

### 交付（每项一提交）

| 计划项 | 提交 | 内容 |
|--------|------|------|
| T7 | `85ef4ce` | `useShortcutScope` 新增 `allowBodyFocus`（默认关），壳层开启：`document.activeElement === body` 也算作用域内 |
| T8 | `e922cae` | 趋势桶加**中位顶点**（`SparkBucket.vertices` 升序）+ `MAX_BUCKETS = 24` 硬上限；`percent` 仍是桶内最大样本 |
| T9 | `6a8b8b8` | `ApprovalDialog` 的 bash 字段表 `cmd` → `command`（命令）/ `timeout_ms`（超时） |
| T10 | `8524e71` | `agentj-protocol` 文档与 `schemas/PermissionRequestParams.json` 的 `cmd` → `command` + 两条防回退断言 |
| T12 | `26c7b53` | TUI 六处 bash 权限夹具 `cmd` → `command` |
| T13 | `ebdf516` | TUI `RunView`（仅测试通道）两处 `map` 都过滤视觉空白帧 |
| T14 | `9c0d3a2` | 新增 `useSubscriptionData`（数据层整体搬出），`SubscriptionPanel` 127 行 |
| T15 | `6f1c1b7` | 新增 `SubscriptionHistory`（图表 + 刷新），面板 123 行 |
| T16 | `04efc34` | 新增 `SubscriptionCredentialCard`（链接表单 + 设备码），面板 **80 行** |

### 实测门禁（本批收尾，2026-10-30）

| 门禁 | 结果 |
|------|------|
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 66 文件 / **784 passed** |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | `tsc -b && vite build` 通过 |
| `npm --prefix agentj-ui run lint --workspace=@agentj/web` | 0 error（warning 未新增） |
| `bun --cwd agentj-ui/apps/tui test` | 23 文件 / **205 passed** |
| `@agentj/sdk` / `@agentj/protocol` / `@agentj/tokens` | 92 / 4 / 11 passed |
| protocol / sdk 生成 + 漂移 | `generate` 无 diff，`methods.ts` 仍 29 方法 |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | `desktop-ui-ok` |
| `./scripts/check-version.sh` | `version-ok` |

### 本批新增测试要点（防回退的判别力）

- `SparkBucket` 中位顶点的断言只能用 `,\d+\.\d+` 抓 y：旧写法 `-?\d+\.\d+` 连 x 一起抓，
  会得到 29 个不同值而假通过；去掉中位顶点后该用例必须失败（`expected 2 to be 3`）。
- `UsageSparkline` 的降采样上界从 `toBeLessThanOrEqual(30)` 放宽到 `72`：120px 宽、200 样本时
  密度下限给出 span 14 > 桶上限 span 9，真正变化的是**每桶顶点数 2 → 3**。
- TUI `RunView` 的空白帧用例必须用**交错**空白（`["hello","\u200b","world","  ","","\ufeff"]`）：
  连续尾部空白帧渲染成行尾空行，会被 `trimEnd`/裁剪吞掉，不修也能过（首版探针即假通过）。
  `testRender` 返回 Promise，取帧用 `setup.captureCharFrame()`（没有 `lastFrame()`）。
- `useSubscriptionData` 的注入式模式断言「不调 `providerUsage`、仍调 `providerUsageHistory`」；
  轮询停止用 `startDevice` + fake timers 驱动真实 `authDevicePoll`，而不是空跑一次 advance。
- `ApprovalDialog` 的 bash 夹具必须是 `{ command, timeout_ms }`：字段表键名与真实参数名不一致时，
  面板会把整个 `args` 当原文倾倒，只有断言「命令 / 超时」两个中文标签才抓得到。
- 组件拆分零 DOM 变更由既有 `subscriptionPanel.test.tsx`（17 处渲染）与
  `settingsDialog.test.tsx` 零改动通过来保证。

## 遗留问题 / 已知限制（前端）

- **`SubscriptionPanel` 仍自己发起四条 RPC**：拆出 hook 只是把代码搬走，没有做「一页面一次
  连接级缓存」；`history` 与 `provider/usage` 仍是每次挂载各拉一次（注入 `status` 时省掉 usage）。
- **`packages/sdk` 版本断言的守护范围**：`config.test.ts` 断言「`VERSION` == 仓库根 `Cargo.toml`
  版本」，desktop 三清单 / `agentj-ui/package-lock.json` 根条目仍靠 `scripts/check-version.sh`。
- **`sessionBusyMessage` 用文本匹配**：SDK 抛错只保留 `message` 文本（未附 `code`），故用
  `includes("-32805")`；若给 SDK `Error` 加 `code` 字段，应改为结构化判断。
- **无 `session/unsubscribe`**：会话切换后旧会话订阅仍在（服务端按连接归属，重连即释放）。
- **Web 连接为手动重连**：`Connect` 才重建 transport/client（`connectSeq`），无自动重连 / 退避。
- **标签回退仍是「我们请求的」模型**：仅当内核省略 `model` 时才回退选择器取值。
- **工具摘要只覆盖已知工具**：`toolSummary` 对未识别 `name` 返回空串，MCP / 插件工具可能无摘要。
- **窗口标签只在有 `context_window` 时出现**：目录快照里仍有 7 个模型无窗口（不猜），不显示容量标签。
- **趋势依赖服务端取样密度**：`history_enabled = false` 或保留期不一致时趋势变短 / 为空。
- **`MAX_BUCKETS = 24` 是定值**：极宽窗口（> ~1500px）仍会因上限而丢分辨率，改自适应需先有
  可测的宽度来源。

## 待实施（跨阶段保留：未实施不得删除）

> B9 与 B10 两批的前端项均已实施完毕（本表无对应条目）；当前表中暂无前端条目——
> 下一特性由 `jskill-feature-next` 合并 `_se.md` 待实施表选取。

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 本批次（B10 前端侧 T7 / T8 / T9 / T10 / T12 / T13 / T14 / T15 / T16）已收尾并过门禁；
  分支推 `origin` 与 `gitcode`，PR 开在 `origin`（用户负责合并）。
- 表外可选改进：`SubscriptionPanel` 的四条 RPC 改为连接级共享缓存；趋势桶上限随宽度自适应；
  `toolSummary` 覆盖 MCP / 插件工具的参数摘要。
- 后端候选见 `summary_and_next_se.md`（⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证）。

> 远端：`origin`（GitHub `JeillZhang/agentj`）是唯一权威远端，PR 只在此创建与合并；
> `gitcode` 是只读镜像（快进推送同步，不再开 MR）。feature 分支两远端同推同一 SHA。
