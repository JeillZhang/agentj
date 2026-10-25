# Summary & Next — Backend (SE / 后端)

> 本文件仅保留**当前正在开发**的后端功能内容：进度、剩余步骤、遗留问题、待实施、下一步。
> 后端范围：`crates/*`（Rust 内核 / 协议 / CLI）+ `schemas/` + `protocol/` + `docs/specs/`。
> 同步更新时**替换**（不要追加）前一功能内容。
> **强制例外（跨阶段保留）**：「## 待实施」表中**尚未实施**的特性条目必须保留（状态 =
> 待实施），不得随内容替换而删除；仅当该特性实际实施完成后才能移除（移入功能记录）。
> 前端总结独立存放于 `summary_and_next_fe.md`；两文件内容隔离，互不写入对方文件。

本文件当前记录**结构债收敛与三端纯函数共享（后端侧）**——wire 面零变更，工作区版本仍 `0.1.0`。

> 当前功能：**结构债收敛批次（后端侧）** —— 分支
> `refactor/structure-debt-consistency-batch`，计划 33 任务，后端侧为 T1–T15 + T31。
> `PROTOCOL_VERSION` 不变（无 wire 变更），`schemas/` 与 `protocol/methods.json` 未改。
> 设计与计划是本地文档（`docs/superpowers/{specs,plans}/2026-10-25-structure-debt-*`，不入库）。

## 结构债收敛批次（后端侧） ✅

### 交付

**A. `agentj-ipc/src/web.rs` 文件级拆分（T1–T10）**

- 4032 行单文件 → `crates/agentj-ipc/src/web/`：`mod.rs`（73 行，只声明模块与再导出）+
  `integrity.rs`(329) + `config.rs`(482) + `persist.rs`(104) + `auth.rs`(213) + `router.rs`(219) +
  `endpoints.rs`(158) + `prometheus.rs`(351)。对外路径 `agentj_ipc::web::*` 与 wire 行为不变。
- 84 个内联测试按域分到 `web/tests/`：`support.rs`（255 行共享夹具）+ `config` / `auth` /
  `router` / `endpoints` / `prometheus` / `persist` / `integrity` 七个文件，另加 `structure.rs`
  薄根 pin；`crates/agentj-ipc/src/tests/pins.rs` 的「已搬走符号不回潮」pin 现覆盖全部 8 个
  web 文件（防止逻辑回流 `mod.rs`）。ipc 测试 310 → 311。

**B. 权限结算单次化 + 无审批器立即拒绝（T12–T15）**

- `Notifier` 由 `Arc<dyn Fn(&PermissionRequestParams, &str)>` 改为返回 `bool`（是否投递成功）。
  `register_ask` 投递失败时立即以 `NO_APPROVER_REASON`（`no approver attached`）拒绝并退回占用；
  `settle` 只在 `finished.swap(true)` 抢占成功的一方产生副作用，因此重复/迟到答复既不能重复通知、
  也不能凭 `AllowAlways` 扩权。`cancel_session` 以断连理由结算并返回「该 owner 仍待决的条数」，
  `clear_all` 也会通知，但 owner 断连不动 session 级条目（沿用 P4 归属语义）。
- `agentj-cli`：非 TTY 且未加 `--yes` 时先打印一行
  `warning: no approver attached (non-interactive stdin, no --yes); approval-required tools will be denied`，
  随后当场拒绝，不再空等 `[permission] timeout_ms`；TTY 与 `--yes` 路径逐字节不变。`--attach`
  不走本机这条路径（审批归远端内核的订阅客户端）。`[permission] timeout_ms` 的语义收窄为
  「仅覆盖已投递但无人回答的请求」。
- `agentj-ipc` 单测夹具新增 `silent_approver()`（审批器在线但不答复），7 个原本依赖「无审批器
  保持待决」的夹具改用它；owner 断连测试断言拒绝理由不是 no-approver 文本。permission 单测
  58 → 66，ipc lib 305 + 6 集成，`cargo test -p agentj-cli --locked` 全绿（含 `e2e_permission`）。

**C. `glob` 超时改为结构化错误（T11）**

- `crates/agentj-tools/src/builtins.rs` 删掉 `Err(ToolError::Timeout)` → `Ok(String)` 的降级分支：
  超时与 bash/grep 一致按 `Error: tool timeout after Ns` 上抛，已走出的部分匹配一并丢弃，
  不再把失败伪装成一条成功的文本结果。

**D. 文档（T31）**

- `docs/guide/tools.md` 重写 glob 超时段落（超时会让本次调用失败、部分匹配丢弃）；
  `docs/changelog/v0.1.md` 补 Changed（web 拆分）与 Fixed（glob 超时、无审批器立即拒绝）。

### 门禁

`cargo test --workspace --locked` 全绿（1204 项）、`clippy --workspace --all-targets -- -D warnings`
零警告、`cargo fmt --all --check` 干净、`fn-length` 0 违规、`assert-{cargo,ui,ci}.sh all` 全 PASS、
`check-version.sh` version-ok。

## 归档：用量历史 + 模型窗口预算 + Desktop 打包验证 ✅

协议 `0.19.0 → 0.20.0`（加性）：`provider/usageHistory`（78 → 81 schemas、28 → 29 方法）+
`crates/agentj-cli/src/usage_history.rs`（真实上游应答才落盘，目录 0700 / 文件 0600）+
`[usage]` 三项配置；`context_window` 贯通 `CatalogEntry`/`ModelEntry` 并由
`budget::effective_budget`（`min(显式上限, 窗口 × 90%)`）逐轮生效；CI 新增 `desktop-package`
（`tauri build` + `check-artifacts.sh linux`），仓库不变量 17 → 18。详见 `docs/changelog/v0.1.md`。

## 归档：依赖刷新批次 ✅

分支 `chore/dependency-refresh-batch`（自 `c82c679`），11 个 dependabot PR 合成一次变更。

### 交付

- **Rust 依赖**：`libloading` 0.9、`base64` 0.23、`toml` 1.1（`agentj-config` / `agentj-plugin`）、
  `sha2` 0.11（`agentj-ipc` / `agentj-cli`）、`criterion` 0.8（基准改用 `std::hint::black_box`）、
  `reqwest` 0.13（`agentj-model` + `agentj-auth`）。
