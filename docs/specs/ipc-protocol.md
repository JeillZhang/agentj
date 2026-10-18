# AgentJ IPC 协议规范

> 本文档是 AgentJ 内核对外暴露的统一 UI 接口协议（IPC）的**人类可读权威规范**。
> 机器可读工件（`schemas/*.json` 与 `protocol/methods.json`）是本规范的机器事实来源，见 [机器可读模型](#10-机器可读模型)。
> 协议当前版本：`0.18.0`。

## 1. 范围与一致性

本文档定义 AgentJ 内核（服务端，Server）与任意语言前端（客户端，Client）之间的通信协议。协议由三部分构成：

- **传输层**：每种传输各自的帧格式（stdio / TCP 使用 NDJSON，WebSocket 使用原生帧）。
- **基础协议**：传输无关的 JSON-RPC 2.0 消息信封、错误对象与错误码。
- **方法面**：本规范支持的 28 个方法（21 个既有 + 5 个订阅方法 + 2 个凭据链接方法；注册表 `protocol/methods.json` 收录全部方法，含 `session/subscribe` / `session/unsubscribe`（见 8.11 / 8.12）、`session/list`（8.18）、`session/history`（8.19）、`file/list`（8.20）、`$/askQuestion`（8.21）、`question/respond`（8.22）、`model/list`（8.23）、`auth/status`（8.24）、`provider/usage`（8.25）、`auth/deviceStart`（8.26）、`auth/devicePoll`（8.27）、`auth/setApiKey`（8.28）、`auth/clearCredential`（8.29））及其参数 / 结果结构。

本文档适用于所有传输。任何传输都必须遵循基础协议与方法面的全部规则。

本文档中的关键词 MUST（必须）、MUST NOT（禁止）、SHOULD（应当）、SHOULD NOT（不应）、MAY（可以）按 [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) 解释：

| 关键词 | 含义 |
|--------|------|
| MUST | 绝对要求，若不满足则实现不合法 |
| MUST NOT | 绝对禁止 |
| SHOULD | 在特定情形下可忽略，但忽略前必须理解并权衡后果 |
| SHOULD NOT | 同 SHOULD，表示"通常不应" |
| MAY | 完全可选，由实现自行决定 |

本规范的结构遵循 LSP 文档组织方式，章节划分与 `docs/superpowers/specs/2026-08-14-agentj-ipc-protocol-design.md` 的「协议规范结构」一致。

## 2. 传输与帧

消息**内容**（JSON-RPC 信封 + 方法面）完全传输无关；每种传输只定义自己的**帧格式**。未来新增传输（如 WebSocket 网关）时，方法面零改动，只新增一个帧适配器。

### 2.1 stdio（v0.1）

stdio 是 v0.1 唯一正式启用的传输。内核进程通过标准输入读取请求、通过标准输出写入响应与通知。

- 帧格式为 **NDJSON**：每行一条 JSON 消息，以换行符 `\n` 分隔。
- JSON 字符串内的换行 MUST 转义为 `\n`（即 `\\n` 的字面两字符序列），因此消息体本身不会包含裸换行，行边界安全。
- 每条消息 MUST 是单个有效的 JSON 值（对象或数组），同一行内不得包含多条消息。
- 消息 MUST 为 UTF-8 编码。

### 2.2 TCP（v0.4 定稿，守护内核）

TCP 传输沿用 NDJSON 帧格式，每条消息一行。多客户端共享内核的能力由守护内核设计定稿（`docs/superpowers/specs/2026-08-15-agentj-daemon-kernel-design.md`）：`agentj serve --port` 常驻内核，通过统一的 `Connection` 抽象（stdio / TCP 均为实现）服务多个并发客户端；TCP 绑定默认 `127.0.0.1`（本机信任模型），可选 `--token`（`initialize` 的 `token` 字段校验，不匹配 → `-32806 NotAuthorized`）、`--max-connections`（默认 16，超限新连接直接拒绝）。连接生命周期、订阅广播、审批定向语义见 §4.5、§7 与 §8 对应方法节。

### 2.3 WebSocket（v0.10.0）

WebSocket 与 raw TCP 同端口共存，通过连接分类器按首字节区分 HTTP Upgrade 与 NDJSON 帧。浏览器可直接连接，无需额外网关进程。

- **入口**：`ws://127.0.0.1:<port>/ws`（与 raw TCP 同端口共存，连接分类器按首字节区分 HTTP 与 NDJSON）。
- **帧语义**：每条 WebSocket Text 消息 = 一条 JSON-RPC 消息（原生帧，无 `\n` 分隔）。
- **鉴权双通道**：upgrade 阶段 `?token=` 校验（不匹配 → HTTP 403）；连接后 `initialize.token` 经 `auth_gate` 复验（`-32806 NotAuthorized`）。
- **Web UI 静态托管（HTTP 层，v0.19.0）**：`agentj serve --web-dist <dist>`（或 `AGENTJ_WEB_DIST` / 自动发现 `<cwd>/agentj-ui/apps/web/dist` / 可执行文件相对路径）在**同一端口**托管 Web UI：`/` 返回 `index.html`、`/assets/*` 长缓存（`Cache-Control: public, max-age=31536000, immutable`）、未知页面路径回落 index（200；`/assets/*` 子树无回落，错拼资产 404）。`--web-dist` / `AGENTJ_WEB_DIST` 显式来源无效 → 警告 + 静态面禁用（**不静默回落**自动发现）；自动发现全部落空 → 404 + 启动 hint。
- **登录门（HTTP 层，v0.19.0）**：`/login`（`GET` 内嵌表单 / `POST` 校验 token）与 `/logout`（`POST` 撤销会话 + 清 cookie）；会话 cookie 名 `agentj_session`，值为不透明随机 ID（服务端内存集合，重启失效），属性 `HttpOnly; SameSite=Lax; Path=/`（会话 cookie，无 `Max-Age`）。
- **静态资源鉴权（HTTP 层，v0.19.0）**：非回环 peer 且已配置 token 时静态资源经登录门——无/无效 cookie → `302 /login?next=<同源相对路径>`；回环 peer 免登录；未配置 token 免登录。
- **WS cookie 通道（HTTP 层，v0.19.0）**：`/ws` upgrade 除既有双通道外新增第三条通道——cookie（须携带**同源** `Origin`，其 authority 必须等于 `Host`）+ 有效会话即可升级；cookie 通道连接免 `initialize.token`。`?token=` 与 `initialize.token` 两通道语义不变；无有效通道仍 403。
- **版本探针 `GET /version`（HTTP 层，v0.20.0，未鉴权）**：返回 `200 application/json` 且 `Cache-Control: no-store`；JSON 形状 `{product_version, protocol_version, dist_present, dist: {product_version, protocol_version, build_time} | null}`（`product_version` = 内核二进制版本，`protocol_version` = 编译期协议常量；`dist` = 挂载 dist 的 `version.json` 事实，缺失或不可解析时为 `null`，此时仍返回 200）。与 `/login` 同类显式注册为路由（**不置于登录门后**），并**优先于 SPA fallback**——否则会被 `index.html` 以 200 吞掉。
- **登录限流（HTTP 层，v0.20.0）**：`/login` POST 按来源 IP 计数失败尝试，第 5 次失败起指数锁定（`min(30s × 2^(n−5), 900s)`，即 30s 起、×2、封顶 15min）；锁定期响应 `429` + `Retry-After: <秒，向上取整、≥1>`，且**不校验 token**（避免时序侧信道），锁定期内的新尝试继续升级锁定；登录成功清零。限流状态**进程内 + 单文件快照**：路径 `$XDG_DATA_HOME/agentj/metrics-state-<port>.json`（目录 `0700` / 文件 `0600` / 同目录 tmp + `rename` 原子写；JSON `version: 1`，上限 1 MiB），`[web] state_dir` 可覆盖；`serve` 启动恢复、运行期 5s debounce 落盘、优雅退出强制落盘。口径为 **per-process**——同机多实例按端口分片各写各的，聚合请在 Prometheus 侧做；文件缺失按首次运行处理（不警告），损坏 / 超 1 MiB / `version != 1` → 警告一次 + 从零开始，**绝不阻断启动**。
- **客户端 IP 派生与反代信任（HTTP 层）**：`[web] trust_proxy_headers`（默认 `false`）；开启后**仅当直接 TCP peer 是回环**才采信 `X-Forwarded-For`（从右向左取第一个非回环合法项；回环跳数视为代理自身）/ `X-Real-IP`。派生 IP **只用于识别**——限流键、per-IP 序列标签与日志字段；**永不参与鉴权**：`auth_required()` 的回环免鉴权判据始终只吃真实 TCP peer，请求头无法用它换取免登录。无代理头的普通回环请求按 `source="peer"` 计数但**不**计为派生失败；信任生效且确有代理头却解析不出转发值时 +1 `agentj_web_proxy_header_invalid_total`。
- **限流可观测性（HTTP 层，v0.21.0）**：`login_limiter` 在状态变化点发出 tracing 事件——`warn!` 仅锁定**新触发**（避免攻击流量下日志洪泛）、`info!` 成功登录（含 `prior_failures`）、`debug!` 逐次失败与锁定期拒绝明细；进程内聚合计数经 `GET /version` 加性暴露（见下）。CLI 日志订阅器改用 `EnvFilter`（`RUST_LOG` 可配，未设时默认 `info`，输出与 v0.20.0 逐字一致）。限流策略语义（阈值/锁定/429/Retry-After/成功清零）零变化。
- **`GET /version` 加性字段（HTTP 层，v0.21.0）**：既有形状 `{product_version, protocol_version, dist_present, dist}` 追加两块（未鉴权语义与 `Cache-Control: no-store` 不变；**不含 per-IP 明细**）：
  - `login_limiter`：`{attempts, failures, lockouts, blocked, locked_now, tracked_ips}`。`attempts` = 到达 limiter 的 `/login` POST 次数（含成功、失败、锁定期拒绝）；`failures` = token 校验失败次数（不含锁定期拒绝）；`lockouts` = 锁定**新触发**次数（`locked_until` 由非锁定态进入锁定态，延长不计）；`blocked` = 锁定期被拒请求数（未校验 token）；`locked_now` / `tracked_ips` = 快照时处于锁定中的 IP 数与 entries 条目数（扫描推导）。
  - `integrity`：`{manifest_present, checked, mismatched, missing}`（见下）。
- **资产完整性清单 `asset-manifest.json`（构建产物 + 内核启动校验，v0.21.0；v0.22.0 覆盖扩展）**：构建期在 `dist/` 生成 `{"algorithm":"sha384","assets":{"/path":"sha384-<base64>"}}`，覆盖全部 Rollup bundle 产物（entry chunk、动态 chunk、CSS 及被引用资产）与 **SRI 注入之后**重写过的 `/index.html`；`assets` 键必须为以 `/` 开头、不含 `..` 的规范服务路径（违反 → 清单视为不可解析），值格式与 SRI `integrity` 同源（单遍哈希，逐字节一致）。**v0.22.0 起覆盖 dist 实目录全部部署文件**——`writeBundle` 枚举 dist 实目录中未入清单的其余文件（含 `public/` 透传，如 `/favicon.svg`）并合并（bundle 产物键优先，同键 public 项跳过）；**排除** `version.json`（由版本比对覆盖）、`asset-manifest.json` 自身与名称以 `.` 开头的隐藏文件/目录；枚举中文件不可读 → 构建失败。内核启动读取清单并逐文件比对哈希：缺失 / 不可解析 / 哈希不符 / 文件缺失 → 追加 `startup_warnings()` 警告（打印 stderr，**不阻断服务**；定位为部署完整性，非安全边界）；摘要进 `GET /version` 的 `integrity` 块（`manifest_present` = 是否存在且可解析、`checked` = 已校验文件数、`mismatched` = 哈希不符数、`missing` = 清单列出但文件缺失数）。
- **指标导出 `GET /metrics`（HTTP 层，v0.22.0，鉴权门控）**：与 `/version` 并列的显式路由（先于静态面/登录门与 SPA fallback；**不依赖 dist 存在**）。门控 `auth_required()` = 已配置 token ∧ 非回环 peer ∧ 无有效会话 cookie；**未通过 → `401 text/plain` + `Cache-Control: no-store`（不重定向，脚本/监控友好）**，通过（回环 / 未配 token / 有效 cookie）→ 200。响应 `Content-Type: text/plain; version=0.0.4; charset=utf-8` + `Cache-Control: no-store`；每条指标附 `# HELP` / `# TYPE`；无 per-IP 时序历史（仅当前条目快照）。暴露面分五组：
  - 限流计数（counter）：`agentj_login_attempts_total`（到达 limiter 的 `/login` POST 次数，含成功/失败/锁定期拒绝）、`agentj_login_failures_total`（token 校验失败，不含锁定期拒绝）、`agentj_login_lockouts_total`（锁定**新触发**，延长不计）、`agentj_login_blocked_total`（锁定期被拒请求数）。
  - 限流快照（gauge）：`agentj_login_locked_ips`（快照时锁定中的 IP 数）、`agentj_login_tracked_ips`（entries 条目数）。
  - per-IP 序列（带 `ip="..."` 标签；上限 `PER_IP_SERIES_LIMIT = 50`）：`agentj_login_ip_failures_total`（counter）、`agentj_login_ip_locked`（gauge 0/1）、`agentj_login_ip_retry_after_seconds`（gauge，未锁定 = 0）、`agentj_login_ip_last_seen_seconds`（gauge，距该 IP 上次出现的秒数）；`agentj_login_ip_series_truncated`（gauge 0/1，per-IP 序列是否被 Top-50 截断）。排序 `locked desc → failures desc → ip asc`；聚合 gauge 恒精确，全量明细经 `RUST_LOG=debug` 日志下探。
  - 资产完整性（gauge）：`agentj_web_manifest_present`（清单存在且可解析 0/1）、`agentj_web_assets_checked` / `agentj_web_assets_mismatched` / `agentj_web_assets_missing`（启动全量校验计数）；抽检累计（counter）：`agentj_web_spot_rounds_total`（轮数）、`agentj_web_spot_checked_total`（累计校验文件数）、`agentj_web_spot_mismatched_total` / `agentj_web_spot_missing_total`（累计不符 / 缺失数）。
  - 客户端 IP 派生与限流状态持久化：
    - `agentj_web_client_ip_source_total{source="peer"|"forwarded"|"real_ip"}`（counter，按来源分辨）；
    - `agentj_web_proxy_header_invalid_total`（counter，信任生效且有代理头但解析失败）；
    - `agentj_web_metrics_state_restored`（gauge 0/1，启动是否成功恢复）；
    - `agentj_web_metrics_state_entries_restored`（gauge，恢复的 per-IP 条目数）；
    - `agentj_web_metrics_state_write_failures_total`（counter，落盘失败次数）；
    - `agentj_web_metrics_state_last_flush_unix`（gauge，上次成功落盘的墙钟秒，未落盘 = 0）。
- **运行期资产抽检（HTTP 层，v0.22.0）**：`TcpIpcServer` 启动后按 `SpotCheckConfig`（`interval` 默认 60s、`sample` 默认 8，测试可注入）`tokio::spawn` 周期任务，以排序键游标轮转抽样 `sample` 个清单文件比对 `sha384`（全量随轮转有界覆盖）；哈希不符计入 `mismatched`、文件缺失计入 `missing`，并 `warn!`（按路径进程内去重防刷屏）；manifest 缺失/不可用 → 任务不启动。结果经 `GET /version` 的 `integrity.runtime` 与 `GET /metrics` 双面暴露。
- **`GET /version` `integrity` 加性嵌套 `runtime`（HTTP 层，v0.22.0）**：既有 `integrity` 块追加嵌套 `runtime`：`{rounds, checked, mismatched, missing, last_run_age_secs?}`——前四者为运行期抽检累计（`u64`，未运行过为 0），`last_run_age_secs` 为距上次抽检的秒数（`Option<u64>`，**未运行过则省略**）；manifest 缺失/不可用 → `runtime` 全零。既有字段（`manifest_present`/`checked`/`mismatched`/`missing`）逐字保留，`/version` 未鉴权语义与 `Cache-Control: no-store` 不变。
- **说明**：以上 HTTP 层行为（`/metrics`、抽检、清单覆盖）本身不改变 wire 契约与方法/schema；同版本另有三处**加性 wire 变更**（`file/list` 的 `query`/`total`、`SessionNewResult.project_root`、`SessionStatusResult.project_root`，见 §8.6 / §8.8 / §8.20 与 §11.3），`PROTOCOL_VERSION` 升至 `0.14.0`。
- **生命周期**：WS 连接不参与「最后连接断开 → 全局关闭」（浏览器会话短，不决定内核存亡）；`max_connections` 对 WS 独立计数。

### 2.4 消息大小上限

- v0.1 默认单条消息上限为 **16 MiB**（MUST 接受小于等于该值的消息）。
- 超过上限的消息：接收方 SHOULD 丢弃该消息并以错误响应回应（若可确定请求 id，返回 `-32600 Invalid Request`）；若无法确定 id（如解析失败），则按 JSON-RPC 2.0 §5 规则返回 `id: null` 的错误响应，或直接关闭传输。
- 实现 MAY 通过配置调整该上限，但调整值 SHOULD 不低于默认值。

### 2.5 传输关闭语义

- **进程退出**：客户端发送 `shutdown`（请求）后，服务端返回结果并开始优雅关闭；随后客户端发送 `exit`（通知），服务端进程退出。`exit` MUST 在 `shutdown` 之后发送。
- **stdio EOF**：stdin 到达 EOF 视为对端主动断开。若发生在优雅关闭流程之外，服务端 SHOULD 完成当前请求处理后退出，MUST NOT 再写入输出。
- **多客户端语义（v0.4 常驻内核，`@since 0.3.0`）**：多客户端模式下，`shutdown` 请求触发**全局**内核优雅关闭链（SIGINT/SIGTERM 信号仍可强制，决策 #13 语义不变）；**最后一条连接断开**同样触发内核优雅关闭链。单个客户端断开（非最后一条）不影响其它连接。
- **异常断开**：任一侧可随时关闭传输；对端 MUST 将任何未完成的请求视为已失败，且不应重试不可重试的请求（如 `session/run`）。常驻内核下，断线客户端的 run 继续执行，重连者用 `session/subscribe` 观测而非重试。该连接拥有会话的**待审批权限**（reason `disconnected`）与**待答提问**在断开时即被结算（不再等满审批超时），run 随之继续而非悬挂。
- 服务端退出前 SHOULD 关闭传输流（刷新 stdout、关闭 stdin）。

## 3. 基础协议

基础协议严格遵循 JSON-RPC 2.0，并叠加 AgentJ 的强化规则。

### 3.1 消息信封

所有消息都是 JSON 对象，`jsonrpc` 字段 MUST 为字符串字面量 `"2.0"`。

**RequestMessage（请求）**

```json
{ "jsonrpc": "2.0", "id": 1, "method": "session/status", "params": { "session_id": "s-1" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `jsonrpc` | string | 是 | 固定 `"2.0"` |
| `id` | integer \| string | 是 | 请求标识，见 3.2 |
| `method` | string | 是 | 方法名，见 3.5 |
| `params` | object | 否 | 一律按名（by-name）对象，不使用位置参数 |

**NotificationMessage（通知）**

```json
{ "jsonrpc": "2.0", "method": "$/progress", "params": { "token": 1, "value": { "kind": "text", "delta": "你好" } } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `jsonrpc` | string | 是 | 固定 `"2.0"` |
| `method` | string | 是 | 方法名 |
| `params` | object | 否 | 同请求 |

通知**没有 `id`**，因此接收方 MUST NOT 对通知返回任何响应。

**ResponseMessage（响应）**

```json
{ "jsonrpc": "2.0", "id": 1, "result": { "status": "active" } }
{ "jsonrpc": "2.0", "id": 1, "error": { "code": -32802, "message": "session not found" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `jsonrpc` | string | 是 | 固定 `"2.0"` |
| `id` | integer \| string \| null | 是 | 回显对应请求的 id |
| `result` | any | 二选一 | 成功结果；`result` 与 `error` MUST 互斥 |
| `error` | ErrorObject | 二选一 | 错误对象，见 3.3 |

### 3.2 Id 规则

- `id` 类型为 integer 或 string（整数值 SHOULD 不使用 `-1`，因其在部分 JSON-RPC 实现中有特殊含义；AgentJ 保留负整数 id）。
- 请求 `id` MUST NOT 为 `null`。
- **会话内永不复用**：客户端 MUST 保证在同一连接生命周期内，每个未完成请求的 `id` 唯一；服务端 MAY 将复用 id 视为协议错误。
- 服务端响应 MUST 回显与请求完全相同的 `id` 值。
- **解析错误特例**：当消息无法解析、无法确定请求 id 时，错误响应的 `id` MAY 为 `null`（JSON-RPC 2.0 §5）。除该情形外，响应 `id` 不得为 `null`。

### 3.3 ErrorObject

```json
{ "code": -32602, "message": "invalid params", "data": { "session_id": "missing field" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `code` | integer | 是 | 错误码，见 3.4 |
| `message` | string | 是 | 人类可读的错误描述（不承诺机器可解析，机器判断靠 code） |
| `data` | object | 否 | 结构化上下文，预留字段级校验错误、会话 id、模块 id 等 |

`data` 的结构在方法面各方法的 `error.data` 小节中定义；未定义时 MAY 省略。

### 3.4 错误码表

错误码沿用 JSON-RPC 保留段，并在其下方（LSP 风格）划分 agentj 保留块。**协议错误**（信封 / 初始化 / 会话 / 取消）使用 JSON-RPC 错误码；**领域错误**（工具失败、模型错误）作为结果数据返回，不占用错误码（见 8.3 与 8.8）。

| 码 | 名称 | 来源 | 说明 |
|------|------|------|------|
| `-32700` | Parse error | JSON-RPC | JSON 解析失败；响应 `id` 为 `null` |
| `-32600` | Invalid Request | JSON-RPC | 非法请求结构（如缺少 `method`） |
| `-32601` | Method not found | JSON-RPC | 未知方法 |
| `-32602` | Invalid params | JSON-RPC | params 校验失败 |
| `-32603` | Internal error | JSON-RPC | 内部未预期错误 |
| `-32000 … -32099` | Server error（实现自定义） | JSON-RPC | 保留给实现自定义的服务器错误 |
| `-32800` | RequestCancelled | agentj | 请求被 `$/cancelRequest` 取消 |
| `-32801` | ServerNotInitialized | agentj | `initialize` 完成前收到业务方法请求 |
| `-32802` | SessionNotFound | agentj | 会话不存在（映射 `SessionError::NotFound`） |
| `-32803` | SessionInactive | agentj | 对 archived / deleted 会话执行操作（映射 `SessionError::InactiveSession`） |
| `-32804` | InvalidVersion | agentj | 协议版本不兼容（`initialize` 握手） |
| `-32805` | SessionBusy | agentj | 同 session 已有活动 run 时再次 `session/run`（`@since 0.3.0`） |
| `-32806` | NotAuthorized | agentj | 非 owner 连接的 `permission/respond`、`$/cancelRequest` 或 `question/respond`；`--token` 不匹配（`@since 0.3.0`） |
| `-32807` | PathNotAllowed | agentj | `file/list` 沙箱越界：请求路径（经词法规范化 + canonicalize）落在会话 `project_root` 之外（`@since 0.13.0`，见 8.20） |

协议扩展（`0.17.0`）**不新增任何错误码**：订阅面的 5 个方法（`model/list` / `auth/status` / `provider/usage` / `auth/deviceStart` / `auth/devicePoll`）复用既有码，并以 `error.data.reason` 区分两类拒绝：

- **未知 provider 名**（不在已注册 provider 中）→ `-32602 InvalidParams` + `error.data = { "field": "provider", "reason": "unknown_provider", "registered": ["<名>", …] }`。
- **provider 已知但不支持该能力**（如对非 opencode-go 调 `provider/usage` / `auth/deviceStart`）→ `-32602 InvalidParams` + `error.data = { "field": "provider", "reason": "unsupported_provider", "capability": "<usage|device_flow>" }`。
- **`provider_service` handle 未注入**（如 `run --attach` 连接远端内核）→ `-32603 Internal error` + `error.data = { "reason": "provider_service_unavailable" }`。

上游 / 凭据失败（未登录、401、403、5xx、网络）是**领域失败**，**不占错误码**：以**成功响应**的结果数据表达（`ProviderUsageResult.error.kind`、`AuthDevicePollResult.state`），使客户端能同时拿到「上次快照 + 失败原因」（`stale: true`，见 §8.25 / §8.27）。

协议扩展（`0.18.0`）同样**不新增错误码**：`auth/setApiKey` / `auth/clearCredential` 复用既有码——`-32602 InvalidParams` 的三种 `reason`（`invalid_params` / `unknown_provider` / `unsupported_provider`，其中不支持该能力的 `capability` 为 `"api_key"`）与 `-32603 Internal error` 的 `provider_service_unavailable`，语义与 0.17.0 逐字相同；`-32603` 另新增一个 `reason`：**凭据已落盘但运行期注册 / 注销失败** → `-32603` + `error.data = { "reason": "activation_failed", "detail": "<自产原因>" }`（凭据保留在盘上，重试有意义）。链接的三种校验结果（`verified` / `unverified` / `rejected`）与「无凭据可清除」同样是**领域失败即结果数据**，不占错误码（见 §8.28 / §8.29）。

错误码是**稳定的协议契约**，一旦发布不可更改含义。新增错误码 SHOULD 从 `-32800` 段向下或 `-32000 … -32099` 段内加性分配。

### 3.5 方法命名规范

- 方法名统一 `namespace/action` 小写：`session/new`、`kernel/status`。
- 协议级扩展用 `$/` 前缀：`$/cancelRequest`、`$/progress`。`$/` 前缀表示**可安全忽略的协议扩展**：接收方对不认识的 `$/` 通知 MUST 忽略而不失败。
- `rpc.` 前缀为 JSON-RPC 保留，MUST NOT 使用。

### 3.6 批量请求（batch）

JSON-RPC 2.0 支持批量请求（一个 JSON 数组包含多条消息），AgentJ 规范**保留**该语义，但 **v0.1 未启用**：方法面不承诺对 batch 的支持，服务端 MAY 以 `-32600 Invalid Request` 拒绝数组形式的消息。是否启用 batch 在 v0.2+ 再议。

## 4. 生命周期

连接建立后，客户端与服务端依次经过以下阶段：

```
initialize（请求）→ initialized（通知）→ 会话阶段 → shutdown（请求）→ exit（通知）
```

### 4.1 initialize（请求）

连接建立后，客户端 MUST 先发送 `initialize` 请求，且 MUST NOT 在该请求完成前发送任何其它业务方法请求。

```json
{ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
  "protocol_version": "0.1.0",
  "capabilities": { "streaming": true },
  "client_info": { "name": "agentj-cli", "version": "0.1.0" }
} }
```

**InitializeParams**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `protocol_version` | string | 是 | 客户端支持的协议版本，semver，见 4.3 |
| `capabilities` | object | 否 | 客户端能力对象，见 4.2 |
| `client_info` | ClientInfo | 否 | 客户端元信息 `{ name, version }` |
| `token` | string | 否 | 访问令牌（`@since 0.3.0`，对应守护内核 `--token`）；仅当服务端以 `--token` 启动时校验，不匹配 → `-32806 NotAuthorized` |

**InitializeResult**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `protocol_version` | string | 是 | 服务端选定的协议版本 |
| `capabilities` | object | 否 | 服务端能力对象，见 4.2 |
| `server_info` | ServerInfo | 是 | 服务端元信息 `{ name, version }` |
| `connection_id` | string | 否 | 本连接在多客户端内核下的唯一标识（`@since 0.3.0`）；stdio 单客户端模式缺省 |

### 4.2 能力交换

`capabilities` 对象用于能力探测：服务端在 `initialize` 结果中声明自己支持的能力，客户端可在 `initialize` 参数中声明自身能力。v0.1 暂无强制能力项；能力交换为加性设计，未知能力 MUST 忽略、绝不致命。

`@since 0.3.0` 新增服务端能力标志：

| 能力 | 类型 | 说明 |
|------|------|------|
| `multi_client` | boolean | 服务端支持多客户端共享同一内核实例（常驻内核，`agentj serve --port`）。为 true 时 `InitializeResult` 携带 `connection_id` |

### 4.3 协议版本协商

`protocol_version` 使用语义化版本（semver）：

| 段 | 变更含义 |
|----|----------|
| major | 破坏性变更（不兼容的字段 / 方法 / 语义变化） |
| minor | 加性变更（新方法、新可选字段） |
| patch | 修复 bug、澄清文档 |

- 双方交换 `protocol_version` 后，以**服务端返回值**为实际生效版本。
- 客户端与服务端版本 major 不一致，或客户端版本高于服务端能接受的范围时，服务端 MUST 以 `-32804 InvalidVersion` 拒绝 `initialize`。
- 收到 `-32804` 后，客户端 SHOULD 断开连接，MUST NOT 继续发送业务请求。
- 版本兼容的判定由实现按「服务端接受区间」裁定，但 MUST 遵守「major 不同即不兼容」的基本规则。

### 4.4 initialized（通知）

`initialize` 成功后，客户端 MUST 发送 `initialized` 通知，宣告就绪。`initialized` 无参数。服务端在收到 `initialized` 之前 MAY 已接受后续请求，但业务方法 SHOULD 在 `initialize` 完成后即可用。

### 4.5 shutdown（请求）

```json
{ "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": {} }
```

`shutdown` 无参数，结果固定为 `null`。收到 `shutdown` 后，服务端 MUST 停止接收新请求（新请求以 `-32600` 或 `-32801` 类错误拒绝），完成进行中的请求，然后返回 `null`，触发内核逆序优雅关闭链。

**多客户端语义（`@since 0.3.0`，常驻内核）**：在 `multi_client` 为 true 的内核上，`shutdown` 触发**全局**内核优雅关闭链（与最后一条连接断开、SIGINT/SIGTERM、stdio EOF 汇合到同一关闭链），而非仅断开请求方连接。stdio 单客户端模式语义不变（`shutdown` 即触发进程优雅关闭）。

### 4.6 exit（通知）

```json
{ "jsonrpc": "2.0", "method": "exit", "params": {} }
```

`exit` 无参数，MUST 在 `shutdown` 之后发送。收到 `exit` 后，服务端进程 MUST 退出。若客户端未发送 `shutdown` 直接发送 `exit`，服务端行为未定义（实现 SHOULD 按错误处理并仍退出）。

### 4.7 未初始化保护

在 `initialize` 完成前收到业务方法请求，服务端 MUST 以 `-32801 ServerNotInitialized` 拒绝。

## 5. 消息顺序与并发

- **请求处理**：服务端 MAY 并发处理多个请求（如多个 `session/run` 并行），MUST 保证同一请求只产生一个响应。
- **响应顺序**：服务端 SHOULD 大致按请求到达顺序返回响应，但不保证严格有序。**顺序无关的响应可以重排**（LSP 规则）：客户端 MUST 通过 `id` 关联请求与响应，MUST NOT 假设响应到达顺序与发送顺序一致。
- **通知顺序**：同一条通知流（如针对同一请求的 `$/progress` 序列）MUST 保持顺序；不同流之间无顺序保证。
- **取消**：`$/cancelRequest` 与对应的被取消请求响应之间无严格的到达顺序保证，客户端 MUST 同时容忍「先收到取消确认语义的最终响应」与「先收到带 `-32800` 的最终响应」两种顺序。

## 6. 取消

客户端可随时取消进行中的请求。

```json
{ "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": 3 } }
```

**CancelParams**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | Id | 是 | 要取消的请求 id |
| `session_id` | string | 否 | 目标会话 id（`@since 0.6.0` 加性字段）。缺省时：若恰好一个会话处于 in-flight 则取消该会话；若零个或多个会话同时 in-flight 则忽略本次取消（no-op，歧义安全） |

- `$/cancelRequest` 是通知，**双向可用**（注册表方向：both），接收方 MUST NOT 对其响应。
- **被取消的请求仍必须返回响应**（JSON-RPC「一请求一响应」不变量）：服务端取消成功时以 `-32800 RequestCancelled` 响应，`data` MAY 携带已产生的部分信息。
- 若取消到达时请求已完成，服务端 MUST 正常返回成功响应（忽略该取消）。
- 取消不保证中断底层工作：已进入工具执行的调用可能继续执行完毕，但其结果不会被计入已取消请求的最终响应。
- `session/run` 的取消语义见 8.5；取消后服务端 SHOULD 停止再发送该请求的 `$/progress` 通知。

## 7. 进度与流式

流式采用 `$/progress` 通知 + **token 解耦**设计（沿用 LSP）：`session/run` 是一个请求，最终返回**恰好一个**最终响应（保住 JSON-RPC「一请求一响应」不变量）；增量数据通过 `$/progress` 通知**带外**下发，`token` 与请求 `id` 解耦。

### 7.1 $/progress

```json
{ "jsonrpc": "2.0", "method": "$/progress", "params": {
  "token": 3,
  "value": { "kind": "text", "delta": "正在思考" }
} }
```

**ProgressParams**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `token` | ProgressToken | 是 | integer \| string，标识流式目标，见 7.2 |
| `value` | ProgressValue | 是 | 按 `kind` 判别的增量数据，见 7.3 |

### 7.2 progress_token 语义

`session/run` 的 `progress_token` 字段是**客户端可选**的（见 8.5）：

- **客户端提供**：服务端 MUST 在针对该请求的每一条 `$/progress` 中回显该 token 值。
- **客户端省略**：服务端使用该请求的 `id` 作为 token（`ProgressToken::Number` 或 `ProgressToken::Str`）。因为客户端本来就知道自己的请求 id，无需额外「宣告 token」的往返。此行为确定性可预期。

前端按 token 将 `$/progress` 通知与对应请求关联，无需预注册即可流式接收。

### 7.3 ProgressValue 判别

`value` 是一个按 `kind` 字段判别的联合类型，当前定义七个已知 `kind`（`text` / `tool_call` / `tool_result` 为 v0.1 既有，`usage` / `permission` `@since 0.12.0`，`todo` / `file_change` `@since 0.13.0`）：

| kind | 必填字段 | 说明 |
|------|----------|------|
| `text` | `delta` | 文本 token 增量（`delta` 为 string，可累积拼接为完整增量） |
| `tool_call` | `tool_call` | 模型请求调用工具；`tool_call` 为 `{ id, name, arguments }` |
| `tool_result` | `tool_call_id`, `content` | 工具执行结果回流；`tool_call_id` 关联对应 `tool_call` |
| `usage` | `usage` | 每轮 run 在 Done 时下发一次的 token 用量结算（`@since 0.12.0`）；`usage` 为 `{ prompt_tokens, completion_tokens }` |
| `permission` | `request_id`, `tool`, `decision` | 待审批 `$/permissionRequest` 的裁决结算（`@since 0.12.0`）；`decision` 为 `PermissionDecision`，可选字段 `session_id`（多会话客户端据此归类） |
| `todo` | `session_id`, `revision`, `items` | 全量 TODO 快照（`@since 0.13.0`，见 决策 #2）；`revision` 按会话单调递增，客户端丢弃比已见更旧的快照；`items` 为 `TodoItem[]`；可选 `run_id`（`@since 0.15.0`，按 run 单调） |
| `file_change` | `session_id`, `change` | 工作区文件变更（`@since 0.13.0`）；`change` 为 `FileChange`（`path` / `op` / `additions` / `deletions` + 可选 `tool_call_id` / `diff` / `diff_truncated` / `@since 0.15.0` 加性 `run_id` / `seq` / `source` / `total_bytes`）；外层可选 `run_id`（路由提示，与 `change.run_id` 一致时以 `change.run_id` 为准） |

七种已知值的完整结构见 `schemas/ProgressValue.json`（嵌套类型 `UsageInfo` / `TodoItem` / `FileChange` / `FileChangeSource` 亦各有独立 schema）。**未知 `kind` 可忽略**：依据 `$/` 前缀约定，消费方对不认识的 `kind` MUST 忽略该通知而不失败，服务端未来新增 `kind` 属加性变更（`@since` 注解）。

### 7.4 流式数据流

```
session/run 请求（携带 progress_token）→ AgentLoop.run_turn_stream()
    → $/progress {token, kind:text, delta}      （文本增量 ×N）
    → $/progress {token, kind:tool_call, tool_call}
    → $/progress {token, kind:tool_result, tool_call_id, content}
    → session/run 最终响应（完整 content + tool_calls）
```

### 7.5 权限审批流（`@since 0.2.0`）

模型请求调用工具时，若工具策略判定为 **Ask**（见 §8.15 与 `[permission]` 配置），内核暂停该工具执行，向客户端发送 `$/permissionRequest` 通知（S→C），等待客户端裁决；客户端通过 `permission/respond` 请求（C→S）返回决策。完整 wire 形状见 §8.13 与 §8.15，此处记录流程语义：

```
工具调用（Ask 判定）→ $/permissionRequest {request_id, tool, args, reason}
    → permission/respond {request_id, decision: allow | deny | allow_always}
    → 工具执行继续 / 被拒绝（Deny）
```

- **request_id 与 JSON-RPC 请求 id 解耦**：`request_id` 是不透明标识，沿用 `$/progress` 的 token 解耦模式（见 §7.2），**独立于**触发该审批的 `session/run` 请求 id。客户端凭 `request_id` 关联通知与响应，无需预注册。
- **60s 默认超时 → 自动 Deny**：内核等待审批默认 60s；超时未收到 `permission/respond`，该工具调用自动以 `Deny` 裁决（错误链 `RequestTimeout` → `Deny`），`session/run` 继续而非挂起。
- **allow_always 会话级作用域（内存态）**：`allow_always` 仅写入当前 session 的 `SessionAllowState`（会话级内存态），**会话结束即失效，永不写回 `[permission]` 配置**；跨会话白名单须显式配置 `[permission]` 规则（见 `agentj-permission` 设计）。
- **取消即拒绝**：`$/cancelRequest` 取消进行中的 `session/run` 时，该 session 所有**待审批**请求解析为 `Deny`（reason `cancelled`），不悬挂审批。
- **断连即拒绝（`@since 0.29.6`）**：owner 连接断开时，该 session 所有待审批请求解析为 `Deny`（reason `disconnected`），run 随之继续而非悬挂；审批请求发出时若该 session 已无 owner 连接可达，同样立即结算为 `Deny`，不等满超时（该请求在结算的同一时刻即被消费，等待方读到的是「请求已不存在」的拒绝结果，而非 `disconnected`）。
- **定向与归属**：`$/permissionRequest` 定向发送给发起该 run 的客户端连接（`session_owner`）；非 owner 连接调用 `permission/respond` → `-32806 NotAuthorized`（多客户端语义见 §4.5 与守护内核设计）。
- **内置工具默认策略**：`bash` / `write` = **Ask**；`read` / `glob` / `grep` = **Allow**；MCP / 插件工具 = **Ask**（未知工具保守 Ask）。均可经 `[permission]` 配置覆盖。

## 8. 方法面

本节为每个方法单独成节，采用 LSP 六段式：**能力声明 → 请求 → 响应 → 部分结果 → error.data**。wire 上所有字段一律 **snake_case**（见 §11 的命名归一化决策）。

### 8.1 方法总表

与 `protocol/methods.json` 一致（注册表收录全部 28 个方法）：

| 方法 | 类型 | 方向 | params | result | 说明 |
|------|------|------|--------|--------|------|
| `initialize` | request | C→S | InitializeParams | InitializeResult | 版本 + 能力协商 |
| `initialized` | notification | C→S | — | — | 客户端就绪 |
| `shutdown` | request | C→S | — | null | 断开连接 / 触发优雅关闭（见 §4.5） |
| `exit` | notification | C→S | — | — | shutdown 后进程退出 |
| `session/new` | request | C→S | SessionNewParams（可选 `project_root`，`@since 0.13.0`） | SessionNewResult | 新建会话 |
| `session/run` | request | C→S | SessionRunParams（可选 `provider` / `model`，`@since 0.16.0`） | SessionRunResult | 运行一轮（流式，见 §7） |
| `session/status` | request | C→S | SessionIdParams | SessionStatusResult | 查询会话状态 |
| `session/archive` | request | C→S | SessionIdParams | null | 软删除 |
| `session/delete` | request | C→S | SessionIdParams | null | 硬删除 |
| `session/subscribe` | request | C→S | SessionSubscribeParams | null | 订阅会话运行进度（`@since 0.3.0`，守护内核） |
| `session/unsubscribe` | request | C→S | SessionIdParams | null | 退订会话进度（`@since 0.3.0`，守护内核） |
| `session/list` | request | C→S | SessionListParams | SessionListResult | 列举会话（`@since 0.12.0`，见 8.18） |
| `session/history` | request | C→S | SessionHistoryParams | SessionHistoryResult | 会话轨迹分页回放（`@since 0.12.0`，见 8.19） |
| `permission/respond` | request | C→S | PermissionRespondParams | PermissionRespondResult | 响应审批（`@since 0.2.0`） |
| `kernel/status` | request | C→S | — | KernelStatusResult | 内核与模块状态 |
| `$/permissionRequest` | notification | S→C | PermissionRequestParams | — | 审批请求通知（`@since 0.2.0`） |
| `$/cancelRequest` | notification | both | CancelParams | — | 取消进行中的请求 |
| `$/progress` | notification | S→C | ProgressParams | — | 流式增量（见 §7） |
| `file/list` | request | C→S | FileListParams | FileListResult | 沙箱化目录列举（`@since 0.13.0`，见 8.20） |
| `$/askQuestion` | notification | S→C | AskQuestionParams | — | owner 定向模型提问（`@since 0.13.0`，见 8.21） |
| `question/respond` | request | C→S | QuestionRespondParams | QuestionRespondResult | 响应模型提问（`@since 0.13.0`，见 8.22） |
| `model/list` | request | C→S | ModelListParams | ModelListResult | 三层来源的模型目录（`@since 0.17.0`，见 8.23） |
| `auth/status` | request | C→S | AuthStatusParams | AuthStatusResult | 凭据状态纯读投影（`@since 0.17.0`，见 8.24） |
| `provider/usage` | request | C→S | ProviderUsageParams | ProviderUsageResult | 三窗口用量快照（`@since 0.17.0`，见 8.25） |
| `auth/deviceStart` | request | C→S | AuthDeviceStartParams | AuthDeviceStartResult | 设备码授权起点（`@since 0.17.0`，见 8.26） |
| `auth/devicePoll` | request | C→S | AuthDevicePollParams | AuthDevicePollResult | 设备码轮询状态（`@since 0.17.0`，见 8.27） |
| `auth/setApiKey` | request | C→S | AuthSetApiKeyParams | AuthSetApiKeyResult | 链接 API Key 并触发运行期激活（`@since 0.18.0`，见 8.28） |
| `auth/clearCredential` | request | C→S | AuthClearCredentialParams | AuthClearCredentialResult | 解除已存凭据并收敛注册（`@since 0.18.0`，见 8.29） |

### 8.2 initialize（请求，见 §4.1）

- **能力声明**：无前置能力；连接后第一个请求。
- **请求**：`InitializeParams`（`protocol_version` 必填，`capabilities` / `client_info` 可选）。
- **响应**：`InitializeResult`（`protocol_version`、`server_info` 必填，`capabilities` 可选）。
- **部分结果**：无。
- **error.data**：`-32804 InvalidVersion` 时 MAY 携带 `{ "supported": "0.1.0" }` 之类的服务端支持版本信息；`-32602` 时 MAY 携带字段级校验错误。

### 8.3 initialized（通知）

- **能力声明**：无。
- **请求（通知）**：无参数。
- **响应**：无（通知不响应）。
- **部分结果**：无。
- **error.data**：不适用。

### 8.4 shutdown（请求，见 §4.5）

- **能力声明**：无。
- **请求**：无参数。
- **响应**：`null`。
- **部分结果**：无。
- **error.data**：不适用（`-32603` 时 MAY 携带内部上下文）。

### 8.5 exit（通知，见 §4.6）

- **能力声明**：无。
- **请求（通知）**：无参数。
- **响应**：无（通知不响应）。
- **部分结果**：无。
- **error.data**：不适用。

### 8.6 session/new（请求）

- **能力声明**：会话能力 `SessionNewResult`；v0.1 无强制能力项。
- **请求**：`SessionNewParams`（可选；`@since 0.13.0`，旧客户端不传 params 行为不变）。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `project_root` | string | 否 | 会话 `project_root`：必须是存在的绝对目录（否则 `-32602`），canonicalize 后随会话持久化；缺省 = 进程 CWD。`file/list` 沙箱、bash git 采样、工具相对路径均以此根为基准（见 8.20） |

- **响应**：`SessionNewResult`。

```json
{ "jsonrpc": "2.0", "id": 2, "result": { "session_id": "s-1", "project_root": "/home/me/proj" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | 是 | 新会话的全局唯一标识 |
| `project_root` | string | 否 | 会话**有效** `project_root`：请求给定值经 canonicalize 后的结果，请求缺省时为内核进程 CWD（`@since 0.14.0` 加性字段；与 `file/list` 沙箱根同源，见 8.20） |

- **部分结果**：无。
- **error.data**：`project_root` 不是存在的绝对目录 → `-32602`，MAY 携带 `{ "project_root": "<原值>" }`；`-32603` 时 MAY 携带内部上下文。

### 8.7 session/run（请求，流式）

- **能力声明**：流式能力（`$/progress`）；v0.1 无强制能力项。
- **请求**：`SessionRunParams`。

```json
{ "jsonrpc": "2.0", "id": 3, "method": "session/run", "params": {
  "session_id": "s-1",
  "content": "帮我总结这段代码",
  "attachments": [ { "path": "/tmp/a.txt", "mime": "text/plain", "content": "..." } ],
  "progress_token": "tok-3"
} }
```

运行期指定模型（`@since 0.16.0`，`provider` / `model` 均可选）：

```json
{ "jsonrpc": "2.0", "id": 4, "method": "session/run", "params": {
  "session_id": "s-1",
  "content": "换模型再跑一遍",
  "provider": "opencode-go",
  "model": "opencode-go/kimi-k3"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | 是 | 目标会话 id |
| `content` | string | 是 | 本轮用户输入 |
| `attachments` | Attachment[] | 否 | 附件列表（`path` / `mime` / `content` 均可选，至少提供一项） |
| `progress_token` | ProgressToken | 否 | 流式 token；省略时服务端用请求 id 作 token（见 §7.2） |
| `provider` | string | 否 | 运行期 provider 名（`@since 0.16.0` 加性字段）：必须是**已注册** provider（= `kernel/status.providers[].id`）；省略 / `null` / trim 后空串 = 缺省（内核默认 provider）；非字符串类型 → `-32602` + `data{field:"provider"}`。**未知但格式合法的名字不回落默认**：本轮运行以 `agent error: unknown provider: <name> (registered: <列表>)` 失败 |
| `model` | string | 否 | 运行期模型 id（`@since 0.16.0` 加性字段）：如 `opencode-go/kimi-k3`；省略 / `null` / trim 后空串 = 缺省（`config.model.default_model`）；非字符串类型 → `-32602` + `data{field:"model"}`；不属于所选 provider 的模型沿用既有 `UnknownModel` 路径失败 |

- **运行期覆盖（`@since 0.16.0`，加性）**：覆盖**只作用于显式给出的维度**——只给 `provider` 时模型仍取 `config.model.default_model`，只给 `model` 时 provider 仍取内核默认；两者都不给 / `null` / 空串或纯空白 = 与今天行为逐字相同（`get_default()` + 配置默认模型）。`provider` / `model` 均为**按 run** 生效，不写会话快照、不影响历史消息与统计。

- **响应**：`SessionRunResult`（**恰好一个**最终响应）。

```json
{ "jsonrpc": "2.0", "id": 3, "result": {
  "content": "这段代码的主要职责是……",
  "tool_calls": [ { "id": "tc-1", "name": "read_file", "arguments": { "path": "src/main.rs" } } ],
  "run_id": "550e8400-e29b-41d4-a716-446655440000"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `content` | string | 是 | 本轮完整结果文本 |
| `tool_calls` | ToolCall[] | 否 | 本轮模型请求的工具调用列表；`ToolCall = { id, name, arguments }` |
| `run_id` | string | 否 | 本轮 `run_id`（UUID v4，`@since 0.15.0`）；用于关联同 run 的 `todo` / `file_change` 脉冲与回放过滤 |

- **部分结果**：增量数据通过 `$/progress` 通知带外下发（见 §7），最终仍返回一个完整响应。
- **error.data**：会话不存在 → `-32802`，MAY 携带 `{ "session_id": "s-1" }`；会话已 archived / deleted → `-32803`，MAY 携带 `{ "session_id": "s-1", "status": "archived" }`；被取消 → `-32800 RequestCancelled`，MAY 携带已产生的部分结果；`provider` / `model` 类型非法 → `-32602`，MAY 携带 `{ "field": "<provider|model>" }`（`@since 0.16.0`）。
- **领域错误**（工具失败、模型错误）**不占错误码**，作为结果数据进入 `$/progress` 与最终响应的 payload，前端在 payload 内感知，协议层不因领域失败而报错。

### 8.8 session/status（请求）

- **能力声明**：无。
- **请求**：`SessionIdParams`（`session_id` 必填）。
- **响应**：`SessionStatusResult`。

```json
{ "jsonrpc": "2.0", "id": 4, "result": { "status": "active", "project_root": "/home/me/proj" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `status` | SessionStatus | 是 | `active` \| `archived` \| `deleted`（未知值见 §9） |
| `project_root` | string | 否 | 会话**有效** `project_root`（canonicalized；与 `file/list` 沙箱根同源，`@since 0.14.0` 加性字段） |

- **部分结果**：无。
- **error.data**：`-32802 SessionNotFound`，MAY 携带 `{ "session_id": "s-1" }`。

### 8.9 session/archive（请求）

- **能力声明**：无。
- **请求**：`SessionIdParams`（`session_id` 必填）。
- **响应**：`null`。
- **部分结果**：无。
- **error.data**：`-32802 SessionNotFound`；对已 archived / deleted 会话重复归档 → `-32803 SessionInactive`。

### 8.10 session/delete（请求）

- **能力声明**：无。
- **请求**：`SessionIdParams`（`session_id` 必填）。
- **响应**：`null`。
- **部分结果**：无。
- **error.data**：`-32802 SessionNotFound`；对已删除会话重复删除 → `-32803 SessionInactive`。

### 8.11 session/subscribe（请求，`@since 0.3.0`，守护内核）

- **能力声明**：多客户端能力 `multi_client`（见 §4.2）。
- **请求**：`SessionSubscribeParams`。

```json
{ "jsonrpc": "2.0", "id": 6, "method": "session/subscribe", "params": {
  "session_id": "s-1",
  "progress_token": "tok-dash"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | 是 | 目标会话 id |
| `progress_token` | ProgressToken | 否 | 订阅流 token；省略时服务端用请求 id 作 token（见 §7.2） |

- **响应**：`null`（订阅成功）。
- **语义**：订阅后，该 session 的 `$/progress` 通知向**所有订阅者**广播（含发起方，其同时是订阅者或经 `session/run` 的 `progress_token`）。订阅是**只读观测**：订阅者不能控制该 run（`$/cancelRequest`、`permission/respond` 仅 owner 可用，见 8.13）。发起 `session/run` 的连接会被服务端**隐式订阅**（无需显式 `session/subscribe` 即可收该 session 的 `$/progress`）。
- **部分结果**：无。
- **error.data**：`-32802 SessionNotFound`；`-32803 SessionInactive`（archived / deleted 会话不可订阅）。

### 8.12 session/unsubscribe（请求，`@since 0.3.0`，守护内核）

- **能力声明**：多客户端能力 `multi_client`（见 §4.2）。
- **请求**：`SessionIdParams`（`session_id` 必填）。
- **响应**：`null`。
- **语义**：退订指定 session 的进度广播。对未订阅的 session 退订为 no-op。**连接断开时隐式退订**（无需显式 unsubscribe，避免残留订阅）。
- **部分结果**：无。
- **error.data**：不适用。

### 8.13 permission/respond（请求，`@since 0.2.0`）

- **能力声明**：权限审批能力（`$/permissionRequest` 通知对端声明）。
- **请求**：`PermissionRespondParams`。

```json
{ "jsonrpc": "2.0", "id": 7, "method": "permission/respond", "params": {
  "request_id": "req-9",
  "decision": "allow"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `request_id` | string | 是 | 对应 `$/permissionRequest` 的通知 id（独立于 JSON-RPC 请求 id） |
| `decision` | PermissionDecision | 是 | `allow` \| `deny` \| `allow_always`（未知值见 §9） |

- **响应**：`PermissionRespondResult`（空对象 `{}`）。
- **归属校验**：服务端按 `session_owner` 校验发起方——**只有发起该 run 的客户端连接**可以响应对应审批；非 owner 连接响应 → `-32806 NotAuthorized`。
- **allow_always 语义**：将（工具, 命中规则指纹）写入当前 session 的 `SessionAllowState`（会话级内存态，会话结束即失效；跨会话白名单需显式配置 `[permission]` 规则）。
- **部分结果**：无。
- **error.data**：`-32806 NotAuthorized`（非 owner 响应）；审批超时（默认 60s）自动 `Deny`，`session/run` 返回拒绝语义。

### 8.14 kernel/status（请求）

- **能力声明**：无。
- **请求**：无参数。
- **响应**：`KernelStatusResult`。

```json
{ "jsonrpc": "2.0", "id": 5, "result": {
  "version": "0.1.0",
  "modules": [ { "id": "toolchain", "state": "ready" } ]
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `version` | string | 是 | 内核版本 |
| `modules` | ModuleInfo[] | 是 | 模块状态列表；`ModuleInfo = { id, state }` |

- **部分结果**：无。
- **error.data**：`-32603` 时 MAY 携带内部上下文。

### 8.15 $/permissionRequest（通知，`@since 0.2.0`）

- **能力声明**：权限审批能力（服务端声明于 `initialize` 的 capabilities）。
- **请求（通知）**：`PermissionRequestParams`。

```json
{ "jsonrpc": "2.0", "method": "$/permissionRequest", "params": {
  "request_id": "req-9",
  "tool": "bash",
  "args": { "cmd": "rm -rf /tmp/x" },
  "reason": "tool default ask"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `request_id` | string | 是 | 审批请求 id（uuid，独立于 JSON-RPC 请求 id，见 §7.5） |
| `tool` | string | 是 | 需要审批的工具名 |
| `args` | object | 是 | 工具原始参数 |
| `reason` | string | 否 | 审批原因（如命中的规则 / 默认模式） |
| `session_id` | string | 否 | 归属会话 id（`@since 0.6.0` 加性字段，客户端可按此字段做多会话 UI 归类） |

- **定向路由**：服务端按 `session_owner` **定向**发送给发起该 run 的客户端连接（设计决策 #5），其余客户端不收到该通知。`origin_client` 字段显式标识发起连接（`@since 0.3.0`）；stdio 单客户端模式下该字段缺省。
- **响应**：无（通知不响应）；发起方通过 `permission/respond`（见 8.13）响应。
- **部分结果**：不适用。
- **error.data**：不适用。

### 8.16 $/cancelRequest（通知，见 §6）

- **能力声明**：无。
- **请求（通知）**：`CancelParams`（`id` 必填，`session_id` 可选 `@since 0.6.0`；缺省时若恰好一个会话 in-flight 则取消该会话，零个或多个 in-flight 则忽略 no-op，见 §6）。
- **响应**：无（通知不响应）；被取消请求本身仍会以 `-32800` 响应。
- **部分结果**：无。
- **error.data**：不适用。

### 8.17 $/progress（通知，见 §7）

- **能力声明**：流式能力（服务端声明于 `initialize` 的 capabilities）。
- **请求（通知）**：`ProgressParams`（`token`、`value` 必填）。
- **响应**：无（通知不响应）。
- **部分结果**：不适用（通知本身就是部分结果载体）。
- **error.data**：不适用；未知 `kind` 忽略（见 §7.3）。

### 8.18 session/list（请求，`@since 0.12.0`）

- **能力声明**：无（只读列举，不经 60s 审批通道）。
- **请求**：`SessionListParams`（空对象 `{}`，无必填字段）。
- **响应**：`SessionListResult`。

```json
{ "jsonrpc": "2.0", "id": 8, "result": { "sessions": [
  { "session_id": "s-1", "updated_at": 1700000000, "state": "active", "preview": "帮我总结这段代码" }
] } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `sessions` | SessionSummary[] | 是 | 会话摘要列表；`SessionSummary = { session_id, updated_at, state, preview? }` |

- **SessionSummary**：`session_id`（string）、`updated_at`（integer，epoch **秒**）、`state`（SessionStatus，见 §9）、`preview`（string，可选，末条消息摘要，内核截断至 120 字符）。
- **部分结果**：无。

### 8.19 session/history（请求，`@since 0.12.0`）

- **能力声明**：无（只读轨迹回放）。
- **请求**：`SessionHistoryParams`。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | 是 | 目标会话 id |
| `cursor` | integer | 否 | 分页游标（起始 `seq`，含）；缺省从头开始 |
| `limit` | integer | 否 | 单页事件数，服务端 clamp 至 `1..=500`，缺省 100 |
| `run_id` | string | 否 | 按 run 过滤回放（`@since 0.15.0`；UUID v4，>128 字符或非法 → `-32602` + `data{reason:"invalid_run_id"}`；缺省全量） |

- **响应**：`SessionHistoryResult`。

```json
{ "jsonrpc": "2.0", "id": 9, "result": {
  "events": [
    { "session_id": "s-1", "seq": 0, "ts": 1700000000000, "value": { "kind": "user", "content": "hi" } }
  ],
  "next_cursor": 3
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `events` | TrajectoryEvent[] | 是 | 本页轨迹事件；`TrajectoryEvent = { session_id, seq, ts, value }` |
| `next_cursor` | integer | 否 | 下一页游标（= 本页最后 `seq` + 1）；无更多页时省略 |

- **run_id 过滤语义（`@since 0.15.0`）**：当 `run_id` 存在时，仅返回该 `run_id` 的 `Todo` / `FileChange` 事件（其余 `TrajectoryValue` kind 保持全量以不破坏时间线）；先过滤再按 `cursor` / `limit` 分页；`run_id` 无匹配时 `events:[]`（不报错）；缺省 `run_id` 时全量分页与旧语义一致。
- **TrajectoryEvent**：`seq`（integer，会话内单调递增，页游标基准）、`ts`（integer，epoch **毫秒**）、`value: TrajectoryValue`（按 `value.kind` 判别的联合类型，见下）。轨迹是**追加式**事件日志，存储文件与 `session/history` payload 逐字节同构。
- **TrajectoryValue kind**：`user`、`thinking`、`tool_call`、`tool_result`、`permission`、`usage`、`error`（`session/history` 引入时既有）；`todo`、`file_change`、`ask_question`、`question_answer`（`@since 0.13.0`）。未知 `kind` 按 §7.3 的 `$` 前缀约定忽略。
- **未知 session**：返回空 `events`（`next_cursor` 省略），**不报错**、不泄露会话信息。
- **部分结果**：无（分页以 `next_cursor` 表达，非流式）。
- **error.data**：`-32603` 时 MAY 携带内部上下文（如轨迹存储读取失败）；`run_id` 非法 → `-32602` + `data{reason:"invalid_run_id"}`。

### 8.20 file/list（请求，`@since 0.13.0`）

- **能力声明**：无（只读列举，沙箱即安全边界）。
- **请求**：`FileListParams`。

```json
{ "jsonrpc": "2.0", "id": 10, "method": "file/list", "params": {
  "session_id": "s-1",
  "path": "src",
  "depth": 1,
  "query": "main"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | 是 | 解析会话 `project_root` 与归属 |
| `path` | string | 否 | 相对项目根的目录路径；缺省 / `""` = 根；query 模式下为搜索子树根 |
| `depth` | integer | 否 | 列举深度 `1..=8`，缺省 1；越界 → `-32602`；query 模式下**忽略** |
| `query` | string | 否 | 搜索子串（`@since 0.14.0` 加性字段）；trim 后空串视作缺省；大小写不敏感，匹配条目名称或相对路径 |

- **响应**：`FileListResult`。

```json
{ "jsonrpc": "2.0", "id": 10, "result": {
  "entries": [
    { "name": "main.rs", "path": "src/main.rs", "kind": "file", "size": 42, "mtime_ms": 1700000000000, "git_status": "M" }
  ],
  "truncated": false,
  "total": 1
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `entries` | FileEntry[] | 是 | 目录条目；缺省模式上限 2000，query 模式返回 Top-100，超出截断 |
| `truncated` | boolean | 是 | 是否被截断（缺省模式：2000 条目上限；query 模式：`SEARCH_VISIT_LIMIT` 访问预算耗尽） |
| `total` | integer | 否 | query 模式发现的匹配总数（`@since 0.14.0` 加性字段，`u64`）；受访问预算限制时为**下界**；缺省模式省略 |

- **query 模式（`@since 0.14.0`，加性）**：在 `path` 子树内搜索（缺省 = 项目根）；**命中仅限文件**（目录参与遍历/访问预算，但不作为命中返回）；`.git` 排除、根外符号链接条目跳过、沙箱规则与错误码均不变；**`depth` 被忽略**。
  - **访问预算**：`SEARCH_VISIT_LIMIT = 20_000` 个访问条目；耗尽 → `truncated: true`（结果可能不完整，`total` 为下界）。
  - **排序（rank）**：名称前缀（0）> 名称子串（1）> 路径子串（2）；大小写不敏感；同 rank tie-break 路径升序。
  - **返回**：Top `SEARCH_RESULT_LIMIT = 100`；`total > entries.len()` 时客户端展示 `N more`。
  - **兼容**：旧内核忽略未知 `query` 字段 → 退化为缺省 listing（响应无 `total`），新客户端据此自愈降级为客户端过滤，无需握手探测。

- **FileEntry**：`name`（不含分隔符）、`path`（相对项目根，无 `./` 前缀）、`kind`（`file` \| `dir`，见 §9）、`size`（integer，可选；目录为省略）、`mtime_ms`（integer，可选，epoch 毫秒）、`git_status`（`M` \| `A` \| `U` \| `D`，可选；目录 / clean / 非 git 仓库省略）。
- **排序与排除**：目录在前、同类按名称升序；`.git` 目录本身不返回、不下钻；其余隐藏文件正常返回。
- **git 映射**（porcelain `XY`）：`??` / `A ` → `A`；` D` / `D ` → `D`；冲突（`UU/AA/DD/AU/UA/DU/UD`）→ `U`；其余非 clean 状态（含 `M` 等）→ `M` 兜底；clean → 省略；非 git 仓库或 git 不可用 → 全省略（降级，不报错）。
- **沙箱**：root = 会话 `project_root`（canonicalize；缺省 = 进程 CWD）；请求 `path` 先词法规范化再 canonicalize，结果必须 `starts_with(root)`，否则 `-32807 PathNotAllowed`；目录内**指向根外的符号链接条目跳过**（条目不返回，不报错）。
- **部分结果**：无（分页由 `depth` + `truncated` 表达，非流式）。
- **error.data**：会话不存在 → `-32802`；`path` 不存在 → `-32602` + `{ "path": "<原值>", "reason": "not_found" }`；`path` 指向文件而非目录 → `-32602` + `{ "path": "<原值>", "reason": "not_a_directory" }`；`depth` 越界 → `-32602` + `{ "reason": "invalid_depth" }`；沙箱越界 → `-32807` + `{ "path": "<原值>", "reason": "outside_project_root" }`；目标目录不可读 → `-32603` + `{ "path": "<原值>", "reason": "<io 错误>" }`。

### 8.21 $/askQuestion（通知，`@since 0.13.0`，owner 定向）

- **能力声明**：提问能力（对端以 `question/respond` 请求声明）。
- **请求（通知）**：`AskQuestionParams`。

```json
{ "jsonrpc": "2.0", "method": "$/askQuestion", "params": {
  "question_id": "q-1",
  "session_id": "s-1",
  "question": "选择部署目标？",
  "header": "部署",
  "options": [ { "id": "staging", "label": "Staging", "hint": "预发" }, { "id": "prod", "label": "Production" } ],
  "allow_multiple": false,
  "allow_other": true,
  "timeout_s": 60
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `question_id` | string | 是 | 提问标识（独立于 JSON-RPC 请求 id，见 §7.5 的解耦模式） |
| `session_id` | string | 是 | 归属会话 id |
| `question` | string | 是 | 提问正文 |
| `header` | string | 否 | 提问卡片短标题 |
| `options` | AskOption[] | 是 | 选项列表（2–8 项）；`AskOption = { id, label, hint? }` |
| `allow_multiple` | boolean | 否 | 是否允许多选；缺省 `false`（wire 上总是序列化） |
| `allow_other` | boolean | 否 | 是否允许 `other:` 自由文本；缺省 `true`（wire 上总是序列化） |
| `timeout_s` | integer | 否 | 超时秒数；缺省省略 = 不超时（超时按 skip 结算） |

- **定向路由**：服务端按 `session_owner` **定向**发送给发起该 run 的客户端连接；其余客户端不收到（非 owner 不可见，见 决策 #5 的 `$/permissionRequest` 同款语义）。
- **无 owner 兜底**：归属连接缺席时该提问按 `skipped` 结算，不悬挂。
- **响应**：无（通知不响应）；发起方通过 `question/respond`（见 8.22）响应。
- **部分结果**：不适用。
- **error.data**：不适用。

### 8.22 question/respond（请求，`@since 0.13.0`）

- **能力声明**：提问能力（服务端以 `$/askQuestion` 通知对端声明）。
- **请求**：`QuestionRespondParams`。

```json
{ "jsonrpc": "2.0", "id": 11, "method": "question/respond", "params": {
  "question_id": "q-1",
  "answers": ["staging"],
  "skipped": false
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `question_id` | string | 是 | 对应 `$/askQuestion` 的通知标识 |
| `answers` | string[] | 是 | 作答：`AskOption.id` 或 `other:` 前缀自由文本 |
| `skipped` | boolean | 否 | 是否跳过；缺省 `false` |

- **响应**：`QuestionRespondResult`（`{ "accepted": boolean }`）。
- **归属与校验顺序**：先解析 `question_id`——未知（从未存在 / 已过期清理）→ 直接 `{ "accepted": false }`，不做归属校验、不泄露会话信息；可解析时先做 owner 校验（非 owner → `-32806`），再做参数校验（畸形 / 未知 option id / 单选多答 → `-32602`），最后做结算尝试。
- **幂等**：同一 `question_id` 仅首次有效响应接受（`accepted: true`）；重复 / 已结算 / 未知 → `{ "accepted": false }`，**不报错**（客户端重试安全）。
- **结算**：respond / 超时 / run 取消 / 会话归档删除 / owner 连接断开 → 统一结算一次（`answers` 或 `skipped`），settle hook 保证轨迹只记录一次（`question_answer`）。
- **部分结果**：无。
- **error.data**：非 owner → `-32806`；未知 option id → `-32602` + `{ "unknown_options": ["<id>", …] }`；单选传多答 → `-32602` + `{ "reason": "too_many_answers" }`。

### 8.23 model/list（请求，`@since 0.17.0`）

- **能力声明**：订阅能力（provider 目录）。需内核在装配时注入 `provider_service` handle；未注入 → `-32603` + `data{reason:"provider_service_unavailable"}`（见 §3.4）。
- **请求**：`ModelListParams`（两个字段均可选）。

```json
{ "jsonrpc": "2.0", "id": 12, "method": "model/list", "params": {
  "provider": "opencode-go",
  "refresh": true
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名（= `kernel/status.providers[].id`）；省略 / `null` = 内核配置的默认 provider |
| `refresh` | boolean | 否 | `true` 绕过 **60s** 进程内复用窗口强制重拉；缺省 / `null` = 窗口内直接返回上次结果（防面板反复开关打上游） |

- **响应**：`ModelListResult`。

```json
{ "jsonrpc": "2.0", "id": 12, "result": {
  "provider": "opencode-go",
  "default_model": "opencode-go/kimi-k3",
  "source": "account",
  "fetched_at": 1700000000,
  "models": [ { "id": "opencode-go/kimi-k3", "verified": true, "format": "anthropic" } ]
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际查询的 provider 名 |
| `default_model` | string | 是 | 内核配置的默认模型 id（`config.model.default_model`；未配置时为 provider 名） |
| `source` | ModelSource | 是 | 本次目录的来源层级，见 §9.1 |
| `fetched_at` | integer | 是 | 本目录的获取时刻（epoch **秒**，int64） |
| `models` | ModelEntry[] | 是 | 目录条目（可为空数组）；`ModelEntry = { id, verified, format }` |

- **目录三层来源（`account` → `live` → `overlay`；非 opencode-go 为 `static`）**：有可用凭据 → 账号级 `GET {console}/api/v2/config`（`Bearer <key|access>`，OAuth 凭据额外带 `x-org-id`），成功即 `source:"account"`；失败或凭据不可用 → live 目录 ∩ overlay → `source:"live"`；live 也失败 → overlay 全集 → `source:"overlay"`；`kind` 非 `opencode-go` 的 provider **不发网络请求**，直接投影 `ModelProvider::models()` → `source:"static"`。逐层回退**全部静默**（`model/list` 仍成功），账号级目录不可读（401 / 403 / 畸形响应）不向客户端报错。
- **路由标记**：账号级 / live 的 id 命中 overlay → 沿用其 `format` 与 `verified`；未命中 → 合成为 `openai-compat` + `verified:false`（**绝不按名字猜路由**）。`format` 取值 `openai-compat` \| `responses` \| `anthropic`。
- **部分结果**：无（一次性快照）。
- **error.data**：未知 provider → `-32602` + `{ "field": "provider", "reason": "unknown_provider", "registered": ["<名>", …] }`；params 结构畸形（如 `provider` 为数字 / 对象）→ `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.24 auth/status（请求，`@since 0.17.0`）

- **能力声明**：订阅能力（凭据状态）。需注入 `provider_service` handle；未注入 → `-32603` + `data{reason:"provider_service_unavailable"}`。
- **请求**：`AuthStatusParams`。

```json
{ "jsonrpc": "2.0", "id": 13, "method": "auth/status", "params": { "provider": "opencode-go" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider |

- **响应**：`AuthStatusResult`。

```json
{ "jsonrpc": "2.0", "id": 13, "result": {
  "provider": "opencode-go",
  "authenticated": true,
  "credential": { "kind": "api_key", "source": "file", "key_hint": "…1234", "expiring_soon": false },
  "models_count": 44
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际查询的 provider 名 |
| `authenticated` | boolean | 是 | 解析后是否存在**可用**凭据（`false` 时省略 `credential`） |
| `credential` | CredentialInfo | 否 | 已脱敏的凭据投影，见下 |
| `models_count` | integer | 是 | 该 provider 已注册目录的模型数（`kernel/status.providers[].models` 的长度） |

- **CredentialInfo（脱敏投影，永不携带完整密钥）**：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `kind` | CredentialKind | 是 | `api_key` \| `oauth`（见 §9.1） |
| `source` | CredentialSource | 是 | `flag` \| `env` \| `file`（本期内核实现只产生 `env` / `file`；`flag` 为加性预留） |
| `key_hint` | string | 否 | 仅 `api_key`：`redact()` 输出，形如 `…1234`（末 4 位）；密钥长度 ≤ 4 时为 `…` |
| `expires_at` | integer | 否 | 仅 OAuth：access token 到期时刻（epoch **秒**，int64） |
| `expiring_soon` | boolean | 是 | 仅 OAuth 有意义：剩余寿命 < **120s** 时为 `true`（api_key 恒 `false`） |
| `org_id` | string | 否 | 仅 OAuth：上游返回的组织 id |

- **纯读语义**：`auth/status` **绝不轮换 token、绝不发网络请求**——只报告本地凭据事实（`api_key` 走 `redact()`，OAuth 走 `org_id` / `expires_at`）。需要触发刷新的是真实请求路径（内核在每轮 run 前 `ensure_access`）。
- **部分结果**：无。
- **error.data**：未知 provider → `-32602` + `{ "field": "provider", "reason": "unknown_provider", "registered": […] }`；params 结构畸形 → `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；凭据文件不可读 / 权限不合规 → `-32603` + `{ "reason": "<原文>" }`；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.25 provider/usage（请求，`@since 0.17.0`）

- **能力声明**：订阅能力（用量查询），**仅 opencode-go** 支持。需注入 `provider_service` handle。
- **请求**：`ProviderUsageParams`（两个字段均可选）。

```json
{ "jsonrpc": "2.0", "id": 14, "method": "provider/usage", "params": {
  "provider": "opencode-go",
  "refresh": false
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider；非 opencode-go → `-32602` + `reason:"unsupported_provider"` |
| `refresh` | boolean | 否 | `true` 绕过 **300s** 进程内缓存强制重拉；缺省 / `null` = 缓存内直接返回上次快照 |

- **响应**：`ProviderUsageResult`（**成功响应**，甚至失败时也是——见下）。

```json
{ "jsonrpc": "2.0", "id": 14, "result": {
  "provider": "opencode-go",
  "fetched_at": 1700000000,
  "stale": false,
  "warn_percent": 80,
  "windows": [
    { "kind": "rolling", "status": "ok", "percent": 4, "resets_at": "2026-08-13T16:27:38Z" },
    { "kind": "weekly", "status": "ok", "percent": 3, "resets_at": "2026-08-17T00:00:00Z" },
    { "kind": "monthly", "status": "rate-limited", "percent": 100 }
  ],
  "error": null
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际查询的 provider 名 |
| `fetched_at` | integer | 是 | 快照获取时刻（epoch **秒**，int64） |
| `stale` | boolean | 是 | `true` = `windows` 是更早一次成功快照的复用（失败路径）；失败且无缓存时 `windows` 为空数组、仍为 `false` |
| `warn_percent` | integer | 是 | 客户端警示阈值（`[model].quota_warn_percent`，默认 **80**，clamp **1..=100**）；客户端只做展示，不各自维护配置 |
| `windows` | UsageWindow[] | 是 | 已知窗口（`rolling` 5h / `weekly` / `monthly`）的用量；无数据 / 失败无缓存时为空数组（**不编造百分比**） |
| `error` | ProviderUsageError | 否 | **领域失败**通道（成功响应内），见下；成功时为 `null` / 省略 |

- **UsageWindow**：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `kind` | UsageWindowKind | 是 | `rolling`（滚动 5 小时）\| `weekly` \| `monthly`（见 §9.1） |
| `status` | UsageWindowStatus | 是 | `ok` \| `rate-limited` |
| `percent` | integer | 否 | **已用**百分比 `0..=255`；上游缺字段或非整数时省略（该窗口降级为「未知」） |
| `resets_at` | string | 否 | 该窗口重置时刻（上游原文，ISO 8601）；`percent == 0` 时**省略**（上游占位值会误导用户） |

- **无余额字段**：结果**只提供已用百分比**，不存在余额 / 剩余额度 / 货币字段；「free 额度」不是本协议的一部分。
- **ProviderUsageError（领域失败即结果数据）**：`kind ∈ { no_credential, expired, unauthorized, entitlement, unavailable, network }`（**普通字符串**，未知取值客户端 MUST 容忍）+ `message`（中文可读文案，如「尚未登录：请运行 agentj auth login --device」「密钥无效或已被撤销：请重新登录」「该工作区没有 OpenCode Go 订阅」「用量暂时不可用（上游响应无法识别）」「网络不可达：<原因>」）。有旧缓存 → 返回旧窗口 + `stale:true` + `error`；无缓存 → 空窗口 + `error`。客户端按 `kind` 映射文案，不解析 `message`。
- **防御式解析**：只认 `usage` 对象下的 `rolling` / `weekly` / `monthly` 三个已知键（未知键忽略）；`status` 未知值归 `Unknown`（不失败）；`percent` 非整数 → 该窗口降级且其余窗口保留；`usage` 整体缺失 → `kind:"unavailable"`，**不编造窗口**。
- **部分结果**：无。
- **error.data**：未知 provider → `-32602` + `{ "field": "provider", "reason": "unknown_provider", "registered": […] }`；非 opencode-go → `-32602` + `{ "field": "provider", "reason": "unsupported_provider", "capability": "usage" }`；params 结构畸形 → `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.26 auth/deviceStart（请求，`@since 0.17.0`）

- **能力声明**：订阅能力（RFC 8628 设备授权），**仅 opencode-go** 支持。需注入 `provider_service` handle。
- **请求**：`AuthDeviceStartParams`。

```json
{ "jsonrpc": "2.0", "id": 15, "method": "auth/deviceStart", "params": { "provider": "opencode-go" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider；非 opencode-go → `-32602` + `reason:"unsupported_provider"` |

- **响应**：`AuthDeviceStartResult`（上游设备授权响应原样透传，不落盘任何凭据）。

```json
{ "jsonrpc": "2.0", "id": 15, "result": {
  "provider": "opencode-go",
  "device_code": "dc-…",
  "user_code": "ABCD-1234",
  "verification_uri": "https://opencode.ai/console/device?user_code=ABCD-1234",
  "expires_in": 600,
  "interval": 5
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际查询的 provider 名 |
| `device_code` | string | 是 | 设备码（**不透明**，客户端在后续 `auth/devicePoll` 原样回传；内核无状态，不保存它） |
| `user_code` | string | 是 | 用户在浏览器中输入的短码 |
| `verification_uri` | string | 是 | 用户应在浏览器打开的地址（`{console}/device?…`） |
| `expires_in` | integer | 是 | 设备码有效秒数（上游当前为 600） |
| `interval` | integer | 是 | 建议轮询间隔秒数（上游当前为 5） |

- **无状态语义**：`device_code` 由客户端持有并回传；内核不保存设备流会话，`auth/deviceStart` 不写 `auth.json`。授权成功与否只由 `auth/devicePoll` 判定（见 8.27）。
- **部分结果**：无。
- **error.data**：未知 provider / 非 opencode-go → `-32602`（同上两种 `reason`）；params 结构畸形 → `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；上游不可达 / 非 200 / 响应畸形 → `-32603` + `{ "reason": "<上游或网络错误原文>" }`（**不落盘任何凭据**）；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.27 auth/devicePoll（请求，`@since 0.17.0`）

- **能力声明**：订阅能力（RFC 8628 设备授权轮询），**仅 opencode-go** 支持。需注入 `provider_service` handle。
- **请求**：`AuthDevicePollParams`。

```json
{ "jsonrpc": "2.0", "id": 16, "method": "auth/devicePoll", "params": {
  "device_code": "dc-…",
  "provider": "opencode-go"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `device_code` | string | 是 | `auth/deviceStart` 返回的 `device_code`，原样回传 |
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider；非 opencode-go → `-32602` + `reason:"unsupported_provider"` |

- **响应**：`AuthDevicePollResult`（**待授权不是错误**——`pending` / `slow_down` 同样以成功响应返回）。

```json
{ "jsonrpc": "2.0", "id": 16, "result": { "state": "pending" } }
```

```json
{ "jsonrpc": "2.0", "id": 16, "result": { "state": "slow_down", "interval": 10 } }
```

```json
{ "jsonrpc": "2.0", "id": 16, "result": {
  "state": "authorized",
  "credential": { "kind": "oauth", "source": "file", "expires_at": 1700000600, "expiring_soon": false, "org_id": "org-…" }
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `state` | DeviceAuthState | 是 | `pending` \| `slow_down` \| `authorized` \| `expired` \| `denied` \| `error`（见 §9.1） |
| `interval` | integer | 否 | 仅 `slow_down`：下一次轮询间隔秒数（= 本次间隔 + **5s**） |
| `message` | string | 否 | 仅 `error`：未识别上游错误的**原文**（不猜测、不改写） |
| `credential` | CredentialInfo | 否 | 仅 `authorized`：刚落盘的 OAuth 凭据脱敏投影（字段同 §8.24；`expiring_soon` 恒 `false`） |

- **状态语义（RFC 8628 映射）**：`authorization_pending` → `pending`；`slow_down` → `slow_down` + `interval`（客户端据此**延长**轮询）；`200` → `authorized`（**唯一写盘的一次调用**：`AuthStore::set` 写入 `auth.json`，目录 `0700` / 文件 `0600` 原子写）；`access_denied` → `denied`；`expired_token` / 设备码用尽 → `expired`；其余非空上游错误字符串 → `error` + 原文。
- **覆盖语义**：同一 provider 再次授权成功 = **替换**既有凭据（`AuthStore::set`），不新增多账号概念，也不新增 wire 字段。
- **凭据长期可用**：返回的 OAuth 凭据其 access token 生命期为 **600s**，内核在每轮真实请求前自动刷新（`auth.json.lock` 文件锁串行化 + 重读校验；refresh token **单次使用**，复用即吊销整个会话）；刷新失败 → 下一轮 run 以可行动错误失败，**绝不回落到空 Bearer 或旧 token 重试**。`auth/status` 是纯读，永不触发该刷新。
- **部分结果**：无（客户端按 `interval` 自行循环调用）。
- **error.data**：未知 provider / 非 opencode-go → `-32602`（同上两种 `reason`）；`device_code` 缺失 / 类型非法 → `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；上游不可达 / 非 200 且无错误码 / 响应畸形 → `-32603` + `{ "reason": "<上游或网络错误原文>" }`（未识别错误码不走此通道，而是 `state:"error"`）；落盘失败 → `-32603` + `{ "reason": "<原文>" }`；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.28 auth/setApiKey（请求，`@since 0.18.0`）

- **能力声明**：凭据链接能力，**仅 opencode-go** 支持（不支持时 `capability` 为 `api_key`）。需注入 `provider_service` handle；未注入 → `-32603` + `data{reason:"provider_service_unavailable"}`。
- **请求**：`AuthSetApiKeyParams`。

```json
{ "jsonrpc": "2.0", "id": 17, "method": "auth/setApiKey", "params": {
  "provider": "opencode-go",
  "api_key": "sk-…",
  "label": "web"
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider |
| `api_key` | string | 是 | 待链接的 API Key；`trim()` 后为空 → `-32602` + `reason:"invalid_params"`，且**不发任何上游请求、不写盘** |
| `label` | string | 否 | 用户自填标签，随凭据一起落盘（`auth.json` 的 api_key 条目 `label`）；不参与解析优先级 |

- **响应**：`AuthSetApiKeyResult`（**三种结果均为成功响应**——见下方的三态语义）。

```json
{ "jsonrpc": "2.0", "id": 17, "result": {
  "provider": "opencode-go",
  "saved": true,
  "outcome": "verified",
  "credential": { "kind": "api_key", "source": "file", "key_hint": "…1234", "expiring_soon": false },
  "source": "account",
  "effective_source": "file",
  "fetched_at": 1700000000,
  "models": [ { "id": "opencode-go/kimi-k3", "verified": true, "format": "anthropic" } ]
} }
```

```json
{ "jsonrpc": "2.0", "id": 17, "result": {
  "provider": "opencode-go",
  "saved": false,
  "outcome": "rejected",
  "error": { "kind": "unauthorized", "message": "API Key 无效或已被撤销（未保存）：请在 opencode.ai 控制台重新复制" },
  "effective_source": "file",
  "fetched_at": 1700000000,
  "models": []
} }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际链接的 provider 名 |
| `saved` | boolean | 是 | 凭据是否已写入 `auth.json`（`rejected` 恒 `false`，`verified` / `unverified` 恒 `true`） |
| `outcome` | ApiKeyOutcome | 是 | `verified` \| `unverified` \| `rejected`（见 §9.1） |
| `error` | ApiKeyLinkError | 否 | 仅 `rejected`：`{ kind, message }`，见下 |
| `credential` | CredentialInfo | 否 | 仅已落盘：脱敏投影，字段同 §8.24；`source` = `effective_source`、`key_hint` = `redact()` 输出（形如 `…1234`）、`expires_at` / `org_id` 恒缺省 |
| `source` | ModelSource | 否 | 仅已落盘：本次目录层级 `account` \| `live` \| `overlay`（`rejected` 时省略） |
| `effective_source` | CredentialSource | 是 | **真正生效**的凭据来源（`file` / `env`）；`api_key_env` 指向的变量非空时为 `env`，此时刚存的 key **暂不生效** |
| `fetched_at` | integer | 是 | 本次链接的时刻（epoch **秒**，int64） |
| `models` | ModelEntry[] | 是 | 与本次目录一致的条目（`rejected` 时为空数组；条目结构同 §8.23） |

- **三态语义（领域失败即结果数据，不新增错误码）**——一次 `GET {console}/api/v2/config` 调用同时决定校验与目录：
  1. **`verified`**：`200` 且解析出账号级 opencode-go 目录 → 落盘 + 运行期激活 + 播种 60s 模型缓存；`source:"account"`，`models` 为账号 id 逐条映射（命中 overlay 沿用其 `format` / `verified`，未命中合成为 `openai-compat` + `verified:false`——**绝不按名字猜路由**）。
  2. **`rejected`（401 / 403）**：**不落盘、不注册**，`saved:false`；`error.kind` 为 `unauthorized`（401）或 `entitlement`（403），`error.message` 是**内核自产**文案，**绝不回显上游响应体**（含 `_tag` 等）。`effective_source` 保持 `file`——失败的尝试不改动解析优先级。
  3. **`unverified`（超时 / 网络不可达 / 5xx / 畸形响应 / 200 但目录解析不出 go 模型）**：**落盘**并标注未校验，`saved:true`；目录退到 live ∩ overlay（live 也失败 → overlay 全集，**绝不编造目录**），`source` 为 `live` / `overlay`。
- **`ApiKeyLinkError.kind` 是普通字符串**（`String`，不是值枚举；先例 `ProviderUsageError.kind`）：本期内核只产出 `unauthorized` / `entitlement` 两个取值，客户端 MUST 容忍未知取值（按 §9.2 处理），`message` 恒为内核自产明文。
- **覆盖语义**：同一 provider 再次链接 = **替换**既有凭据（`AuthStore::set`），不新增多账号概念、不新增 wire 字段；已有 OAuth 凭据时同样替换（提交前的提示由 UI 负责）。
- **运行期激活**：落盘后立刻重建该 provider 实例并**替换注册**（`ProviderRegistry` 内部为 `RwLock`，`register` 为单次写），同时失效该 provider 的两处进程内缓存，使紧随其后的 `session/run` / `model/list` / `provider/usage` / `auth/status` 立即可用；`auth/devicePoll` 的 `authorized` 分支出走**同一激活路径**。
- **风险说明（时序）**：替换注册与并发 `session/run` 之间**没有全局互斥**——换入瞬间**进行中的那一轮 run 可能仍使用旧凭据，新凭据从下一轮生效**。
- **不泄露（no-secret 不变式）**：结果只含 `redact()` 投影出的 `key_hint`，完整 key **绝不**出现在结果、日志或错误文案中（`redact()` 是唯一投影方式）；结果中不存在 `api_key` / `key` / `access` 字段。
- **写入目标**：凭据写入**当前连接内核所在机器**的 `auth.json`（目录 `0700` / 文件 `0600` / 原子写）；`--attach` / 远程 `serve` 场景下即远端内核主机，非回环 `serve --host` 必须配 `--token`。
- **部分结果**：无（一次性快照 + 目录复用窗口；本次结果已播种 60s 缓存，带 `refresh` 的 `model/list` 仍强制重拉）。
- **error.data**：未知 provider → `-32602` + `{ "field": "provider", "reason": "unknown_provider", "registered": ["<名>", …] }`；provider 已知但非 opencode-go → `-32602` + `{ "field": "provider", "reason": "unsupported_provider", "capability": "api_key" }`；`api_key` 为空 / 全空白 → `-32602` + `{ "field": "api_key", "reason": "invalid_params" }`；params 结构畸形（如 `api_key` 为数字 / 对象）→ `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；凭据落盘失败 → `-32603` + `{ "reason": "<原文>" }`；凭据已落盘但运行期激活失败 → `-32603` + `{ "reason": "activation_failed", "detail": "<自产原因>" }`（可重试）；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

### 8.29 auth/clearCredential（请求，`@since 0.18.0`）

- **能力声明**：凭据解除能力，**仅 opencode-go** 支持（不支持时 `capability` 为 `api_key`，与 §8.28 一致）。需注入 `provider_service` handle；未注入 → `-32603` + `data{reason:"provider_service_unavailable"}`。
- **请求**：`AuthClearCredentialParams`。

```json
{ "jsonrpc": "2.0", "id": 18, "method": "auth/clearCredential", "params": { "provider": "opencode-go" } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 否 | provider 名；省略 / `null` = 内核配置的默认 provider；未知 → `-32602` + `reason:"unknown_provider"` |

- **响应**：`AuthClearCredentialResult`（**没有可清除的条目也是成功响应**）。

```json
{ "jsonrpc": "2.0", "id": 18, "result": { "provider": "opencode-go", "cleared": true } }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `provider` | string | 是 | 实际解除的 provider 名 |
| `cleared` | boolean | 是 | 是否真的移除了已存条目；无条目 → `false`（仍是成功响应，不是错误） |

- **收敛语义**：移除 `auth.json` 中该 provider 的条目（文件变空即删文件，沿用既有行为）+ 失效该 provider 的模型 / 用量两处进程内缓存，然后**重新解析剩余凭据**并收敛注册表：
  - 仍有 key（`api_key_env` 非空）→ 按该 key 重建并替换注册（订阅仍可用，只是不再用文件凭据）；
  - 无任何 key → **注销**该 provider（与启动期「无密钥即跳过」严格一致），此后 `auth/status` 转 `authenticated:false`、`model/list` 退到 live ∩ overlay；
  - 绝不保留持有旧密钥的实例（「已解除链接但仍在用旧 key」是不可接受状态）。
- **风险说明（时序）**：与 §8.28 相同——注销 / 替换注册没有全局互斥，**进行中的一轮 run 可能仍持有旧实例**，下一轮生效。
- **不泄露（no-secret 不变式）**：结果只含 `provider` / `cleared`，不含任何凭据字段（含 `key_hint` 也不返回）。
- **部分结果**：无。
- **error.data**：未知 provider → `-32602` + `{ "field": "provider", "reason": "unknown_provider", "registered": ["<名>", …] }`；provider 已知但非 opencode-go → `-32602` + `{ "field": "provider", "reason": "unsupported_provider", "capability": "api_key" }`（**凭据与注册表均不被触碰**，与 §8.28 同值）；params 结构畸形 → `-32602` + `{ "reason": "invalid_params", "detail": "<原文>" }`；凭据文件不可读 / 权限不合规 / 落盘失败 → `-32603` + `{ "reason": "<原文>" }`；收敛（注销 / 重建注册）失败 → `-32603` + `{ "reason": "activation_failed", "detail": "<自产原因>" }`；handle 未注入 → `-32603` + `{ "reason": "provider_service_unavailable" }`。

## 9. 值枚举与未知值宽容

### 9.1 值枚举

当前定义以下字符串值枚举（`Role` / `ModuleState` / `SessionStatus` / `PermissionDecision` 为 v0.2 既有）：

| 枚举 | 已知值 | 用于 |
|------|--------|------|
| `Role` | `system` \| `user` \| `assistant` \| `tool` | 消息角色 |
| `ModuleState` | `unloaded` \| `initializing` \| `ready` \| `stopping` \| `stopped` \| `failed` | 内核模块状态 |
| `SessionStatus` | `active` \| `archived` \| `deleted` | 会话状态 |
| `PermissionDecision` | `allow` \| `deny` \| `allow_always` | 审批决策（`@since 0.2.0`，见 8.13） |
| `FileKind` | `file` \| `dir` | `file/list` 条目类型（`@since 0.13.0`，见 8.20） |
| `TodoStatus` | `pending` \| `in_progress` \| `completed` \| `cancelled` | TODO 条目状态（`@since 0.13.0`，见 7.3） |
| `FileChangeKind` | `add` \| `modify` \| `delete` | 文件变更操作（`@since 0.13.0`，见 7.3） |
| `FileChangeSource` | `write` \| `bash` | 文件变更来源（`@since 0.15.0`，见 7.3；`write`=LCS diff 口径，`bash`=numstat 计数口径） |
| `ModelSource` | `account` \| `live` \| `overlay` \| `static` | `model/list` 目录来源层级（`@since 0.17.0`，见 8.23） |
| `CredentialKind` | `api_key` \| `oauth` | `auth/status` 凭据种类（`@since 0.17.0`，见 8.24；`oauth` 即 `auth.json` 中 `type:"oauth"` 的 OAuth 条目） |
| `CredentialSource` | `flag` \| `env` \| `file` | 凭据来源（`@since 0.17.0`，见 8.24；本期内核实现只产生 `env` / `file`，`flag` 为加性预留） |
| `UsageWindowKind` | `rolling` \| `weekly` \| `monthly` | 用量窗口种类（`@since 0.17.0`，见 8.25） |
| `UsageWindowStatus` | `ok` \| `rate-limited` | 用量窗口状态（`@since 0.17.0`，见 8.25） |
| `DeviceAuthState` | `pending` \| `slow_down` \| `authorized` \| `expired` \| `denied` \| `error` | 设备码轮询状态（`@since 0.17.0`，见 8.27） |
| `ApiKeyOutcome` | `verified` \| `unverified` \| `rejected` | `auth/setApiKey` 链接结果（`@since 0.18.0`，见 8.28） |

上述枚举均带 `Unknown` 变体容忍前向值（`FileKind` / `TodoStatus` / `FileChangeKind` / `FileChangeSource` 与 0.17.0 新增的 `ModelSource` / `CredentialKind` / `CredentialSource` / `UsageWindowKind` / `UsageWindowStatus` / `DeviceAuthState` 与 0.18.0 新增的 `ApiKeyOutcome` 由 `serde(other)` + `schemars(skip)` 实现，schema 只列已知值；`git_status` 的 `M`/`A`/`U`/`D` 为 `FileEntry` 内联的固定字母集合，见 8.20）。未知取值反序列化为 `Unknown`，**重序列化一律为规范字面量 `"unknown"`**（不保留原始字符串）——因此仅适用于「写回不敏感」的字段。

### 9.2 未知值宽容规则

- 客户端 / 服务端收到**未知枚举值**时，MUST 将其归入对应的 **Unknown 变体**（保留原始字符串以便透传），MUST NOT 因此失败或断开连接。
- 例如：`ModuleState` 出现 `"suspended"` 时，消费方将其视为 Unknown 状态继续运行；`SessionStatus` 出现未知值时，可视为「非 active」处理但不崩溃。
- 枚举扩展属**加性变更**（新值不改变既有值语义），只随 minor 版本增加。

### 9.3 Schema 纪律

- `schemas/*.json` 中的枚举 MUST 只列出**已知值**（见各枚举 schema），未知值由实现按 9.2 处理，不写入 schema。
- Schema 校验通过不代表消息语义合法（例如未知枚举值校验不通过但运行时允许），实现 MUST 在运行时层应用 9.2 的宽容规则。
- 协议扩展（0.13.0）新增的 `-32807 PathNotAllowed`、`$/progress` 的 `todo` / `file_change` kind、`file/list`、`$/askQuestion` / `question/respond`、`session/new` 的 `project_root`，以及 `FileKind` / `TodoStatus` / `FileChangeKind` 三个值枚举，均遵循该加性纪律并标注 `@since 0.13.0`；`session/list` / `session/history` 与 `usage` / `permission` kind 为 0.12.0 引入，本规范补录其小节（§8.18 / §8.19、§7.3）。
- 协议扩展（0.14.0）新增的 `file/list` 参数 `query` 与结果 `total`、`SessionNewResult.project_root`、`SessionStatusResult.project_root` 同为**加性字段**（标注 `@since 0.14.0`；不新增方法、不新增 schema、不改既有字段语义）。
- 协议扩展（0.15.0）新增的 `FileChangeSource` 值枚举与 `FileChange` / `ProgressValue::Todo` / `ProgressValue::FileChange` / `TrajectoryValue::Todo` / `TrajectoryValue::FileChange` / `SessionRunResult` / `SessionHistoryParams` 的 `run_id` / `seq` / `source` / `total_bytes` 四加性字段同为**加性字段**（标注 `@since 0.15.0`；不新增方法、不改既有字段语义；新增 `FileChangeSource.json` 一个 schema，去重键由全局 `path` 升为 `(run_id, path)`）。
- 协议扩展（0.16.0）新增的 `session/run` 参数 `provider` / `model` 同为**加性字段**（标注 `@since 0.16.0`；不新增方法、不新增 schema——仅更新 `SessionRunParams.json` 内容，21 方法 / 52 schemas 不变；两者均省略 / `null` / 空串时行为逐字不变）。
- 协议扩展（0.17.0）新增 **5 个请求方法**（`model/list` / `auth/status` / `provider/usage` / `auth/deviceStart` / `auth/devicePoll`）与 **20 个 schema**（`ModelListParams` / `ModelListResult` / `ModelEntry` / `ModelSource`；`AuthStatusParams` / `AuthStatusResult` / `CredentialInfo` / `CredentialKind` / `CredentialSource`；`ProviderUsageParams` / `ProviderUsageResult` / `ProviderUsageError` / `UsageWindow` / `UsageWindowKind` / `UsageWindowStatus`；`AuthDeviceStartParams` / `AuthDeviceStartResult` / `AuthDevicePollParams` / `AuthDevicePollResult` / `DeviceAuthState`）与 6 个新值枚举，全部标注 `@since 0.17.0`；方法数 21 → 26、schema 总数 52 → 72，**不新增错误码**（复用 `-32602` / `-32603` + `error.data.reason`，见 §3.4），既有方法 / 既有字段的语义与 wire 形状零变化——全部为加性变更，此前客户端零破坏。

## 10. 机器可读模型

- **`protocol/methods.json`**：方法注册表，捕获 JSON Schema 表达不了的方法语义（method 字符串 ↔ params/result schema 引用、方向、类型）。当前收录 28 个方法，`protocol_version` 为 `0.18.0`。

对应关系：

| 注册表方法 | params schema | result schema |
|-----------|---------------|---------------|
| `initialize` | `InitializeParams.json` | `InitializeResult.json` |
| `initialized` | — | — |
| `shutdown` | — | — |
| `exit` | — | — |
| `session/new` | `SessionNewParams.json` | `SessionNewResult.json` |
| `session/run` | `SessionRunParams.json` | `SessionRunResult.json` |
| `session/status` | `SessionIdParams.json` | `SessionStatusResult.json` |
| `session/archive` | `SessionIdParams.json` | — |
| `session/delete` | `SessionIdParams.json` | — |
| `session/subscribe` | `SessionSubscribeParams.json` | — |
| `session/unsubscribe` | `SessionIdParams.json` | — |
| `session/list` | `SessionListParams.json` | `SessionListResult.json` |
| `session/history` | `SessionHistoryParams.json` | `SessionHistoryResult.json` |
| `permission/respond` | `PermissionRespondParams.json` | `PermissionRespondResult.json` |
| `kernel/status` | — | `KernelStatusResult.json` |
| `$/permissionRequest` | `PermissionRequestParams.json` | — |
| `$/cancelRequest` | `CancelParams.json` | — |
| `$/progress` | `ProgressParams.json` | — |
| `file/list` | `FileListParams.json` | `FileListResult.json` |
| `$/askQuestion` | `AskQuestionParams.json` | — |
| `question/respond` | `QuestionRespondParams.json` | `QuestionRespondResult.json` |
| `model/list` | `ModelListParams.json` | `ModelListResult.json` |
| `auth/status` | `AuthStatusParams.json` | `AuthStatusResult.json` |
| `provider/usage` | `ProviderUsageParams.json` | `ProviderUsageResult.json` |
| `auth/deviceStart` | `AuthDeviceStartParams.json` | `AuthDeviceStartResult.json` |
| `auth/devicePoll` | `AuthDevicePollParams.json` | `AuthDevicePollResult.json` |
| `auth/setApiKey` | `AuthSetApiKeyParams.json` | `AuthSetApiKeyResult.json` |
| `auth/clearCredential` | `AuthClearCredentialParams.json` | `AuthClearCredentialResult.json` |

信封与公共类型见 `RequestMessage.json`、`NotificationMessage.json`、`ResponseMessage.json`、`ErrorObject.json`、`Id.json`、`ProgressToken.json`、`ProgressValue.json`、`Role.json`、`PermissionDecision.json` 等；协议扩展（0.13.0）新增 / 更新的公共类型见 `SessionNewParams.json`、`SessionSummary.json`、`FileListParams.json`、`FileListResult.json`、`FileEntry.json`、`FileKind.json`、`AskQuestionParams.json`、`AskOption.json`、`QuestionRespondParams.json`、`QuestionRespondResult.json`、`TodoItem.json`、`TodoStatus.json`、`FileChange.json`、`FileChangeKind.json`、`UsageInfo.json`。协议扩展（0.14.0）不新增 schema，仅向既有 schema 追加字段：`FileListParams.json`（`query`）、`FileListResult.json`（`total`）、`SessionNewResult.json` 与 `SessionStatusResult.json`（`project_root`）；协议扩展（0.15.0）新增 `FileChangeSource.json` 一个 schema（`FileChange`/`ProgressValue`/`TrajectoryValue`/`SessionHistoryParams`/`SessionRunResult` 的 `run_id`/`seq`/`source`/`total_bytes` 为加性字段）；协议扩展（0.16.0）不新增 schema，仅向既有 `SessionRunParams.json` 追加 `provider` / `model` 两个加性可选字段；协议扩展（0.17.0）新增 **20 个 schema**（`ModelListParams` / `ModelListResult` / `ModelEntry` / `ModelSource`；`AuthStatusParams` / `AuthStatusResult` / `CredentialInfo` / `CredentialKind` / `CredentialSource`；`ProviderUsageParams` / `ProviderUsageResult` / `ProviderUsageError` / `UsageWindow` / `UsageWindowKind` / `UsageWindowStatus`；`AuthDeviceStartParams` / `AuthDeviceStartResult` / `AuthDevicePollParams` / `AuthDevicePollResult` / `DeviceAuthState`），并新增 `ModelSource` / `CredentialKind` / `CredentialSource` / `UsageWindowKind` / `UsageWindowStatus` / `DeviceAuthState` 六个值枚举（均带 `Unknown` 容忍前向值）；schema 总数 52 → **72**。协议扩展（0.18.0）新增 **6 个 schema**（`AuthSetApiKeyParams` / `AuthSetApiKeyResult` / `ApiKeyLinkError` / `ApiKeyOutcome`；`AuthClearCredentialParams` / `AuthClearCredentialResult`），并新增 `ApiKeyOutcome` 一个值枚举（带 `Unknown` 容忍前向值，见 §9.1）；schema 总数 72 → **78**。

一致性锚点：Rust 侧 `schemars` 生成 `schemas/*.json` 并提交，契约测试断言实际 wire 输出与 Schema 一致，CI 漂移门禁在生成结果与提交版本不一致时失败。

## 11. 版本化与变更日志

### 11.1 版本策略

协议版本遵循语义化版本（semver），规则见 §4.3：major 破坏 / minor 加方法或加性字段 / patch 修 bug。当前版本 `0.18.0`（`protocol/methods.json` 中 `protocol_version` 字段为准）。

### 11.2 命名归一化决策（v0.1.0 记录）

**决策**：wire 字段名统一 **snake_case**。

设计文档 `2026-08-14-agentj-ipc-protocol-design.md` 决策 #7 的方法表混用了 camelCase（`protocolVersion`、`clientInfo`）与 snake_case（`session_id`）。经计划级决策确认，wire 格式归一化为 snake_case，与 v0.1 既有 serde 类型（`session_id`、`tool_calls`、`input_schema`）一致。典型对照：

| camelCase（设计文档字面） | snake_case（本规范 wire 格式） |
|---------------------------|-------------------------------|
| `protocolVersion` | `protocol_version` |
| `clientInfo` | `client_info` |
| `serverInfo` | `server_info` |
| `progressToken` | `progress_token` |
| `toolCalls` | `tool_calls` |
| `inputSchema` | `input_schema` |

本规范是权威契约，一律使用 snake_case。各语言绑定从 `schemas/*.json` 生成时按 Schema 字段名原样导出，不另做映射。

### 11.3 变更日志

#### 0.1.0

- 初版协议规范，定义 12 个方法（4 个生命周期方法、5 个 `session/*` 方法、`kernel/status`、`$/cancelRequest`、`$/progress`）。
- 定义 JSON-RPC 2.0 信封、Id 规则、错误码表（JSON-RPC 保留段 + `-32000 … -32099` 实现自定义段 + agentj 块 `-32800` … `-32804`）。
- 定义 `initialize` 握手（semver 版本协商 + 能力交换）、`$/progress` 流式（token 解耦，`progress_token` 客户端可选、缺省用请求 id 作 token）、`$/cancelRequest` 取消（被取消请求仍须以 `-32800` 响应）。
- 记录 snake_case 命名归一化决策。
- 明确 batch 语义保留但 v0.1 未启用。

#### 0.2.0（权限系统）

- 方法面 12 → **14**（注册表）：新增 `permission/respond`（请求）与 `$/permissionRequest`（通知），支撑工具审批流（见 §7.5）。
- 新增**权限审批流**（设计决策 #17）：`request_id` 与 JSON-RPC 请求 id 解耦（沿用 `$/progress` token 模式）；60s 默认审批超时 → 自动 `Deny`；`allow_always` 仅写入会话级内存态 `SessionAllowState`，不写回 `[permission]` 配置；`$/cancelRequest` → 待审批解析为 `Deny`（reason `cancelled`）。
- 值枚举新增 `PermissionDecision`（`allow` / `deny` / `allow_always` + Unknown）。
- 全部为加性变更（只加不改），v0.1 客户端零破坏。

#### 0.3.0（守护内核 + 商业扩展）

- 方法面 14 → **16**（注册表）：新增 `session/subscribe`（请求）与 `session/unsubscribe`（请求），支撑多客户端订阅广播（见 §8.11 / §8.12）。
- 错误码新增 2 个：`-32805 SessionBusy`（同 session 并发 run 互斥）、`-32806 NotAuthorized`（非 owner 的 respond / cancel、`--token` 不匹配）。
- `initialize` 结果新增可选字段 `connection_id`；服务端能力新增 `multi_client` 标志（见 §4.2）；`initialize` 参数新增可选字段 `token`（对应 `--token`，见 §4.1）。
- `$/permissionRequest` 新增可选字段 `origin_client`（属主定向路由，见 §8.15）；`permission/respond` 发起方归属校验 → 非 owner → `-32806`。
- `session/run` 并发互斥（同 session 活动 run → `-32805`）；发起方隐式订阅（见 §8.11）。
- `shutdown` 多客户端语义：触发**全局**优雅关闭链（见 §4.5 / §2.5）。
- 全部为加性变更（只加不改），此前客户端零破坏。

#### 0.10.0（G7 EventBus 重入防护 + G8 WebSocket 网关）

- **传输层**：新增 `2.3 WebSocket`（`ws://127.0.0.1:<port>/ws`，与 raw TCP 同端口共存，`MSG_PEEK` + `looks_like_http` 分类器），每条 WS Text 帧 = 一条 JSON-RPC 消息；鉴权双通道 `?token=` 403 + `initialize.token` -32806；`max_connections` 对 WS 独立计数，WS 不参与最后连接断开全局关闭。
- **事件总线重入防护（G7）**：`EventGuard` thread_local `EVENT_DEPTH`/`EVENT_STACK` + `MAX_EVENT_DEPTH=16` + 同栈循环检测 + `tracing::warn!` 丢弃 + RAII Drop 回卷。
- 协议 wire 零破坏（加性传输扩展）。

#### 0.13.0（协议扩展：`file/list` + progress kind + 结构化提问）

- **方法面 18 → 21**：新增 **3 个方法**——`file/list`（请求，沙箱化目录列举）、`$/askQuestion`（通知，owner 定向模型提问）、`question/respond`（请求，响应提问）；同时**补录** 0.12.0 已引入的 `session/list` / `session/history` 小节（§8.18 / §8.19），§8.1 方法总表与 §10 映射表补齐至 21 行。
- **`$/progress` 新增 kind**：`usage` / `permission`（`@since 0.12.0`）与 `todo` / `file_change`（`@since 0.13.0`）补入 §7.3 判别表；`todo` 为全量快照（`revision` 单调递增，客户端丢弃比已见更旧的快照），`file_change` 携带 `FileChange`（行数 + unified diff）。未知 `kind` 仍按 `$/` 前缀约定忽略。
- **新错误码 `-32807 PathNotAllowed`**（agentj 块向下加性分配）：`file/list` 请求路径（含符号链接逃逸）落在会话 `project_root` 之外；`error.data = { path, reason: "outside_project_root" }`（见 §3.4 / §8.20）。
- **`session/new` 加性参数** `SessionNewParams.project_root`（可选绝对目录，canonicalize 后随会话持久化；缺省 = 进程 CWD）；旧客户端不传 params 行为不变。
- **新增值枚举**：`FileKind`、`TodoStatus`、`FileChangeKind`（均带 `Unknown` 变体容忍前向值，见 §9.1）；schema 计数 38 → 51。
- 全部为**加性变更（只加不改）**，此前客户端零破坏；协议版本 minor 递增 `0.12.1 → 0.13.0`。

#### 0.19.0（内核托管 Web UI：HTTP 静态托管 + 混合鉴权 + 文件树目录聚合状态）

- **wire 零变化**：方法面、`schemas/*` 与 `PROTOCOL_VERSION`（保持 `0.13.0`）均不变；新增能力全部位于 **HTTP 层**。
- **HTTP 面新增**：同一端口静态托管 Web UI（`serve --web-dist <dist>` / `AGENTJ_WEB_DIST` / 自动发现；`/` → `index.html`、`/assets/*` 长缓存、未知页面路径回落 index 200）；混合鉴权（非回环 peer + token 时静态资源经登录门：`/login` 内嵌表单 + `agentj_session` 不透明会话 cookie（`HttpOnly; SameSite=Lax`）+ `/logout` 撤销）；WebSocket `/ws` 新增 cookie 通道（须同源 `Origin`），`?token=` 与 `initialize.token` 通道不变。
- **文件树目录聚合状态**：`file/list` 结果的目录项填充**既有可选字段** `FileEntry.git_status`（后裔前缀聚合，优先级 `U > D > M > A`），schema 与 wire 契约零变化。
- 全部为 HTTP 层新增 + 存量可选字段填充，此前客户端零破坏；工作区 `0.18.1 → 0.19.0`。

#### 0.20.0（内核托管面加固 + TUI 文件视图：HTTP 面与构建工具链）

- **wire 零变化**：方法面、`schemas/*` 与 `PROTOCOL_VERSION`（保持 `0.13.0`）均不变；新增能力全部位于 **HTTP 层与构建工具链**（`GET /version` 为 HTTP 静态面路由，**非** JSON-RPC 方法）。
- **HTTP 面新增**：`GET /version` 未鉴权版本探针（`200 application/json` + `Cache-Control: no-store`，返回内核 `product_version`/`protocol_version` 与 dist 版本事实 `dist_present`/`dist`；显式路由优先于 SPA fallback、不置于登录门后）；`/login` POST 登录限流（按来源 IP 计数，第 5 次失败起指数锁定 30s ×2 封顶 15min，`429 + Retry-After`（秒、向上取整、≥1），锁定期不校验 token 且新尝试升级锁定，成功清零）。
- **构建工具链新增**：web 构建生成 `dist/version.json`（`product_version`/`protocol_version`/`build_time`）并向入口 HTML 同源引用注入 `sha384` `integrity`（本地引用缺失即构建失败）；内核启动比对 dist 版本，缺失/不可解析/不一致 → 启动警告（**不阻断服务**）。
- 另含**纯前端** TUI 侧栏 `Files` 视图（复用既有 `file/list`，无 wire 变化）；全部为 HTTP 层与构建期新增，此前客户端零破坏；工作区 `0.19.0 → 0.20.0`。

#### 0.21.0（限流可观测 + 资产完整性 + TUI 搜索 + Web 焦点作用域：HTTP 层与前端）

- **wire 零变化**：方法面、`schemas/*` 与 `PROTOCOL_VERSION`（保持 `0.13.0`）均不变；新增能力全部位于 **HTTP 层与前端**（`GET /version` 加性块为 HTTP 静态面字段，**非** JSON-RPC 方法）。
- **HTTP 面新增（① ②）**：`login_limiter` tracing 事件（锁定新触发 `warn`、成功登录 `info`、明细 `debug`）+ CLI `RUST_LOG` 支持（`EnvFilter`，默认 `info`）；`GET /version` 加性 `login_limiter` 聚合计数块（`attempts`/`failures`/`lockouts`/`blocked`/`locked_now`/`tracked_ips`，无 per-IP 明细）与 `integrity` 清单摘要块（`manifest_present`/`checked`/`mismatched`/`missing`）；构建期 `dist/asset-manifest.json`（`sha384` 覆盖全部 bundle 产物 + 重写后 `index.html`，排除 `version.json`/自身/`public/` 透传）与内核启动逐文件校验（警告并入 `startup_warnings()`，**不阻断服务**）。限流策略语义（429/Retry-After/锁定升级/清零）零变化。
- **纯前端新增（③ ④）**：TUI Files 搜索（`s` 进入 + `depth=8` 单请求扁平索引 + 客户端过滤，`Esc` 清空后退出、`Enter` 跳转并展开祖先、`r` 重建、`truncated` 显示 `· partial`）与展开状态持久化 `filetree.json`（按本地项目键，per-key ≤100、恢复 ≤50、失效路径静默跳过）；Web 焦点作用域（`useShortcutScope`：焦点位于侧栏且无模态/菜单时 `f`/`n`/`r`/`j`/`k`/`Esc` 生效，**不劫持 Tab**）+ 文件树/会话列表 roving 键盘导航 + `ModalDialog` 焦点恢复。均为前端行为，无 wire 变化。
- 全部为 HTTP 层与服务/前端行为新增，此前客户端零破坏；工作区 `0.20.1 → 0.21.0`。

#### 0.14.0（指标导出 + 清单扩展抽检 + `file/list` 搜索 + 会话有效根）

- **方法面不变**：仍为 21 方法、51 schemas；本版本为**三处加性 wire 变更**，无新方法、无新 schema、无新错误码。
- **HTTP 面新增（仅 HTTP 层，不属 JSON-RPC 方法面）**：鉴权门控的 `GET /metrics`（Prometheus 文本 `text/plain; version=0.0.4` + `Cache-Control: no-store`；聚合计数 + per-IP 序列 Top-50 截断 + 资产完整性摘要；门控 `auth_required()` = 已配 token ∧ 非回环 ∧ 无 cookie，未通过 → `401 text/plain` 不重定向）；`GET /version` 的 `integrity` 块加性嵌套 `runtime{rounds,checked,mismatched,missing,last_run_age_secs?}`（运行期 60s / 8 文件游标轮转抽检）；`asset-manifest.json` 覆盖扩展至 dist 实目录全部部署文件（含 `public/` 透传，bundle 键优先；排除 `version.json`/自身/隐藏文件）。既有 `/version`、静态面与限流策略契约零变化。
- **`file/list` 加性字段**：参数 `query`（trim 后空串视作缺省；`path` 子树内、忽略 `depth`、大小写不敏感子串匹配；访问预算 `SEARCH_VISIT_LIMIT = 20_000`；rank 名称前缀 > 名称子串 > 路径子串、tie-break 路径升序；返回 Top-100）与结果 `total`（匹配总数，预算受限时为**下界**，缺省模式省略）。缺省 listing 行为（`ENTRY_LIMIT=2000`、`depth` 1..=8、`truncated` 语义）零变化；旧内核忽略 `query` → 新客户端据「响应无 `total`」自愈降级为客户端过滤。
- **会话结果加性字段**：`SessionNewResult.project_root`（请求值 canonicalize 结果，请求缺省 = 内核 CWD）与 `SessionStatusResult.project_root`（有效根，与 `file/list` 沙箱根同源）。
- **纯前端（无 wire 变化）**：TUI Files 搜索改防抖服务端查询 + 旧内核降级、展开态键改为会话有效 `project_root`；Web 共享 roving 收口（TopbarMenu / SessionListMenu / SidebarTabs / SettingsDialog）+ 侧栏底部快捷键提示行。
- 协议版本 minor 递增 `0.13.0 → 0.14.0`；工作区 `0.21.1 → 0.22.0`；此前客户端零破坏。

#### 0.15.0（右边栏运行详情：`run_id` 维度 + `FileChangeSource` + `total_bytes`）

- **方法面不变**：仍为 21 方法、**52 schemas**（新增 `FileChangeSource.json` 一个）；本版本为**七处加性 wire 变更**，无新方法、无新错误码。
- **Protocol 加性字段**：`FileChange { run_id?, seq?, source?, total_bytes? }`（`FileChangeSource = write|bash`，`Unknown` 容忍前向值；`total_bytes` 为截断前原始 diff 字节数，仅 `diff_truncated:true` 时携带；`seq` 为 per-run 单调 `u64` 用于抽屉排序）与外层 `ProgressValue::Todo/FileChange { run_id? }`（路由提示，与 `change.run_id` 一致性以 `change.run_id` 为准）与 `TrajectoryValue::Todo/FileChange { run_id? }`（轨迹落盘与回放同源）与 `SessionHistoryParams { run_id? }`（UUID v4，>128 或解析失败 → `-32602` + `data{reason:"invalid_run_id"}`；缺省全量；过滤语义为先按 `run_id` 过滤 Todo/FileChange 再按 `cursor`/`limit` 分页，其余 kind 保持全量）与 `SessionRunResult { run_id? }`（本轮 `run_id` 回显，用于客户端关联订阅与回放过滤）。去重键由全局 `path` 升为 `(run_id, path)`，`revision` 按 run 单调（客户端按 `(run_id, revision)` 丢旧帧，`run_id=null` 时退化为会话级）。
- **行为细节**：`run_id` 生成 `Uuid::new_v4()`（内核 `TurnRunner` 转入点）；`todo_revisions` 改为 `HashMap<RunId,u64>`（全局唯一 UUID 键）；`DIFF_MAX_BYTES=64 KiB` 不变；bash 变更 `source=bash` 且 `diff=null` 不可展开，write 变更 `source=write` 可展开；截断时 inline 截断前缀 + hint `已截断（原始 N KiB）`，不新增 `file/diff` 懒加载。
- **纯前端（无 wire 变化）**：Web `todoState` / `changes` 归约改 `(run_id, revision)` / `(run_id, path)` 并分组展示（当前 run 置顶，`__legacy__` → "未分组（历史）"），抽屉按 `seq` / `eventSeq` 排序；`RightPanel` 按 run 分组、抽屉按 `seq`、source 徽标；`App` 捕获 `SessionRunResult.run_id` 并携带 `session/history {run_id}` 与 `FileTree` 跳转联动；TUI `TodoBlock` 兼容 `run_id`（仅只读）。
- 协议版本 minor 递增 `0.14.0 → 0.15.0`；工作区 `0.22.0 → 0.23.0`；此前客户端零破坏（旧客户端忽略新字段，旧数据 `run_id=null` 兼容为未分组）。

#### 0.16.0（运行期切模型）

- **方法面不变**：仍为 21 方法、**52 schemas**（仅 `SessionRunParams.json` 内容更新）；本版本为 `session/run` 的**两个加性可选字段**，无新方法、无新错误码。
- **`session/run` 加性参数**：`SessionRunParams { provider?, model? }`（均 `@since 0.16.0`，snake_case）——`provider` 为已注册 provider 名（= `kernel/status.providers[].id`，如 `opencode-go`），`model` 为模型 id（如 `opencode-go/kimi-k3`）。省略 / `null` / 空串或纯空白 = 今天行为（provider 走 `get_default()`、model 走 `config.model.default_model`）；覆盖**只作用于显式给出的维度**（只给 `provider` 时模型仍取配置默认，只给 `model` 时 provider 仍取内核默认），两者均为**按 run** 生效（不写会话快照、不改 `SessionRunResult`、不回显生效模型）。
- **校验与失败可行动**：非字符串类型（数字 / 对象 / 数组）→ `-32602`，`error.data` 携带 `{ "field": "provider" | "model" }`；未知但格式合法的 provider 名 → 本轮运行失败（与今天 `no default provider` 同一 turn 错误通道），文案含 `unknown provider: <name>` 与已注册 provider 名列表，**不回落默认 provider**；不属于所选 provider 的 `model` 沿用既有 `UnknownModel` 路径。
- **纯前端（无 wire 变化）**：Web composer 模型弹层与 TUI `/models` 选择器在选中后于**下一次** `session/run` 携带两个字段；未选择时**不携带**新字段（no-selection 路径逐字不变）；选择仅本地保存（Web localStorage / TUI 会话内存）。
- 协议版本 minor 递增 `0.15.0 → 0.16.0`；工作区 `0.25.2 → 0.26.0`；此前客户端零破坏（旧内核忽略未知字段 → 行为不变）。

#### 0.17.0（订阅面：用量 / 设备码登录 / 账号级目录）

- **方法面 21 → 26**：新增 5 个请求方法——`model/list`（三层目录：`account` → `live` → `overlay`；非 opencode-go 为 `static`）、`auth/status`（凭据状态**纯读**：`key_hint` / `org_id` / `expires_at` / `expiring_soon`，绝不轮换 token）、`provider/usage`（三窗口 `rolling` 5h / `weekly` / `monthly`，`percent` 为**已用**百分比，无余额字段）、`auth/deviceStart`（RFC 8628 设备授权起点）、`auth/devicePoll`（轮询状态：`pending` / `slow_down` / `authorized` / `expired` / `denied` / `error`）。五个方法的参数 / 结果契约见 §8.23–§8.27；§8.1 方法总表与 §10 映射表补齐至 26 行。
- **新 schema（20 个）**：`ModelListParams` / `ModelListResult` / `ModelEntry` / `ModelSource`、`AuthStatusParams` / `AuthStatusResult` / `CredentialInfo` / `CredentialKind` / `CredentialSource`、`ProviderUsageParams` / `ProviderUsageResult` / `ProviderUsageError` / `UsageWindow` / `UsageWindowKind` / `UsageWindowStatus`、`AuthDeviceStartParams` / `AuthDeviceStartResult` / `AuthDevicePollParams` / `AuthDevicePollResult` / `DeviceAuthState`；schema 总数 52 → **72**。
- **新值枚举（6 个）**：`ModelSource`、`CredentialKind`（`api_key` \| `oauth`）、`CredentialSource`、`UsageWindowKind`、`UsageWindowStatus`、`DeviceAuthState`——均带 `Unknown` 变体容忍前向值（仅列已知值入 schema，`Unknown` 重序列化为规范字面量 `"unknown"`，见 §9.1 / §9.2）。
- **不新增错误码**（见 §3.4）：未知 provider 名 → `-32602` + `data{field:"provider", reason:"unknown_provider", registered:[…]}`；provider 已知但不支持该能力 → `-32602` + `data{field:"provider", reason:"unsupported_provider", capability:"<usage|device_flow>"}`；`provider_service` handle 未注入 → `-32603` + `data{reason:"provider_service_unavailable"}`。上游 / 凭据失败（未登录 / 401 / 403 / 5xx / 网络）是**领域失败**，以成功响应的结果数据表达（`ProviderUsageResult.error.kind`、`AuthDevicePollResult.state`），使客户端能同时拿到「上次快照 + 失败原因」（`stale:true`）。
- **缓存与新鲜度语义**：`model/list` 进程内复用窗口 **60s**、`provider/usage` TTL **300s**（两者的 `refresh:true` 均强制绕过）；警示阈值 `warn_percent` 由 `[model].quota_warn_percent`（默认 80，clamp 1..=100）随结果下发；OAuth access token 生命期 **600s**，由内核在真实请求前自动刷新（`auth.json.lock` 文件锁串行化；refresh token 单次使用），刷新失败以可行动错误失败——以上均不改变 wire 契约（客户端只消费结果字段）。
- **客户端兼容契约**：未知枚举取值与未知 `error.kind` 字符串 MUST 容忍（不因未知而失败，按 §9.2 处理）；未注入 handle 的内核（如 `run --attach` 连接远端内核）统一 `-32603`，客户端展示「该内核未提供订阅服务」且**不缓存该失败**。
- 全部为**加性变更（只加不改）**：既有方法、既有字段的语义与 wire 形状零变化，此前客户端零破坏；协议版本 minor 递增 `0.16.0 → 0.17.0`。

#### 0.18.0（API Key 链接：`auth/setApiKey` + `auth/clearCredential`）

- **方法面 26 → 28**：新增 2 个请求方法——`auth/setApiKey`（链接 opencode-go API Key：一次上游调用决定校验三态 `verified` / `unverified` / `rejected`，落盘、运行期激活、播种 60s 模型缓存并返回账号级目录）与 `auth/clearCredential`（移除已存凭据并收敛注册：仍有 env key → 重建，无 key → 注销）；契约见 §8.28 / §8.29；§8.1 方法总表与 §10 映射表补齐至 28 行。
- **新 schema（6 个）**：`AuthSetApiKeyParams` / `AuthSetApiKeyResult` / `ApiKeyLinkError` / `ApiKeyOutcome` / `AuthClearCredentialParams` / `AuthClearCredentialResult`；新值枚举 `ApiKeyOutcome`（`verified` \| `unverified` \| `rejected`，带 `Unknown`）；schema 总数 72 → **78**。
- **不新增错误码**（见 §3.4）：复用 `-32602`（`reason` ∈ `invalid_params` / `unknown_provider` / `unsupported_provider`，后者 `capability:"api_key"`）与 `-32603`（`provider_service_unavailable`），并为「凭据已落盘但运行期注册 / 注销失败」新增 `-32603` + `error.data{reason:"activation_failed"}`。
- **领域失败即结果数据**：401 / 403 以**成功响应**表达（`saved:false` + `outcome:"rejected"` + `error.kind` ∈ `unauthorized` / `entitlement`，文案内核自产、不回显上游正文）；超时 / 5xx / 畸形响应为 `outcome:"unverified"` 且**仍落盘**（目录退到 live ∩ overlay，不编造）；`ApiKeyLinkError.kind` 为普通字符串（本期内核只产出上述两个取值）。
- **凭据语义零变化**：解析优先级仍为 `--api-key` flag > `api_key_env`（非空）> `auth.json`；`effective_source` 如实回告 env 遮蔽；结果仅经 `redact()` 暴露 `key_hint`，完整 key 不出现在结果 / 日志 / 错误文案中；`auth/status` 仍是纯读投影。
- **已知时序风险**：替换 / 注销注册与并发 `session/run` 之间无全局互斥——**进行中的一轮 run 可能仍使用旧凭据，下一轮生效**。
- 全部为**加性变更（只加不改）**：既有方法、既有字段的语义与 wire 形状零变化（`kernel/status` 逐字不变），此前客户端零破坏；协议版本 minor 递增 `0.17.0 → 0.18.0`。

#### 后续版本
