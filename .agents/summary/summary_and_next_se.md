# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。
> 当前功能：**v0.25.1 opencode-go 订阅接入（一期）——后端部分（含验收修复）**
> ——来源：设计 `docs/superpowers/specs/2026-10-04-agentj-opencode-go-subscription-design.md`
> + 实现计划 `docs/superpowers/plans/2026-10-04-agentj-opencode-go-subscription-plan.md`（T0–T24）。
> 后端范围：新增第 18 个 crate `agentj-auth` + `agentj-core`/`agentj-config`/`agentj-model`/`agentj-ipc`/`agentj-cli` 扩展；
> `PROTOCOL_VERSION` 保持 **0.15.0**（52 schemas / 21 方法不变）；工作区 0.24.2 → **0.25.0** → **0.25.1**。
> 前端部分见 `summary_and_next_fe.md`。

## v0.25.1（验收修复，后端部分）✅

- ✅ **BUG-001（high）**：live `/zen/go/v1/models` 返回**裸 id**（`kimi-k3`/`minimax-m3`）与 overlay/
  `overrides()`/`default_model` 的 `opencode-go/<id>` 命名空间不一致 → `from_overlay_and_live`
  交集恒空、路由全部降级、`dev_format` 恒失配。修复：`catalog_source::fetch_live_models` 在
  单一上游边界把裸 id 规范化为 `opencode-go/<id>`（幂等，sort/dedup 前），下游自动对齐。
  提交 `fix: BUG-001 namespace live model ids`。
- ✅ **BUG-002（medium）**：`auth login --default-model` 在 `config.toml` 缺失时报原始 OS 错误。
  修复：`write_model_defaults` 遇 `ErrorKind::NotFound` 走「缺表则建表」路径。
  提交 `fix: BUG-002 create config.toml when writing model defaults`。
- ✅ 两个 Bug 均独立回归 `verified`；无新 Bug；每 Bug 一提交。

## v0.25.0（后端部分）✅（本次完成）

- ✅ **`agentj-auth`（新 crate）**：`Credential`（`api_key` / `oauth` 二期占位，未知 tag → `Unsupported` 而非 Malformed）+ `AuthStore`（XDG 路径、目录 `0700` / 文件 `0600`、临时文件 + fsync + rename 原子写、权限/JSON 损坏拒绝降级）+ `resolve_credential`（`--api-key` > `api_key_env` 非空 > `auth.json`，绝不返回空串）+ `redact`（仅末 4 位；≤4 字符折叠为 `…`）。
- ✅ **`agentj-core`**：`ModelProvider::models()` 默认 `vec![]`（加性零破坏）+ `ProviderRegistry::list_with_models()`（按名排序）+ `ModelOptions.session_id`（内部字段，`TurnRunner` 注入本轮 `SessionId`）。
- ✅ **`agentj-config`**：`toml_edit 0.22` 定点写回 `set_model_defaults` / `write_model_defaults`（保留注释与无关表）。
- ✅ **`agentj-model`**：`http`（统一 `agentj/<版本>` UA + `session_header` 规则）+ `gateway_error`（`{"type":"error","error":{type,message}}` → `类型: 消息`，raw 兜底）+ `catalog`（`verified` 字段 + `from_sources` live/overlay 合并，空 live 视为不可用）+ `catalog_source`（`fetch_live_models`，5s 超时）+ `catalog_gen`（overlay 生成器纯函数）+ 三 transport（UA、`x-opencode-session`、401/403 重登录提示、openai-compat 补齐 `Retry-After`）+ `OpenCodeGoProvider::with_catalog` / `models()` + `ProviderFactory::create_with_catalog` + `bin/gen_opencode_catalog`（seeded overrides，`--check`）+ Ollama 补统一 UA。
- ✅ **`agentj-ipc`**：`kernel/status.providers[].models` 改为由 `ProviderRegistry::list_with_models()` 填充（反映**已注册** provider，零协议变更）。
- ✅ **`agentj-cli`**：`agentj auth login|status|logout`（`--provider`/`--api-key` 或 stdin/`--default-model`/`--offline`；`auth` 在 config 加载与 license 门之前分发）+ `build_providers`（凭据解析 → 目录构建 → 跳过无密钥真实 provider → 空注册表硬错误 `no usable model provider`；provenance 行只打印来源 + 末 4 位）+ `run --attach` 不做本地 provider 装配。
- ✅ **e2e**：`e2e_auth`（login/status/logout 往返 + 0600 + 不泄漏完整 key + keyless 启动失败且不触网）；e2e_cli/e2e_ws 夹具补 `kind="mock"` provider 以适配新的硬错误行为。