- **TLS 选型**：reqwest 0.13 默认 `rustls` 会引入 `webpki-root-certs`（`CDLA-Permissive-2.0`
  不在许可白名单）与 5 条 rustls 公告，故 `default-features = false` + `native-tls`；
  `agentj-auth` 额外开 `form` feature（OAuth 表单提交）。
- **`deny.toml`**：新增 8 条带理由的 duplicate skip（`digest` / `block-buffer` / `crypto-common` /
  `cpufeatures` / `sha2` 0.10、`base64` 0.22、`toml_datetime` 0.6 + `winnow` 0.7），删除因 rustls
  退出依赖图而失效的 `core-foundation` skip。
- **前端依赖**：react 19.3、vite 8.3、vitest 5（四个 workspace 同一大版本）、jsdom 30、
  `@testing-library/jest-dom` 7、`lucide-react` 1.60、`@opentui/*` 0.6、`@tauri-apps/cli` 2.12。

### 验证

| 命令 | 结果 |
|------|------|
| `cargo test --workspace --locked` | 48 组 `test result: ok`，无失败 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 警告 |
| `cargo deny check advisories licenses bans sources -D unmatched-skip` | advisories / bans / licenses / sources 全 ok |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh all` | 3 / 5 / 8 passed，0 failed |

### 遗留问题 / 已知限制（本批次）

- `deny.toml` 的 8 条 duplicate skip 都是上游阻塞（`ed25519-dalek` / `sha1` 钉 `digest` 0.10、
  `axum` 钉 `base64` 0.22、`toml_edit` 钉旧 `toml_datetime` + `winnow`）；上游同步后应逐条删除，
  `-D unmatched-skip` 会在删早了或删晚了时直接失败。
- jsdom 30 声明 `engines.node ^22.22.2 || ^24.15.0 || >=26`，本机 Node v24.14.1 安装时会打印
  EBADENGINE 警告（非致命）。

## 归档：健壮性与去重批次 ✅

分支 `chore/hardening-dedup-batch`，14 个提交（`d13ebab..808424f`）。

### 交付

- **防中毒锁 helper**（`crates/agentj-core/src/sync.rs`，新模块）：`lock_or_recover` /
  `read_or_recover` / `write_or_recover`（中毒即 `tracing::error!` + `into_inner()`）+
  `panic_payload_message`（`&str` / `String` 负载，回退 `<non-string panic payload>`，
  `PANIC_MESSAGE_LIMIT = 200` 按字符截断）。全仓 233 处 `.lock()/.read()/.write().unwrap()/expect()`
  与既有 `.unwrap_or_else(|e| e.into_inner())` 全部收敛：core / session / permission / question /
  model / ipc / cli；`agentj-session/src/file_store.rs:53` 保留等价本地 helper。
- **长驻任务 panic 隔离**：`kernel::route_message` 抽出 + `catch_unwind`（单条消息 panic 不杀守护
  循环）；`IpcServer::dispatch_request` 抽出 + `isolate_panic`（panic → `-32603`
  `InternalPanic`，`error.data { method, reason }`）；`TurnRunner::spawn_turn` 包住 `run_turn`
  （panic → `AgentEvent::Error` + 释放 `turns` 槽位，不再永久 `SESSION_BUSY`）。
- **登录门加固**（`crates/agentj-ipc/src/secure.rs`，新模块）：`ct_eq` 常量时间比较替代
  `form.token == expected`；`session_cookie_header` / `clear_cookie_header` 单点构造 cookie
  （`HttpOnly; SameSite=Lax; Path=/` + 可选 `Secure` + 可选 `Max-Age=0`），序列化失败记日志而非
  `expect`；`Retry-After` 同改。
- **`[web] secure_cookies`**：`agentj-config` `WebSection` → `TcpBindOptions.secure_cookies` →
  `WebConfig::with_secure_cookies` → cookie 属性；CLI `TcpServeOptions.secure_cookies` 在
  `serve` / `dashboard` 两条装配路径注入。文档落在 `docs/guide/http-and-auth.md`。
- **SSE 分派收敛**（`crates/agentj-model/src/sse.rs`）：`SseFrame` / `decode_frame` /
  `SsePayload` trait / `push_line` 驱动；responses / anthropic / openai-compat 三份 `push_line`
  只剩 `crate::sse::push_line(self, line, out)`，payload 解释留在各自 `push_payload`。
- **响应序列化去 `expect`**：`to_wire` helper（失败 → `-32603`）；轨迹写入遇不可序列化事件记
  日志跳过并推进 seq。
- **约定落地**：`CONTRIBUTING.md` 新增「Panics, locks and swallowed errors」章节（锁 / panic
  隔离 / `let _ =` 三条规则），`AGENTS.md` Conventions 同步一行。
- **验收修复**：`e7718dc` BUG-001 补上换行写法（`.lock()` 换行接 `.unwrap()`）的 48 处漏改
  （含 16 处生产路径），并加不变量测试 `kernel::tests::modules_survive_a_poisoned_state_mutex`；
  `862ec69` 把最后 12 处内联 `.unwrap_or_else(|p| p.into_inner())`（含 3 个测试 `env_lock`）
  也改走 helper，保证「中毒必打日志」全仓一致。

### 验证

| 命令 | 结果 |
|------|------|
| `cargo fmt --all --check` | 干净 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 警告 |
| `cargo test --workspace` | 48 组 `test result: ok`，无失败 |
| 单 crate | core 184 → 185（含 `modules_survive_a_poisoned_state_mutex`）、ipc 277 → 281、model 75 → 79、session 34、permission 49、question 21、config 43 |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 59 passed（7 files，含新增 blankText） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 677 passed（54 files） |
| `bun --cwd agentj-ui/apps/tui test` | 193 pass / 0 fail |
| `npm --prefix agentj-ui run build --workspace=@agentj/web` | 通过；oxlint 仅既有 warning |

### 遗留问题 / 已知限制（本批次）

- `let _ =` 仍 165 处（lib 代码 58 处）：本批次只**约定**语义、未逐处改写；后续按约定在触碰时收敛。
- `sync.rs` 之外仍有 3 处 `panic!`/`expect` 属启动期常量（`agentj-model/src/catalog.rs:68`
  内嵌目录解析、`gen_opencode_catalog` 生成器、`http.rs:17` 客户端构造）——不在请求路径，未改。
- 常量时间比较不防护「长度差异」以外的时间侧信道（长度本身可由计数侧信道获得），限流仍是主防线。
- `Secure` 属性由部署方显式开启（`[web] secure_cookies`），未做「按 `X-Forwarded-Proto` 自动判定」。

## 归档：工程治理批次 ✅

分支 `chore/engineering-governance-batch`。对应待实施 ⑮ ⑳ ㉛ ㉜ 的实施落地（本侧）；
㉑ ㉒ ㉕ ㉝ ㉞ 属前端侧，见 `summary_and_next_fe.md`。

### 交付

- **仓库断言 harness**（`scripts/tests/assert-ui.sh` / `assert-cargo.sh` / `assert-ci.sh`）：
  `--list` / `<check>` / `all` / `--strict` 四种入口，未实现项以 `SKIP`（exit 3）显式暴露、
  `--strict` 视为失败。17 个检查覆盖 UI 清单与单一 lockfile、workspace Cargo 策略与 15 crate
  继承、LICENSE 三件套、`deny.toml`、lockfile 一致性（`cargo metadata --locked`）、CI 作业契约、
  覆盖率棘轮、frontend 作业、治理文件、工作区卫生、两份 summary 与文档基线。
- **CI 作业补全**（`.github/workflows/ci.yml`）：新增 `deny`（cargo-deny 四类检查）、
  `cross-platform`（ubuntu / macOS / Windows，仅 `cargo build --workspace --locked`）、
  `frontend`（17 步：`npm ci` → protocol/sdk 生成漂移 → protocol 构建/测试 →
  sdk/tokens/web 测试 → web 构建 →
  `dist/version.json` ↔ `protocol/methods.json` 协议版本一致性 → oxlint → TUI 测试 →
  desktop UI 校验 → `check-version.sh` → `check-relocate.mjs`）；`lint` 追加「Repo invariants」
  （`python3 -m pip install --user pyyaml` + `taiki-e/install-action@cargo-deny`
  + 三份断言脚本 `--strict`，`SKIP` 即失败）；`deny` 作业命令带 `-D unmatched-skip`
  （cargo-deny 默认只对陈旧 `bans.skip` 告警且 `exit 0`）。
- **覆盖率棘轮**：实测行覆盖 **93.67%**（`cargo llvm-cov report`，2026-10-23：lines 36671/2321、
  regions 93.45%、functions 86.95%），`COVERAGE_THRESHOLD` 由 `50` → **92**
  （`max(50, floor(X) - 1)`，注释记录日期与基线）；`cargo llvm-cov` 与
  `cargo build -p agentj-plugin-fixture` 均加 `--locked`。
- **依赖 / 许可治理**：`deny.toml` —— advisories / licences / bans / sources 四类；licence 白名单
  14 项（MIT、Apache-2.0、BSD、ISC、Zlib、BSL-1.0、CC0-1.0、Unicode-3.0、Unlicense、MPL-2.0 等）；
  `multiple-versions = "deny"` + 6 条带来源注释的 `skip`（`getrandom` 0.2/0.3、`r-efi`、
  `rand_core` 0.6、`core-foundation` 0.9、`syn` 3）；`[sources]` 用 `allow-registry` 只放
  crates.io。`LICENSE` / `LICENSE-MIT` / `LICENSE-APACHE` 落地 **MIT OR Apache-2.0** 双许可。
- **社区治理**：`CONTRIBUTING.md`（本地门禁清单 + 不可提交项）、`SECURITY.md`
  （GitHub Security Advisories 上报通道、7 天响应）、`.github/dependabot.yml`
  （cargo / npm / github-actions，minor+patch 各聚合成一个 PR）。
- **Cargo 工作区策略**：`[workspace.dependencies]` 收敛 12 个三方依赖、`[workspace.lints.clippy]
  all = { level = "warn", priority = -1 }`、`[profile.release]` `lto = "thin"` +
  `codegen-units = 1`（**不设** `panic = "abort"`，保留 `catch_unwind`）、
  `[workspace.package] license = "MIT OR Apache-2.0"`、`clippy.toml` `msrv = "1.96"`；
  18 个 crate 全部改为继承式清单（依赖 `workspace = true`、`license.workspace = true`、
  `[lints] workspace = true`），42 处内部 path 依赖补 `version = "0.1.0"`（消除 wildcard 告警）。
- **版本元数据**：`scripts/check-version.sh` 新增 `AGENTJ_ROOT` 覆盖、`agentj-ui/package.json` /
  `apps/web` / `apps/tui` / lockfile 根条目断言、通用 `0.2x` 字面量守卫；失败打印出错文件、
  期望值与命中字面量（不再静默 `exit 1`）。`scripts/test-check-version.sh` 用临时目录夹具覆盖
  正/负样例。
- **工作区卫生**：删除 `agentj-ui/node_modules/.old_modules-*` 迁移残留、仓库根空 `node_modules/`、
  `.playwright-mcp/`；`.gitignore` 补 `.opencode/`（AI 工具运行时状态，机器本地）。
- **远端命名**：`origin` → GitHub（唯一权威）、`gitcode` → 只读镜像，与 `AGENTS.md`
  「Remote & Merge Policy」一致；分支按纪律推送两个远端，只在 `origin` 合并。

### 验证

| 命令 | 结果 |
|------|------|
| `cargo fmt --all --check` | FMT_OK |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 警告 |
| `cargo test --workspace --locked` | 48 组 `test result: ok`，无失败 |
| `cargo deny check advisories licenses bans sources -D unmatched-skip` | advisories / bans / licenses / sources 全 ok，无 `unmatched-skip` |
| `cargo metadata --locked --format-version 1` | 通过（307 packages，`Cargo.lock` 无漂移） |
| `bash scripts/tests/assert-{ui,cargo,ci}.sh --strict` | 全部 PASS，0 SKIP |
| `sh scripts/test-check-version.sh` / `./scripts/check-version.sh` | `check-version-tests-ok` / `version-ok` |
| 前端：sdk 55、protocol 4、tokens、web 681、TUI 192 + oxlint + `check-desktop-ui.mjs` + `check-relocate.mjs` | 全绿 |

### 测试验收（两轮 + 修复轮）

`jskill-test-acceptance`：第 1 轮测试 8 个缺陷（2 high / 4 medium / 2 low）、第 1 轮回归再报
5 个 low/nit、第 2 轮回归报 1 个 medium（阻塞）+ 6 个 low/nit，合计 **20 条，19 修复 + 1 有理由
不修**（`allowlist` 里 4 个许可为预审批名额，`license-not-encountered` 仅 warning），每缺陷一个提交。

- **安全**：`tokio` 1.53.1 命中 `RUSTSEC-2026-0386`（unsound，`AsyncFd::new` I/O safety）→ 升
  `1.53.3`（仅 `Cargo.lock`）；否则 `deny` 作业首跑即红。
- **失效门禁**（本轮主要收获，均为「断言恒真 / 检查没跑」类）：
  - `check_coverage_gate` 先 `join` 再 `splitlines()`，两条 `--locked` 断言恒真；
  - `assert-ci.sh` 的 check 清单无自检——删一个 check 仍 `7 passed` exit 0；
  - `unmatched-skip` 守卫在 CI 恒 `SKIP`（`lint` 未装 cargo-deny，且脚本用 `all` 不失败）→
    `lint` 装 cargo-deny + `--strict`、`deny` 作业加 `-D unmatched-skip`；
  - `CONTRIBUTING.md` 的「CI runs the same commands」不成立（漏 tokens 测试 / oxlint / 三份断言
    脚本 / `check-desktop-ui.mjs` / `check-relocate.mjs`）→ 清单改为从 `ci.yml` 派生并强制全覆盖。
- **死指针 / 漂移**：`deny.toml` 曾指向 gitignore 的 `docs/superpowers/`；改用
  `CONTRIBUTING.md` 新增的「Dependency and licence policy」章节（白名单、`yanked` 默认 deny、
  skip 必须命中真实重复、wildcard、sources、dependabot），断言校验指针被 git 跟踪、文档含关键词、
  文档白名单与 `deny.toml` 集合相等；失效的 `getrandom =0.2.17` skip 删除；`yanked = "warn"`
  撤销（回到工具默认 deny）。
- **文档失真**：两份 summary 归档段残留「origin=GitCode / 命名相反」、`## 下一步` 仍把 T21/T22
  当待办——已改写并加禁用短语断言；`@agentj/protocol` 测试此前从未在 CI 执行（现在会）。

