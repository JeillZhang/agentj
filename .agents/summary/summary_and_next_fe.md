# Summary & Next — Frontend (FE / 前端)

> 本文件仅保留**当前正在开发**的前端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 前端范围：`agentj-ui/*`（`apps/web` / `apps/tui` / `apps/desktop` + `packages/sdk` /
> `packages/protocol` / `packages/tokens`）。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 后端总结独立存放于 `summary_and_next_se.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.0 Web/TUI 链接 opencode-go API Key（前端部分）**
> ——来源：设计 `docs/superpowers/specs/2026-10-08-agentj-ui-opencode-go-api-key-link-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-08-agentj-ui-opencode-go-api-key-link-plan.md`
> （T13–T22 / T24–T25）。
> 前端范围：`packages/protocol`（78 schemas 类型重生成）+ `packages/sdk`（METHODS 28 + 版本常量）
> + `apps/web`（订阅面板「链接方式」区块）+ `apps/tui`（订阅 overlay 链接模式）
> + `apps/desktop`（版本清单）。
> 工作区 0.28.0 → **0.29.0**；`PROTOCOL_VERSION` 0.17.0 → **0.18.0**。后端部分见
> `summary_and_next_se.md`。

## v0.29.0（Web/TUI 链接 opencode-go API Key，前端部分）🟡（T13–T16 / T20 完成）

- ✅ **`packages/protocol`**：`schemas/*.json` 72 → **78**（`AuthSetApiKeyParams` /
  `AuthSetApiKeyResult` / `ApiKeyOutcome` / `ApiKeyLinkError` / `AuthClearCredentialParams` /
  `AuthClearCredentialResult`）重生成 `types.d.ts`，补类型断言并 `tsc --noEmit` 通过。
- ✅ **`packages/sdk`**：`METHODS` 26 → **28**（`authSetApiKey` / `authClearCredential`）；
  `VERSION` 0.28.0 → **0.29.0**、`PROTOCOL_VERSION` 0.17.0 → **0.18.0**（`config.test.ts`
  计数与常量断言）。
- ✅ **Web · `SubscriptionPanel`**：「链接方式」区块 = `type="password"` + `autoComplete="off"`
  遮蔽输入、空 key 时「链接」禁用、链接 / 解除链接按钮、三态回显（verified / unverified 文案 +
  rejected 复用共享 `ERROR_TEXT` 的「未保存：…」）、已有 OAuth 凭据时「将替换现有 OAuth 凭据」
  提示、`effective_source=env` 时「环境变量当前优先，链接的密钥暂不生效」提示、
  「API Key 只保存在所连接的内核主机（auth.json），不会写入浏览器存储」；提交后清空输入框、
  DOM 中不出现完整 key；`subscriptionPanel.test.tsx` 覆盖上述断言。
- ✅ **TUI · `SubscriptionOverlay`**：`i` 进入输入（逐字符只渲染 `•` 遮罩，帧内无明文）、
  `Enter` 提交、`Esc` 取消（链接模式下 Esc 取消输入而非关闭 overlay）、`c` 解除链接；
  三态文案与 env 优先提示、「只保存在所连接内核主机」说明；
  overlay 打开时全局键盘早退与 Esc 关闭不回归；`subscription.test.tsx` 覆盖。
- ⏳ **剩余步骤（同阶段内闭合，Phase 8 收尾前必须落地）**：
  - **T17 / T18 / T19**：`App` 合并 `auth/setApiKey` 返回的账号目录并自动选中首个 opencode-go
    条目、`SettingsDialog` 透传 `onLinked` 回调、`TopbarMenu` 的 RPC mock 补齐。当前
    `SubscriptionPanel` 的 `onLinked` **尚无人消费**——目录覆盖与自动选中未接通，
    `docs/guide/subscription.md` 已按设计 D10 的目标行为描述。
  - **T22**：`apps/desktop` 三清单（`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml`）
    与 lockfile 版本元数据 → 0.29.0（`check-version.sh` 的 desktop 段仍停在 0.28.0）。
  - **T25**：`AGENTS.md` / `README.md` 基线（0.29.0 / 0.18.0 / 28 方法 / 78 schemas）。

## 验证结果（当前树实测，2026-10-08）

| 门禁 | 结果 |
|---|---|
| `npm --prefix agentj-ui run build --workspace=@agentj/protocol` | clean（`tsc --noEmit`） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 55 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 644 passed（53 files） |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | clean（仅 chunk > 500 kB 提示） |
| `bun --cwd agentj-ui/apps/tui test` | 177 passed（21 files） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |
| `./scripts/check-version.sh` | ⏳ 未通过：0.29.0 / 0.18.0 已就位，desktop 三清单仍 0.28.0（待 T22） |

> 本表在 Phase 8 收尾（`docs: auto-sync ...`）于最终树重跑；T17–T19 / T22 / T25 落地后数字
> 会再变，以收尾轮的实测为准。

## 人工验收（真实 Go 订阅 Key；**非 CI 跟踪项**）

用真实 Go 订阅 key 走一遍端到端（设计「测试策略 · 端到端（人工，记入验收清单，不进 CI）」）：

1. `agentj serve`（远程场景加 `--host` + `--token`）→ 打开 Web → 设置 · 订阅。
2. 粘贴真实 key → 「链接」→ 期望 `verified`（「已链接：密钥已保存并验证通过」）+ 账号目录，
   `auth.json` 权限 `0600`、`auth/status.credential.key_hint` 为 `…XXXX`。
3. 选中模型（opencode-go/&lt;id&gt;）→ 跑一轮含工具调用的对话；抓包 / 日志确认请求带
   `User-Agent: agentj/<版本>` 与 `x-opencode-session`。
4. 「解除链接」→ 目录回退 live / overlay；`auth/status.authenticated=false`（env 无 key 时）。
5. 断网重新链接 → 期望 `unverified`（「密钥已保存，但未校验」），保存后仍在；切回网络即恢复。
6. TUI 同一流程（palette `subscription` → `i` / `Enter` / `c`）复核遮罩与三态文案。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ③ | Web 接入服务端搜索 | Web Files 搜索态打通（消费服务端 `query`/`total`；提示行搜索态可达） | 无（基于 v0.22.0 服务端搜索） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_se.md` 待实施表。

## 下一步

- 收尾 v0.29.0 前端：T17–T19（目录覆盖 / 自动选中 / 回调透传 / mock 补齐）→ T22（desktop
  三清单 + lockfile）→ T25（AGENTS.md / README.md 基线），随后重跑本文件「验证结果」表。
- 前端候选：③ Web 接入服务端搜索；或订阅状态常驻指示（需先定义新鲜度语义，见设计 D10 的非目标）。
- 后端候选见 `summary_and_next_se.md`。
