# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**G5 License gating + G6 MCP wiring (v0.9.0)**——设计文档 `docs/superpowers/specs/2026-09-03-g5-g6-license-mcp-wiring-design.md` + 实现计划 `docs/superpowers/plans/2026-09-03-g5-g6-license-mcp-wiring-plan.md`（v0.9.0 已发布）。

## 待实施

| 阶段 | 标题 | 依赖 | 优先级 | 状态 |
|------|------|------|--------|------|
| ✅ 完成 | IPC 协议契约（agentj-protocol + schemas + registry + spec + TS binding） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.1 核心内核（25 任务，v0.1.x 发布） | 无 | P0 | 已完成 |
| ✅ 完成 | v0.2 权限审批系统（13 任务，v0.2.x 发布） | v0.1 | P0 | 已完成 |
| ✅ 完成 | v0.3 生态扩展（T0-T11，v0.3.0 发布） | v0.1+v0.2 | P1 | 已完成 |
| ✅ 完成 | v0.4 商业扩展（守护内核常驻 serve / Web Dashboard / 用量统计 / License；v0.4.1） | v0.1-v0.3 | P2 | 已完成 |
| ✅ 完成 | v0.5 内核调度全重构（决策 #18 兑现，v0.5.0/0.5.1） | v0.4 | P0 | 已完成 |
| ✅ 完成 | G2 修复：单全局 in-flight 会话槽 → per-session 路由（T1-T6，v0.6.0） | v0.5 | P0 | ✅ 已实现 v0.6.0 |
| ✅ 完成 | G4 会话持久化生产接线（FileBackedStore 生产接入 + 唯一 tmp + 目录 fsync）（T0-T6，v0.7.0） | v0.6.0 | P0 | ✅ 已实现 v0.7.0 |
| ✅ 完成 | G3 token 级用量统计（TokenUsage + 4 提供方 + SessionRunResult.usage + CLI 激活）（T0-T11，v0.8.0） | v0.7.0 | P0 | ✅ 已实现 v0.8.0 |
| ✅ 完成 | **G5 License gating + G6 MCP wiring（[license] + McpServerSpec + close_all + per-command entitlement）**（T0-T8，v0.9.0） | v0.8.0 | P0 | ✅ 已实现 v0.9.0 |

## Phase: G5 License gating + G6 MCP wiring (v0.9.0) ✅ (本次完成)

### 进度与摘要

- ✅ **根因**：`agentj-license` 的 Ed25519 校验能力完备但全仓零生产依赖（CLI 无引用），商业门控悬空，无 `[license]` / `--license-key` / entitlement 链路；MCP 侧 `McpConnectionManager` 已有 `reap_idle` 但 `[[mcp.servers]]` 发现→`connect` 链路缺失，且 `connect` 仅接受 `sh -c` 单字符串，存在 shell 注入面。

- ✅ **方案**（设计 D1-D7）：G5 在 CLI 边界 fail-fast 门控（`[license] key` + `AGENTJ_LICENSE_KEY` env + `--license-key` flag，优先级 flag > env > config；`AGENTJ_LICENSE_PUBKEY` env 覆盖嵌入公钥；无 key 零门控 free mode，有 key 校验签名+过期+`run`/`serve`/`dashboard` entitlement）；G6 将 `connect` 升级为 `McpServerSpec{command,args,env}`（`Command::new` + args/env，消除 `sh -c`），新增 `close_all`，CLI 启动遍历 `[[mcp.servers]]` 逐个 connect，失败 `tracing::warn!` 并继续。