### 遗留问题 / 已知限制（本批次）

- `cargo-deny` 不在开发机默认工具链（需 `cargo install cargo-deny --locked`，实测 0.20.2）；
  CI 走 `taiki-e/install-action@cargo-deny`。
- `frontend` 作业的 CI 首次运行尚未观测；本地已逐步跑通同一 17 步序列。
- 覆盖率基线 93.67% 为单次实测——后续若测试减少，棘轮会立即转红（预期行为，需重新基线化）。
- `cross-platform` 只做 `cargo build`，不做打包（dmg/msi 见待实施 ㉟）。
- **T22 远端收敛 BLOCKED**：`git push origin main` 被 GitHub 拒绝（非快进）——远端 `main` @
  `d883a79`（PR #90 合并）与本地 `main` @ `5062736`（!67 合并）是同一份内容的**两个 merge
  commit**，三端 tree 完全相同（`4e72b5a6b47df679d70489244abe70a3c59b5ed1`）。按纪律**未使用
  `--force`**、未改写任何远端历史。重命名已生效：`origin` = GitHub（权威）、`gitcode` = GitCode
  （只读镜像），`main` 已 `--set-upstream-to=origin/main`，`gitcode/main` 与本地 `main` 一致。
  收敛方式待用户决定：① 在 `main` 上 `git merge origin/main`（生成一个内容零变化的合并提交，
  之后 `git push origin main` 即为快进，不改写历史）；② 重置镜像或权威端的 `main`。

