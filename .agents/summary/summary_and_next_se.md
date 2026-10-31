# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录 **订阅用量默认 provider 修复（缺陷修复，后端侧）**——`PROTOCOL_VERSION` 仍
**`0.21.0`**（29 方法 / 81 schemas / 15 crate 不变，本批不改 wire 结构，只改默认值解析），
工作区版本仍 **`0.1.0`**（按 `AGENTS.md`：版本号不随修复递增）。

> 当前功能：**订阅用量「用量无法获取 / -32602 InvalidParams」双症状修复** —— 分支
> `fix/subscription-usage-default-provider`。

## 缺陷与根因 ✅

| 现象 | 根因 |
|------|------|
| 设置内用量提示「用量暂时无法获取（上游响应无法识别）」+ 左上角状态「额度用尽或获取失败 -32602: InvalidParams」 | 两者同源：`provider: null` 的 RPC 一律 `-32602`。`[model] provider` 缺省值 = 历史字面量 `"deepseek"`（`agentj-config` 的 `ModelSection::default()`），而实际配置的 provider 名是 `deepseek-chat` 与内置 `opencode-go`；`CliProviderService::cfg()` 直接把它当默认值 → `unknown_provider`。内核自己走 `ProviderRegistry` 默认 provider，所以「内核能跑、UI 报错」 |
| 为什么文案读作「上游响应无法识别」 | `useSubscriptionStatus` 探针失败套用了 `errorKind: "unavailable"`，而该 kind 在内核侧专指 `UsageError::Unavailable`（上游响应体不可解析），文案因此把 RPC 失败说成上游问题（前端侧修复，见 `_fe.md`） |

现场证据（debug 二进制 `serve --port 0` + WS 探针）：`auth/status` / `provider/usage` /
`provider/usageHistory` / `model/list` 在 `provider` 缺省时全部返回
`-32602 {field:"provider", reason:"unknown_provider", registered:["deepseek-chat","opencode-go"]}`；
显式 `{provider:"opencode-go"}` 四条全成功；`curl https://opencode.ai/zen/go/v1/usage` 返回 200
且形状被 `agentj-model` 的 `usage` 解析器正常识别——上游没有问题。

## 修复 ✅

`crates/agentj-cli/src/provider_service.rs`：

- 新增 `fn resolve_default_provider(configured: &str, cfgs: &HashMap<String, ProviderCfg>,
  registry: &ProviderRegistry) -> String`：`trim()` 后的配置名命中 `cfgs` 即采用；否则按
  **注册表默认 provider**（且命中 `cfgs`）→ **排序后首个 `opencode-go` 条目** → **排序后首个名字**
  回退；都为空则 `""`。设计要点：回退顺序与内核实际会跑的 provider 对齐（注册表默认），而不是猜一个名字。
- 新增 `fn go_cfg_name(cfgs)` + `fn sorted_names(cfgs)`：按能力解析。`provider/usage` /
  `provider/usageHistory` / `auth/deviceStart` / `auth/devicePoll` / `auth/setApiKey` /
  `auth/clearCredential` 六个方法只认 `opencode-go`，其 `provider: null` 现在取**已配置的 go 条目**
  （无 go 条目才回落到默认 provider）；显式传入的非 go 名字仍由 `require_kind` 报
  `unsupported_provider`。`auth/status` / `model/list` 继续报内核默认 provider（与内核实际运行的
  provider 一致）。起因：`docs/guide/configuration.md` 的示例配置 + 设了 `DEEPSEEK_API_KEY` 时
  注册表默认是 `deepseek-chat`，只按默认解析会让用量面板与设备码登录仍以
  `-32602 unsupported_provider` 失败。
- 解析改为**每次请求现算**：字段从 `default_provider` 换成 `configured_provider`（`[model] provider`
  原文），`cfg()` / `go_cfg()` 各自调用 `default_provider_name()` / `go_cfg_name()`，运行期
  `auth/setApiKey` 链接凭据后立刻生效；构造时只做一次 `warn_unmatched_provider()` 告警。
