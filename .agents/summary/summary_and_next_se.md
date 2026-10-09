# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

> 当前功能：**v0.29.2 opencode-go 路由改为 API 派生（后端部分）**
> ——来源：用户提出「`models/` 目录是否可删 / 端点配置能否不写死」，
> 以官方[端点表](https://opencode.ai/docs/go/#api-endpoints) + models.dev 实测核对后实施。
> 后端范围：`agentj-model`（`catalog` / `catalog_gen` / `bin/gen_opencode_catalog` /
> `data/opencode-go.json`）、`agentj-cli`（base_url 单一真源）、
> `docs/guide/subscription.md`、`docs/changelog/v0.29.md`、`AGENTS.md` / `README.md` 基线、版本元数据。
> 协议 wire 面**不变**（`PROTOCOL_VERSION` 仍 **0.18.0** / 28 方法 / 78 schemas）；
> 工作区 0.29.1 → **0.29.2**。

## v0.29.2（opencode-go 路由 API 化）✅

### 核对结论（2026-10-09 实测，全部可复现）

| 来源 | 内容 | 是否含端点元数据 |
|---|---|---|
| `GET /zen/go/v1/models` | 45 条，字段仅 `id` / `object` / `created` / `owned_by` | ❌ 无 |
| `GET /zen/v1/models` | 87 条，同上四字段 | ❌ 无 |
| `GET {console}/api/v2/config` | provider → 模型 **id 键** | ❌ 无 |
| `GET /zen/go/v1/models/{id}`、`/zen/go/models`、`/api/models` | 404 | — |
| `GET https://models.dev/api.json` | `opencode-go.models.<id>.provider.npm`（模型级覆盖）+ provider 级 `npm` 默认 | ✅ 有 |

- models.dev 解析结果与官方端点表「AI SDK 包」列 **12/12 非 chat 模型零冲突**，20 条 chat 模型
  经 provider 级 `@ai-sdk/openai-compatible` **继承**后亦逐条吻合 ⇒ 端点路由**完全可由 API 派生**。
- `npm` 到 endpoint 的映射（协议事实，非逐模型配置）：`@ai-sdk/anthropic` → `/v1/messages`、
  `@ai-sdk/openai` → `/v1/responses`、`@ai-sdk/openai-compatible` → `/v1/chat/completions`。

### 已完成

- ✅ **修 `catalog_gen::dev_format` 路径（原为静默失效）**：原读 `models.<id>.npm`，真实形状是
  `models.<id>.provider.npm`；旧单测 fixture 用同形状假数据掩盖 → 该分支恒空，路由实际全靠手写种子。
  现按「模型级覆盖 → provider 默认」解析；未收录 id 一律 `openai-compat` + `verified:false`（不猜）。
- ✅ **删除手写路由表**：`bin/gen_opencode_catalog.rs` 的 `overrides()` 20 条 id→format 种子整体移除。
- ✅ **快照迁入 crate + 重新生成**：`models/opencode-go.json` → `crates/agentj-model/data/opencode-go.json`
  （45 条 / **35 verified** / 10 unverified；`include_str!("../data/...")` 去掉 `../../../` 反向引用；
  根目录 `models/` 删除）。生成门禁：`gen_opencode_catalog [--check]`。
- ✅ **`CatalogEntry.endpoint` 删除**：该字段从未被消费（各 transport 自持 `/v1/...` 路径），属重复端点配置；
  快照不再写 `endpoint`（`overlay_json` / `from_json` / 单测同步）。
- ✅ **base_url 单一真源**：新增 `agentj_model::catalog::OPENCODE_GO_BASE`；`agentj-cli` 的
  `OPENCODE_GO_BASE`（auth）/ `BUILTIN_OPENCODE_GO_BASE`（providers）改为引用它。
- ✅ **修复 7 条错误路由**（旧快照缺失 → 被合成为 `openai-compat`）：`claude-haiku-5-5`、
  `qwen3.8-flash`（→ `/v1/messages`）与 `grok-4.6`、`grok-4.7`、`gpt-6-luna`、
  `muse-spark-1.2/1.3-contributor`（→ `/v1/responses`）；由 `catalog_lookup_routes_formats` 锁定。
- ✅ **单测**：`catalog_gen`（覆盖继承 / 未收录不猜 / 未知 npm 降级 / 去重 / JSON 往返）、
  `catalog`（12 条官方路由 + 未解析降级为 unverified）；旧 `overrides` 相关测试随种子一并删除。
- ✅ **文档与基线**：`docs/guide/subscription.md`（新增「路由映射从哪来」小节 + 快照路径/生成命令）、
  `docs/changelog/v0.29.md`（0.29.2 小节）、`AGENTS.md`（当前版本 + `agentj-model` catalog 说明）、
  `README.md`（版本行 + 目录树去 `models/`）、`scripts/check-version.sh`。

### 已完成（0.29.0 / 0.29.1，详见 `docs/changelog/v0.29.md`）

- ✅ v0.29.0：Web/TUI 链接 opencode-go API Key（`auth/setApiKey` / `auth/clearCredential`，
  协议 0.18.0 / 28 方法 / 78 schemas）+ 运行期激活 + 内置 provider 条目；评审 1H/1M/4L 全闭合。
- ✅ v0.29.1：验收 BUG-001（env 遮蔽时读侧/发送侧不一致）单提交修复，回归 verified。

## 验证结果（当前树实测，2026-10-09）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `cargo test --workspace` | **853** passed / 0 failed（48 suites，含 permission e2e） |
| `cargo test -p agentj-protocol -- drift` | 2 passed |
| `cargo run -p agentj-model --bin gen_opencode_catalog -- --check` | exit 0（快照未过期） |
| 路由冒烟（live ∩ 快照，真实网络） | 45 模型；官方端点表 12 条非 chat 路由 **0 不匹配**，其余 10 条 unverified |
| `schemas/*.json` / `protocol/methods.json` | 78 / 28 方法 · `protocol_version` 0.18.0 |
| `./scripts/check-version.sh` | **version-ok（0.29.2 / 0.18.0）** |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 遗留问题 / 已知限制（后端）

- **`/v1/messages` 鉴权头（新发现，未修，待裁决）**：实测 `POST /zen/go/v1/messages`
  **只认 `x-api-key`**（仅 `Authorization: Bearer` → 401 `Missing API key`；仅 `x-api-key` →
  401 `Invalid API key`），而 `/v1/responses` 与 `/v1/chat/completions` 认 Bearer。
  `crates/agentj-model/src/anthropic.rs` 目前发的是 `Authorization: Bearer`（单测用 wiremock
  匹配同形头部，无法暴露）⇒ opencode-go 的 anthropic 路由（`claude-haiku-5-5` / `minimax-m2.7` /
  `minimax-m3` / `qwen3.7-plus` / `qwen3.8-flash` / `qwen3.8-max`）在真实网关上预计 401。
  **本阶段未改动**（属鉴权语义，需用户决定：`x-api-key` 与 `Authorization` 双发，或仅 `x-api-key`）。
- **2 条 legacy 路由变更**：`qwen3.6-plus` / `qwen3.7-max` 现按 models.dev 判为
  `openai-compat`（旧手写表标 `anthropic`）；二者均不在官方在售列表内。如实测不符，
  重新加回一条种子即可（改动面 = `build_overlay` 之前的一次 map）。
- **时序（设计已记录）**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间
  **进行中的那一轮 run 可能仍使用旧凭据，新凭据从下一轮生效**。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve`
  即远端主机）；非回环 `serve --host` 必须配 `--token`（既有要求，指南重述）。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案；
  两个 auth 方法**不新增权限门**。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。
- **预存在的 e2e 挂起（非本阶段回归）**：`crates/agentj-cli/tests/e2e_permission.rs` 的
  `permission_request_notified_and_timeout_denies` 在无外部交互时可能挂起；本次全量运行该用例
  通过（60s 内），如需可继续用 `-- --skip` 兜底。
- **真实 Key 人工 E2E 为跟踪项（非 CI）**：步骤见 `summary_and_next_fe.md`「人工验收」小节。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |
| ⑥ | 用量历史（服务端持久化 + 趋势） | `provider/usage` 由按需 TTL 300s 扩为服务端持久化采样 + 历史/趋势查询（协议加性）、Web/TUI 趋势视图与保留策略 | 无（基于 v0.27.0 用量面板；设计需先定采样归属/保留/加性 wire） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- v0.29.2（路由 API 化 + 生成器缺陷修复）已落地，全门禁绿（`version-ok`）。
- 待用户裁决：**`/v1/messages` 头部策略**（`x-api-key` 双发 / 仅 `x-api-key`）——确认后为
  单点改动（`anthropic.rs` 两处请求）+ 单测改断言 `header("x-api-key", ...)`，建议同版本内 patch。
- 后端候选：上表 ①②④⑥；或前端跟进见 `summary_and_next_fe.md`。