## 归档：run 可靠性批次 ✅

分支 `feature/run-reliability-batch`（自 `e924473` merge 后起）。六项特性 + 协议加性变更一次收尾，
对应待实施 ⑦⑧⑨⑭⑱⑲ 的实施落地；`PROTOCOL_VERSION` 升 **0.19.0**、工作区仍 0.1.0。

### 交付

- **工具结果落库上限**（待实施 ⑦）：`crates/agentj-core/src/budget.rs:147 fn truncate_chars` 按字符
  预算保留头部并追加 `…[已截断 {dropped} 字符，原文 {total} 字符；请缩小查询范围]…`
  （`budget.rs:24-25 TOOL_RESULT_TRUNCATION_NOTICE`）；唯一截断点 `turn_runner.rs:468`（构造
  `ToolResult` 处）；`[agent] max_tool_result_chars` 默认 `100_000`、钳制 `1_000..=1_000_000`
  （`crates/agentj-config/src/lib.rs`）；截断时发内核内事件
  `AgentEvent::ToolResultTruncated { tool_call_id, dropped_chars }`（`crates/agentj-core/src/types.rs:120`，不上 wire）。
- **`session/run` 回显生效 provider / model**（待实施 ⑧）：`AgentEvent::Done` 携带 `provider` /
  `model`（`crates/agentj-core/src/types.rs:135-136`）；wire `SessionRunResult` 加性新增
  `provider` / `model`（`crates/agentj-protocol/src/session.rs:60-65`，
  `#[serde(default, skip_serializing_if = "Option::is_none")]` → 未知时整键省略、绝不为 `null`）；
  轨迹新增 `TrajectoryValue::RunModel`（每轮至多一条）（`crates/agentj-protocol/src/trajectory.rs`，
  `crates/agentj-ipc/src/lib.rs` 记录）；`ProviderRegistry` 补 `set_default` / `default_name` /
  `get_default_with_name`（`crates/agentj-core/src/module.rs:487-507`）。