- 四条回归用例：`unconfigured_default_provider_falls_back_to_the_kernel_default`（配置名
  `"deepseek"` + `cfgs = {deepseek-chat, opencode-go}` + 注册表默认 `opencode-go` → 默认解析为
  `opencode-go`）、`keyless_kernel_still_answers_with_the_linkable_provider`（**无任何注册
  provider** 的全新内核 → `auth/status` 仍返回 `provider: "opencode-go"` 且
  `authenticated: false`）、`go_capability_rpcs_pick_the_go_provider_over_the_kernel_default`
  （注册表默认 `deepseek-chat` 时 `usage_history(None)` 仍解析到 `opencode-go`）、
  `auth_status_stays_on_the_kernel_default_provider`（同一配置下 `auth/status` 报 `deepseek-chat`
  且 `authenticated: false`）。前三例在修复前均以 `-32602` 失败。
- `omitted_provider_reaches_the_go_entry_and_an_explicit_non_go_one_is_rejected` 重写自旧用例：
  显式 `deepseek-chat` → `unsupported_provider` 且零上游请求；省略 provider → 链接到
  `opencode-go` 成功。

## 实测门禁（本批，2026-10-31）

| 门禁 | 结果 |
|------|------|
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `cargo test --workspace` | 全绿（含 `agentj-cli` 163 例） |
| `python3 scripts/tests/fn-length.py crates/agentj-cli/src/provider_service.rs` | 0 个 fn > 50 行 |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh all` | 3 + 6 + 10 = 19 项全 PASS |
| `./scripts/check-version.sh` | `version-ok` |
| 现场复验（`serve --port 0` + WS） | `auth/status` → `provider: "opencode-go"`, authenticated true；`provider/usage` → rolling 2% / weekly 2% / monthly 75%；`model/list` → 28 模型、无 error；`provider/usageHistory` → 无 error |

## 遗留问题 / 已知限制（后端）

- **`auth/status` 与订阅用量可能报不同 provider**：前者按内核默认为准（例如 `deepseek-chat`），
  后者按 go 条目解析。这是刻意的：`auth/status` 描述「内核会跑的 provider 的凭据」，用量只对
  go 有意义。若将来 UI 想让凭据行跟随订阅 provider，应显式传 `provider: "opencode-go"`。
- **配置名不可用只在启动时 `warn` 一次**：日志里能看到「已回退」，但 `/version` 的
  `config_warnings` 不含这条（那是 `Config::validate()` 的告警集合）；如需可观测，应在
  `validate()` 里把「`model.provider` 不是已配置 provider」升为告警项。
- **`provider: null` 的解析每次请求现算**：`cfgs` 是构造时的快照，运行期新增的 provider（仅
  `activation::activate_opencode_go` 注册进注册表、不进 `cfgs`）不会被该回退看到；当前链接目标固定为
  `opencode-go` 且它始终在 `cfgs` 里，所以不构成现实问题。
- **`check-artifacts.sh linux` 本地 `rc=1`**：需要 `src-tauri/target/release/bundle` 产物，
  属 CI `desktop-package` 作业，不在本地收尾门禁内。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑰⁺b | 真 BPE 词表 | 可插拔 `TokenCounter` 接缝 + 字符类校准权重已交付，权重仍是启发式；下一步按 provider 家族嵌入真词表（`tiktoken` 类），只换 `TokenCounter` 实现 | 需引入分词器依赖 / 体积权衡 | 待实施 |
| ㉟⁺b | 产物签名与公证 | 三平台产物已由 CI 真实构建并校验（`deb` / `AppImage` / `dmg` / `msi`），仍缺签名与公证（macOS notarize / Windows signtool） | 需证书与 CI secret 策略 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- 本修复已收尾、过门禁；分支推 `origin` 与 `gitcode`，PR 开在 `origin`（用户负责合并）。
- 若用户希望「配置名不可用」也进 `/version` 告警，可把该判断搬进 `Config::validate()`
  （会牵动 `config_warnings` 的既有断言，属独立小批）。
- 下一批候选（见上表）：⑰⁺b 真 BPE 词表、㉟⁺b 产物签名与公证。

> 远端提示：本机 `git fetch gitcode` 若报 `fatal: couldn't find remote ref gitcode`，按纪律
> 不 `--force`；分支照常 `git push gitcode <branch>:<branch>`，失败即如实记录。