- ✅ **任务分解（T0-T8）已完成**：
  - T0 — `agentj-config` 新增 `[license] key`（`LicenseConfig` + `AgentJConfig.license` + merge `license` 分支 + `AGENTJ_LICENSE_KEY` env 覆盖 + `get_str("license.key")`）。
  - T1 — `agentj-cli` 新增 `license.rs`（`AGENTJ_LICENSE_PUBKEY` 常量 + `resolve_validator` env 覆盖 + `check_license_gate` flag>env>config + free mode + entitlement 校验，含 4 单测）。
  - T2 — `agentj-cli` 接线 `Cli.license_key` + `main` 分发前 `check_license_gate`（`run`→`run`/`serve`→`serve`/`dashboard`→`dashboard`，`run --attach` 不门控）+ `e2e_license.rs` 4 场景 + fixture `license_run.toml`。
  - T3 — `agentj-config` 新增 `[[mcp.servers]]`（`McpServerConfig{name,command,args,env}` + `McpConfig{servers}` + `AgentJConfig.mcp` + merge 分支，含 3 单测）。
  - T4 — `agentj-mcp` 引入 `McpServerSpec`（breaking）+ `connect(server_id, spec)` + `spawn_and_init(spec)`（`Command::new` + args/env）+ `close_all` + `TransportOp::Connect` spec 化 + `reconnect`/`close` 适配，含 args/env 回显单测 + `close_all` 单测 + 6 既有测试迁移。
  - T5 — `agentj-cli` 接线 `[[mcp.servers]]` 启动遍历（`mcp_servers` clone 于 `KernelContext` 前 + `connect` 循环 + `close_all` 于 run/serve_stdio/serve_tcp 收尾）+ `e2e_mcp.rs` 2 场景 + fixture `mcp_cli_echo.sh`。
  - T6 — workspace version `0.8.0` → `0.9.0`（`Cargo.toml` + `Cargo.lock` via `cargo build`）。
  - T7 — 文档同步 `AGENTS.md`/`README.md`/summary（本次）。
  - T8 — gap-analysis `G5/G6` 状态行更新为 ✅ 已实现 v0.9.0（并行）。

- ✅ **决策 D1-D7**：
  - D1 Key-optional 免费模式：无 key 不门控，向后兼容，社区可用。
  - D2 按命令细分 `run`/`serve`/`dashboard`：`entitlements: Vec<String>` 天然匹配，可组合售卖。
  - D3 `connect(spec)` 破坏性替换 + `close_all`：零生产调用者，spec 结构化消除注入面。
  - D4 单 server 失败 warn-and-continue：个别故障不拖垮整体，`Failed` 墓碑 + 重连兜底。
  - D5 验证公钥编译期常量 + `AGENTJ_LICENSE_PUBKEY` env 覆盖：e2e 注入 + 生产轮换。
  - D6 解析优先级 `--license-key` > `AGENTJ_LICENSE_KEY` > `[license] key`：与既有 env 覆盖先例一致。
  - D7 门控收敛 CLI 命令边界：IPC 零改动，serve 启动即校验，YAGNI per-request 复验。

- ✅ **验证结果**：
  - `cargo test -p agentj-protocol -- drift` — 2 通过（33 schemas 无漂移，协议零变化）
  - `cargo test --workspace -- --skip permission_request_notified` — 全量通过（含新增 license/mcp 单测 + e2e_license 4 + e2e_mcp 2 + 既有 e2e_cli/e2e_permission/e2e_serve 回归）
  - `cargo test -p agentj-config` — 通过（license + mcp 新测 + regression）
  - `cargo test -p agentj-mcp` — 通过（spec/close_all 新测 + 6 迁移 + lib）
  - `cargo test -p agentj-cli --test e2e_license --test e2e_mcp` — 通过（6 场景）
  - `cargo clippy --workspace --all-targets -- -D warnings` — 零警告
  - `cargo fmt --all --check` — 通过
  - `cargo test -p agentj-protocol -- drift` 二次确认 — 通过

### 遗留问题 / 已知限制

- serve 运行期 license 到期不中途吊销（设计非目标，D7 明确记录；需重启校验）。
- `close_all` 为尽力关闭（逐 server `close` + `kill_on_drop` 兜底，无强一致保证）。
- per-request 复验未做（`run --attach` 瘦客户端不门控，依赖远端 serve 的启动门控覆盖）。
- `AGENTJ_LICENSE_PUBKEY` env 覆盖为操作者可控轮换通道（能设 env 者本就拥有进程环境，非新增攻击面）。
- `McpServerSpec` connect 的 `args`/`env` 为原样透传，未做额外转义或鉴权（与既有 `bash` 工具权限模型分离）。
- `mcp_cli_echo.sh` / `mcp_echo_args.sh` 固件仅覆盖 `initialize`→`tools/list`→`tools/call` 最小链路。

### 下一步

- 按 gap 档案推进 G7（EventBus 重入 guard）与 G8（WebSocket 网关），或根据产品规划启动新 feature 设计 → `/jskill-feature-design`。
- 可选增强：serve 周期性 license 复验（到期主动下线）、`close_all` 超时控制、`[[mcp.servers]]` 热重载。