- **`glob` 路径语义（进程内执行）**（待实施 ⑨）：删 `find` 子进程，`crates/agentj-tools/src/glob.rs`
  进程内 `globset` + `walkdir`；pattern 不含 `/` 按路径分量 basename、含 `/` 按根相对全路径
  （`literal_separator(true)`：`*` 不跨 `/`、`**` 跨）；跳过目录沿用 `GrepTool::SKIP_DIRS`；输出
  `./<相对路径>` 逐行排序、上限 `2000`（超出追加 `# 已截断 N 条命中（上限 2000），请缩小 pattern`）；
  默认超时 60s、`timeout_ms` 可覆盖（钳制 1s–600s）；超时以 `glob timed out after Ns` 文本结果回报而非错误。
- **`ask` 默认超时**（待实施 ⑱）：`[agent] question_timeout_s` 默认 `600`、钳制 `1..=86_400`
  （`crates/agentj-config/src/lib.rs`）；CLI 注入 `QuestionModule::set_default_timeout_s`，未显式给
  `timeout_s` 的提问超时后按 **skipped** 结算（`crates/agentj-question/src/module.rs`），不再永久挂起。
- **CLI 审批与配置健壮性**（待实施 ⑲）：`run --yes` 自动批准并在 stderr 打印
  `auto-approved <tool> (--yes)`；TTY 下 `approve <tool>? <精简参数>` / `  reason: <原因>` /
  `[y/N] ` 打到 stderr、另起线程读 stdin（`y`/`yes` 批准，其余含 EOF 拒绝 → 拒绝理由
  `denied by user`）；非 TTY 且未加 `--yes` 不读 stdin、不挂审批器，待决请求按
  `[permission] timeout_ms` 预算自动拒绝；`timeout_ms` 钳制 `100..=600_000`（越界 stderr 警告、
  省略 = `60_000`）。
- **e2e 测试环境隔离**（待实施 ⑭）：共享 `TempEnv`（`crates/agentj-cli/tests/common/mod.rs`）把
  `HOME` / `XDG_DATA_HOME` / `XDG_CONFIG_HOME` 指向临时目录并移除凭据类环境变量，
  `e2e_cli` / `e2e_permission` / `e2e_auth` / `e2e_serve` 全部改用——测试不再读写真实 `~`。
- **协议版本**：`PROTOCOL_VERSION` **0.19.0**（仍 28 方法 / 78 schemas）；schemas 重新生成、
  `@agentj/sdk` `PROTOCOL_VERSION` 同步 0.19.0（`VERSION` 仍 0.1.0）。

### 验证（最终门禁）

| 门禁 | 结果 |
|------|------|
| `cargo test --workspace` | 全绿（`failed = 0`；新增 `glob` / 截断 / `session/run` 回显 / 提问超时 / CLI 审批 / e2e 隔离 测试） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warning |
| `cargo fmt --all --check` | 干净 |
| `cargo test -p agentj-protocol -- drift` | 全绿（78 schemas 无漂移） |
| `npm --prefix agentj-ui test --workspace=@agentj/sdk` | 55 passed（6 files） |
| `npm --prefix agentj-ui test --workspace=@agentj/web` | 681 passed（55 files） |
| `bun --cwd agentj-ui/apps/tui test` | 192 pass / 0 fail（22 files） |
| `./scripts/check-version.sh` | `version-ok`（T19 已把 `AGENTS.md` 与脚本断言同步到 0.19.0） |

### 真实提交（`e924473..HEAD`，最旧 → 最新）

- `fdf5421` test(e2e): add shared TempEnv helper and adopt it in e2e_cli
- `6e2685d` refactor(e2e): adopt shared TempEnv helper in permission, auth and serve suites
- `2817385` feat(tools): add in-process glob matcher and walker
- `51594ab` refactor(tools): run glob in-process and drop the find subprocess
- `f293f8e` feat(core): add char-budget truncation helper
- `06f0c4a` feat(config): add [agent] max_tool_result_chars
- `6d10033` feat(core): truncate oversized tool results once
- `9f0ae54` feat(config): add [agent] question_timeout_s
- `c8ef0cd` feat(protocol): add provider and model to SessionRunResult and a RunModel trajectory value
- `bc603e7` feat(question): make the default ask timeout configurable
- `15d5cd4` feat(core): report the effective provider and model on run completion
- `082b07d` chore(protocol): bump PROTOCOL_VERSION to 0.19.0
- `4bca0a9` chore(protocol): regenerate schemas and sync generated client types
- `0da5465` chore(sdk): sync PROTOCOL_VERSION with the 0.19.0 wire bump
- `effd538` feat(cli): add --yes and a stdin approver for interactive runs
- `a1f8125` test(ipc): restore partial-window paging coverage
- `f508471` feat(ipc): surface the effective provider and model on run replies
- `69074f2` feat(web): show the effective model reported by the kernel
- `f0b510e` docs: cover the run-reliability behaviour in specs, changelog and guides
- `306922c` docs: update the CLI guide and the (se/fe) summaries after the batch
- `f604697` docs: sync the AGENTS.md baseline and the version gate to protocol 0.19.0
- `b661b80` polish: address the accumulated review minors from the run-reliability batch
- `03dcc9e` fix: BUG-001 inject the question timeout default on every assembly path
- `bb52cd7` fix: BUG-002 clamp a negative permission timeout instead of failing the parse
- 本提交 docs: sync the config guide and the se summary after the acceptance fixes

### 遗留问题（非阻塞）

- **非 TTY 未加 `--yes` 仍等到 `timeout_ms` 才拒绝**：本次只消除「永久挂起」，未做「立即拒绝」——
  计划 AC 措辞「auto-denied instead of waiting」与实装「等 `[permission] timeout_ms` 预算耗尽再拒」
  不符，已按实装记录（guide 与 changelog 均为「按预算自动拒绝」）。
- **EOF 无独立「no input」文案**：计划/AC 写作 `Deny { reason: "no input" }`，实装里 EOF 只是映射为
  普通 `Deny`，最终拒绝理由是权限引擎的 `denied by user`（`crates/agentj-permission/src/module.rs:150`）。
- **`glob` 超时以文本结果回报**：`glob timed out after Ns` 是 Ok 文本而非 Err，模型看不到「工具超时」
  错误语义（与 grep/bash 的超时 Err 不一致）；属既有取舍，可选跟进。