## 验证结果（后端门禁，2026-10-06）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `cargo test --workspace` | **全绿**（**718 passed / 0 failed**；含 e2e_cli/e2e_serve/e2e_mcp/e2e_ws/e2e_http_static/e2e_permission/e2e_auth；`e2e_serve_exits_gracefully_on_sigterm` 为已知环境性 flaky，本次通过） |
| `cargo test -p agentj-protocol -- drift` | 2 passed（52 schemas） |
| `cargo test -p agentj-auth` | 13 passed（新增） |
| `cargo test -p agentj-cli --test e2e_auth` | 2 passed |
| `./scripts/check-version.sh` | version-ok（**0.25.1**；`PROTOCOL_VERSION` **0.15.0**） |
| `node agentj-ui/apps/desktop/check-desktop-ui.mjs` | desktop-ui-ok |

## 遗留问题 / 已知限制（后端）

- **overlay 未按 live 重新生成**：`models/opencode-go.json` 仍为人工 curation 的 20 条（无 `verified` 字段，`from_json` 默认 true）；`gen_opencode_catalog` 的联网生成/`--check` 属手动/CI 刷新，未在本次执行（避免 live 变化破坏已提交路由）。首次刷新后应确认 models.dev `npm` 路径与 live id 前缀。
- **`KEY_REQUIRED_KINDS` 反向白名单 fail-open**：未来新增真实 transport 若未加入白名单，会被当作免密钥注册（设计 §D7 的写法即如此）；如需硬化可改为显式 `KEYLESS_KINDS` 白名单。
- **启动期 `Flag` 密钥来源不可达**：`build_providers` 恒以 `flag=None` 调用（`Cli` 无启动期 `--api-key`），来源实际只可能是 `Env` / `File`。
- **serve e2e 非完全隔离**：启动会读取真实 `$XDG_DATA_HOME/agentj/auth.json`；仅 `e2e_auth` 覆盖 `HOME`/`XDG_DATA_HOME`，本机若存在损坏/权限不符的 `auth.json` 可能影响共享 serve 测试（CI 无此文件）。
- **`ollama` 未接入 `ProviderFactory`**（既有事实，本期未改）；已补统一 UA。
- **lockfile 版本元数据漂移**：`agentj-ui/package-lock.json` / `bun.lock` / `src-tauri/Cargo.lock` 中的 desktop 版本仍为旧值，无门禁读取，下次 install 时刷新。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ① | 指标导出扩展 | 计数持久化 / 跨实例共享、代理头解析 | 无（基于 v0.22.0 `/metrics`） | 待实施 |
| ② | 抽检扩展 | 逐请求校验 / 手动全量触发端点 | 无（基于 v0.22.0 运行期抽检） | 待实施 |
| ④ | rank 规则单一真源化 | Rust `file_list::search_rank` 为唯一真源，TUI / mock 镜像退役 | 无（前端跟进见 `_fe.md` 遗留 ③） | 待实施 |

> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- **opencode-go 订阅一期（0.25.1）已在开发分支落地**，验收 PASS（BUG-001/002 verified），待创建 PR 后由用户合并 main。
- 后端候选：opencode-go 二期（`GET /zen/go/v1/usage` 用量面板 / device-code OAuth / 运行期切模型 / 新协议方法），或上表 ①②④。
- 前端候选见 `summary_and_next_fe.md`。