- **`[permission] timeout_ms` 上界收窄为 `i64`**：验收 BUG-002 为让负值不被 serde 拒绝而把字段改为
  `Option<i64>`（`crates/agentj-permission/src/policy.rs:82`），因此 `> i64::MAX` 的 TOML 整数字面量在
  解析阶段报 `number too large to fit in target type`（修复前 `Option<u64>` 可接受再钳制）。量级约
  3.2 亿年，验收判 low 并**接受现状**；`docs/guide/configuration.md` 已写明取值须在 64 位有符号整数范围内。
- **`e2e_mcp` 会写真实数据目录**：偶发在真实 `~/.local/share/agentj` 留下 `metrics-state-<port>.json`，
  不在本次 ⑭ 改造的 4 个 e2e 文件范围（范围外观察项，未处理）。
- **TUI 测试改写真实配置**：`bun --cwd agentj-ui/apps/tui test` 会改写真实 `~/.config/agentj/theme.json`，
  属前端既有问题（本批次明确不触碰 TUI），未处理。

## 已完成（0.29.0 – 0.29.12 + BUG-017/018 + 抽检扩展 + search-rank 单一真源化 + run 可靠性批次，详见 `docs/changelog/v0.1.md`）

- ✅ run 可靠性批次（本阶段，见上「run 可靠性批次 ✅」）：工具结果落库上限 + `session/run` 回显
  生效 provider/model + `glob` 路径语义（进程内）+ `ask` 默认超时 + CLI 审批/配置健壮性 + e2e
  隔离；`PROTOCOL_VERSION` 0.19.0。
- ✅ search-rank 单一真源化（已合并 PR #89）：文件搜索排名收敛到 Rust `file_list::search_rank`
  唯一真源 + 跨语言契约夹具 `protocol/search-rank-cases.json`（Rust 契约测试 2 条 + TUI 镜像退役 +
  mock kernel 夹具回放）；TUI 192 passed、Web 679 passed（未触碰）、check-version ok。
- ✅ 抽检扩展（逐请求校验 + 手动全量端点）：`[web]` 4 键（`spot_interval_secs` / `spot_sample` /
  `verify_on_request` / `verify_cache_ttl_secs`）+ `IntegrityOptions` + 逐请求资产校验中间件
  （memoize + 绝对 TTL + 上限 1024、fail-open 计数 / 去重告警、未鉴权零哈希）+ `POST /integrity/verify`
  手动全量 + `/version` 加性 `integrity.on_demand` + 8 个加性指标；`serve` / `dashboard` 两路装配。
- ✅ 0.29.8：工具往返落库（每轮 `assistant{tool_calls}` + `tool` 结果写入会话，「继续」不再失忆）
  + 截断流（无 `Done` 的纯文本轮）重放 ≤2 次后如实报错；前端输入框 chip 跟随实际选择。
- ✅ 0.29.9：工具轮次用尽后追加一趟「无工具收尾」（`conclude_without_tools`），
  `[agent] max_tool_iterations` 从死配置变为可配（钳制 `1..=1000`）。
- ✅ 0.29.10：`text_tool_call` 把模型写成纯文本方言（`<rfl:parameter>` / `<tool_call>`）的
  工具调用还原成真正的调用；无法还原时纠偏 2 次后如实报错，而不是当结论。
- ✅ 0.29.11：Web 模型选择器不再提供虚构 `deepseek-chat` 占位（前端，详见 `_fe.md`）。

- ✅ 0.29.7：请求 token 预算（`budget.rs` + `ContextPipeline::fit` + 超限收紧重试）——长会话
  不再把整段历史原样发给上游后才被 400 拒绝；`[model] max_context_tokens` 可配（默认 128k）。

- ✅ 0.29.0：Web/TUI 链接 opencode-go API Key（协议 0.18.0 / 28 方法 / 78 schemas）。
- ✅ 0.29.1：验收 BUG-001（env 遮蔽时读侧 / 发送侧不一致）。
- ✅ 0.29.2：opencode-go 路由改为 API 派生（models.dev `provider.npm`），删除手写路由表；
  快照迁入 `crates/agentj-model/data/opencode-go.json`（45 条 / 35 verified）。
- ✅ 0.29.3：`/v1/messages` 鉴权头改双发（`x-api-key` + `Authorization: Bearer`）。
- ✅ 0.29.4：真实 key 端到端调测适配（4 个上游缺陷 + 1 个测试挂起）——`catalog_source::bare_id()`
  去 `opencode-go/` 前缀；`responses` 文本/工具 delta 改「字符串优先、旧对象形状容忍」；
  新增 `retry::send()` 温度类 400 去参数重试一次；`gateway_error::api_error` 收敛三份私有副本
  + `429` → `RateLimited`；`[permission] timeout_ms` 使 e2e 审批超时降到 800ms，
  `e2e_permission.rs` 改用真实 `session_id` + 20s 截止通道（修复前**无限挂起**，现 0.85s）。
  真实 key 实测 9 模型 × 3 路由全绿（含 `glob` 命中唯一标记文件 `./UNIQUE_MARKER_7f3a.txt`）。
- ✅ 0.29.5：`serve` / `dashboard` 不再强制预先配置密钥（`providers::NO_USABLE_PROVIDER`
  只作为警告，`main.rs` 仅对无可录入客户端凭据的命令 fatal；`run` 行为不变）。
- ✅ 0.29.6：断连重连后的 `SessionBusy`——`settle_owner_pending` 在**每条**连接收尾路径
  （EOF / 传输错误 / shutdown）结算该 owner 的 question 与 permission 待决项（reason
  `disconnected`），`ConnectionRegistry::clear_owner` 在排空**前**撤销归属（排空期间新审批
  立即被拒），`attach_permission_notifier` 无 owner 可达即刻拒绝；Web 重连即重新
  `session/subscribe` + `SessionBusy` 友好文案；`pump_requests` 借用 `&mut Box<dyn Connection>`
  使连接活过排空（否则写方向被 shutdown，响应丢失）。

## 遗留问题 / 已知限制（后端）

- **抽检扩展 E4 的判别力**：`tokio::time::interval` 首 tick 立即触发，故 e2e 只能断言「15s 内出现
  **第二**轮」才真正证明 `[web] spot_interval_secs` 到达 `spawn_spot_check`（已按此实现）；若将来
  把抽检改成 `sleep` 语义需重新校准该断言。
- **`temp_dist` 固定 `/tmp` 目录**：`agentj-cli-e2e-web-{tag}` 仅按 tag 区分，同 tag 并发跑同一测试
  目标（或残留服务）会互踩；既有模式，未在本阶段改动。
- **fixture 基线重复**：`web_verify_off.toml` / `web_fast_spot.toml` 各自复制了
  `permission_deny.toml` 的完整基线（含 `timeout_ms = 800`），基线变更时需同步三处。
- **计划文本笔误（抽检扩展 T2）**：计划 AC4 写 `mismatched is 2`，正确值为 1（代码与测试是对的）。
- **远端命名（已由工程治理批次解决）**：当时实测 `origin` 指向 GitCode、`github` 指向 GitHub，
  与 `AGENTS.md`「Remote & Merge Policy」相反；现 `origin` = `git@github.com:JeillZhang/agentj.git`
  （唯一权威）、`gitcode` = `git@gitcode.com:jeill/agentj.git`（只读镜像）。feature 分支仍推
  两个远端，PR 开在 `origin`。
- **文档滞后（本阶段有意未动 `AGENTS.md`）**：`AGENTS.md` 的 `agentj-config` 条目尚未列新增 `[web]`
  配置节，`agentj-ipc` 条目尚未提 `client_ip` / `metrics_state` 两个新模块（本阶段文档落在
  `docs/specs/ipc-protocol.md` + `docs/guide/http-and-auth.md` + `docs/changelog/v0.1.md`）；下次
  结构性触碰时补齐。
- **状态目录默认值文档不完整（实测）**：`docs/specs/ipc-protocol.md:58` 与
  `docs/guide/http-and-auth.md:63,82` 只写默认 `$XDG_DATA_HOME/agentj`，而
  `crates/agentj-cli/src/main.rs:146 fn metrics_state_dir_from` 的实装回退链为 `[web] state_dir`
  → `$XDG_DATA_HOME/agentj` → `$HOME/.local/share/agentj`（代码自身的启动警告亦写「set
  XDG_DATA_HOME, HOME or [web] state_dir」）；下次触碰该文档时补 HOME 回退。
- **错误路径收尾的测试只覆盖到 helper 内部**（终审 MEDIUM）：`crates/agentj-cli/src/main.rs` 的
  新测试 `shutdown_flushes_metrics_state_even_when_the_serve_loop_failed` 直接调用私有
  `shutdown_tcp_server`，因此**能**钉住「`run_result?` 不得早于 `flush_metrics_state()`」这一
  顺序回归（把 `?` 前移即 RED）；但它**不**驱动 `serve_tcp`，故若回归重新在调用点写回
  `tcp.run().await?`（`main.rs:447` 一类），该测试仍会通过。要覆盖这一层需让 `serve_tcp` 以
  可失败的 `run()` 端到端跑通（当前无该夹具），故仅记录、不再加测。
- **`[web] state_dir = ""` 未做配置校验**（验收实测）：`metrics_state_dir_from`
  （`crates/agentj-cli/src/main.rs:146`）对空 `$XDG_DATA_HOME` / `$HOME` 有过滤，但**显式空**
  `state_dir` 会返回 `Some(PathBuf::from(""))`；后果为每次 debounce flush 均失败（
  `warning: cannot persist login metrics state to metrics-state-<port>.json (io error: No such
  file or directory)` + `agentj_web_metrics_state_write_failures_total` 增长），不写文件、
  不改 cwd 权限、不崩溃。字面满足验收 AC（`<dir>` 即 `state_dir`），故未计 Bug；跟进项：
  加载配置时把空串归为 `None` 并复用「无状态目录」启动警告。
- **flush 与变更之间存在理论竞态（未复现）**：`flush_metrics_state`
  （`crates/agentj-ipc/src/web.rs:412-424`）先快照、后**无条件** `mark_clean()`；若某次限流变更
  恰好落在两者之间，该变更会被当作「已持久化」。可由下一次变更或关机时的无条件 flush 补回，
  仅「在该窗口内 SIGKILL」才真正丢一次增量；因无法确定性复现，故仅记录。
- **预算为近似估算**：`estimate_tokens` 仍是「ASCII / 4 + 非 ASCII × 1」的字符启发式，非真实
  tokenizer；本批已把**天花板**换成真实窗口（`ModelEntry.context_window` → `effective_budget =
  min(显式上限, 窗口 × 90%)`），但单条消息的 token 数仍可能被低估，故仍保留 `retry_budget`
  在上游超限时收紧重试（最多 2 次）。目录之外的 id 仍无窗口，退回显式上限或 128k。
- **裁剪即丢失**：超预算时最老的会话历史与（同轮内）较早的工具结果会被丢弃，正文超长时中段
  被截断；仅在日志/行为上可观测，UI 无「已裁剪」提示。
- **文本方言还原只按参数名推断**（0.29.10）：泄漏模型已确认为 **`beam-free`**（用户
  2026-10-17），但泄漏随机、无法稳定复现（前缀回放 / 完整 harness run 都返回原生
  `tool_calls`），因此还原能力靠**参数名**推断工具（`command`→bash 等）；多参数调用（如
  write 的 path+content）与未知参数名仍走「纠偏重发 → 如实报错」。
- **grep 回退不读 `.gitignore`**（0.29.12）：无 `rg` 时的进程内检索只跳过固定目录
  （`.git`/`target`/`node_modules` 等），不解析 `.gitignore`；命中结果仍受 2000 条上限约束。
- ~~`e2e_ws` 端口竞态~~：0.29.7 已修复（`serve --port 0` + 读回绑定地址；并行复现 1/3 → 3/3）。
- **2 条 legacy 路由**：`qwen3.6-plus` / `qwen3.7-max` 按 models.dev 判为 `openai-compat`
  （旧手写表标 `anthropic`），二者均不在官方在售列表内；如实测不符，加回一条种子即可。
- **温度降级只重试一次**：`retry::send` 仅处理「仅 temperature 不支持」的 400；若模型同时拒绝
  其他参数，仍按原错误上抛（不盲目剥参）。
- **凭据时序**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——换入瞬间进行中的那一轮 run
  可能仍使用旧凭据，新凭据从下一轮生效。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 即远端主机）；
  非回环 `serve --host` 必须配 `--token`。
- **进程型工具报错里的 `name` 变成程序名**：`NotFound` / spawn 失败时 executor 把
  `ProgramMissing.name` 填成**程序**（`crates/agentj-tools/src/executor.rs:27-30`：bash → `sh`、
  grep → `rg`），而改动前填的是工具名（`bash` / `grep`）；管道读失败则是 `"child"`
  （`executor.rs:71,78`）。错误码与变体未变（无 wire 影响），但与设计 G4「错误面不变」有偏差，
  属可选跟进。（`glob` 已进程内化，不再 spawn `find`，故不再有 `glob → find` 映射。）
- **`ToolExecutor::run_blocking` 只能在阻塞线程里调**：它自建 runtime 后 `block_on`
  （`crates/agentj-tools/src/executor.rs:220-237`），当前唯一生产调用方是 `spawn_blocking`
  （`crates/agentj-core/src/module.rs:451`，线程未设 `EnterRuntime::Entered` 故安全）；若将来在
  异步上下文（多线程 worker / `block_in_place`）直接调 `Tool::execute` 会 panic。
- **MCP 子进程链路仍未接线**：`crates/agentj-mcp/src/connection_manager.rs:222` 仍是裸
  `tokio::spawn` 管理子进程，未走 `ToolExecutor` 的进程组 / 超时 / drop 回收；MCP server 挂起
  同样不会被杀整组（后续跟进）。
- **bash 默认 120s 对冷编译偏紧**：Rust 全量 `cargo build` / 大测试套件可能超过 120s；模型可用
  入参 `timeout_ms`（上限 600s）覆盖，默认值是否上调待观察。
- **不泄露**：结果只经 `redact()` 暴露 `key_hint`；完整 key 不出现在结果 / 日志 / 错误文案 /
  前端持久化中；两个 auth 方法**不新增权限门**。
- **`cancel_session` / `clear_all` 必须先收集再结算**（本批实现取舍）：
  `PendingRegistry::for_session` / `prune_where` 持注册表锁执行回调，回调里若直接 `settle` / `get`
  会自锁，故 `crates/agentj-permission/src/module.rs` 先收集 request id 再逐个结算；后续若要在这两个
  入口加带副作用的谓词需沿用同一步骤。
- **no-approver 只在投递那一刻判定一次**：审批器投递成功后才断连的情形仍按 `timeout_ms` 到期拒绝
  （`--attach` 断连改走 `cancel_session` 的断连理由）；日志可区分
  `permission denied: no approver` 与 `permission approval timed out`。
- **`web` 拆分后的运行期盲点**：`crates/agentj-ipc/src/web/` 现按域分层，跨域行为（例如 metrics 落盘
  ↔ 限流计数）仍需在 `web/tests/integrity.rs` 与 `web/tests/persist.rs` 两侧同时改动；`structure.rs`
  只钉薄根，不阻止域内继续变胖（最大 `config.rs` 482 行）。
- **不做**：多账号 / 账号切换（同 provider 重复链接 = 替换）；OS keyring；不做「从解析层 `Err`
  反推 entitlement」（`rejected` 仅由明确 401/403 触发）。

## 待实施（跨阶段保留：未实施不得删除）

| # | 特性 | 内容 | 依赖 | 状态 |
|---|------|------|------|------|
| ⑰⁺ | 真 tokenizer | 把 `estimate_tokens` 的「ASCII/4 + 非 ASCII×1」启发式换成真 tokenizer（已接模型窗口，估算本身仍是启发式） | 需引入分词器依赖 / 体积权衡 | 待实施 |
| ㉟⁺ | 【债】macOS / Windows 打包链路 | CI 只真实构建并校验 Linux deb/AppImage；`dmg` / `msi` 仍无 CI 回归保护 | 需 macOS/Windows runner + 签名/公证策略 | 待实施 |
| ㊱⁺ | 【债】`run --attach` 的审批归属回归 | `--attach` 的审批归远端内核订阅客户端，本进程不打印 no-approver 告警；owner 断连后重新认领、以及「投递失败但会话仍在」的组合无专门 e2e | 需要跨进程审批会话夹具 | 待实施 |
| ㊲⁺ | 【债】`grep` 进程内检索的时间预算 | 无 `rg` 时的 `search_native` 只有 2000 条命中上限，没有像 `glob` 那样的 60s deadline，超大树上可能长时间不退 | 把 `glob` 的 deadline 模式移植到进程内检索 | 待实施 |

> 类型标记：`【债】` = 工程债 / 质量门禁，`【修复】` = 已确认缺陷或元数据不一致；无标记 = 特性。
> 规则：本表条目在未实施时必须**跨阶段保留**（不得随内容替换删除）；实施完成后从本表移除，
> 并记入对应功能章节。下一特性选择（`jskill-feature-next`）合并本表与 `_fe.md` 待实施表。

## 下一步

- 本批次（结构债收敛，后端侧 T1–T15 + T31）已收尾：`web.rs` 拆分、权限单次结算 + 无审批器立即
  拒绝、`glob` 超时结构化、文档同步全部落地并过门禁；分支推送 `origin` 与 `gitcode`，
  PR 开在 `origin`（用户负责合并）。
- 下一批候选（见上表）：⑰⁺ 真 tokenizer、㉟⁺ macOS/Windows 打包链路、㊱⁺ `--attach` 审批归属回归、
  ㊲⁺ `grep` 进程内检索 deadline。
- 表外可选改进：用量历史分页 / 截断标志 / 按 provider 的保留期差异；`[web] state_dir = ""` 归为
  `None` 并复用「无状态目录」告警；`flush_metrics_state` 快照与 `mark_clean()` 之间的理论竞态；
  把「工具结果已截断」作为可观测事件透出（当前仅内核内 `AgentEvent::ToolResultTruncated`）。

> 远端提示：本机 `git fetch gitcode` 若报 `fatal: couldn't find remote ref gitcode`，按纪律
> 不 `--force`；分支照常 `git push gitcode <branch>:<branch>`，失败即如实记录。
