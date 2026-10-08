# opencode-go 订阅接入

AgentJ 把 [OpenCode Go](https://opencode.ai/docs/go/) 订阅作为一等 provider：登录一次、
凭据落盘，内核启动时发现订阅里的真实模型目录，Web / TUI 直接展示并高亮默认模型，
请求自带厂商要求的客户端标识头。本版本把订阅做成**可运维**：在 Web 设置 · 订阅（或 TUI
overlay）里粘贴 API Key 即可链接、看得到用量、用设备码登录、拿到账号级真实目录，并让只有
600s 生命的 OAuth access token 在长驻内核里自动保鲜。

## 两种登录方式

### 1. 工作区 API Key（长期密钥）

#### 在界面中链接（推荐）

**Web**：顶栏「设置…」→「订阅」tab →「链接方式」区块，把控制台复制的 Key 粘进输入框
（`type="password"` + `autoComplete="off"`，屏幕只显示圆点），点「链接」。
**TUI**：palette 运行 `subscription` 打开浮层，按 `i` 进入输入（逐字符只渲染遮罩），
`Enter` 提交、`Esc` 取消；按 `c` 解除链接。

- 输入**不写入 `localStorage`**，提交后立即清空输入框；完整 Key 既不出现在界面/帧里，
  也不出现在结果、日志或错误文案中（只回显 `redact()` 的 `…后 4 位`）。
- 凭据写入**当前连接内核所在机器**的 `auth.json`（`--attach` / 远程 `serve` 场景下即远端主机）。
  非回环 `serve --host` **必须配 `--token`**，否则任何能访问该端口的人都能录入 / 解除凭据。
- **链接成功即生效、免重启内核**：provider 当场重建并替换注册，模型目录立即刷新，
  当前未选 opencode-go 时自动选中首个账号条目（per-run 覆盖，**不改 `config.toml`**）。
  唯一时序例外：换入瞬间**进行中的那一轮 run 可能仍用旧凭据**，新凭据从下一轮生效。

一次「链接」= 一次 `auth/setApiKey` 调用（内核向 `GET {console}/api/v2/config` 发一次请求），
结果三态：

| 结果 | 触发条件 | 是否落盘 | 界面回显 |
|---|---|---|---|
| `verified` | `200` 且解析出账号级目录 | 落盘 + 立即激活 | 「已链接：密钥已保存并验证通过」；目录 `source: account` |
| `unverified` | 超时 / 网络不可达 / 5xx / 畸形响应 / `200` 但解析不出 go 模型 | **仍然落盘** | 「密钥已保存，但未校验：上游暂时无法确认，首次使用时才会暴露问题」；目录退到 live ∩ overlay，**绝不编造** |
| `rejected` | `401` / `403` | **不落盘、不激活** | 「未保存：密钥无效或已被撤销…」/「未保存：该工作区没有 OpenCode Go 订阅」；文案内核自产，**不回显上游正文** |

- **环境变量遮蔽**：若该 provider 的 `api_key_env` 已设置且非空，刚存的 Key **不会生效**，
  面板显示「已保存；环境变量当前优先，链接的密钥暂不生效」。优先级不变：
  `--api-key` flag > `api_key_env`（非空）> `auth.json`。
- **替换语义**：同一 provider 再次链接 = **替换**原凭据；已有 OAuth 凭据时提交前显示
  「将替换现有 OAuth 凭据」。不做多账号。
- **解除链接**：点「解除链接」（TUI `c`）→ `auth/clearCredential`。移除后若仍有 env key，
  则以该 key 重建 provider（仍可用）；无任何 key 时 provider 从注册表移除，
  `auth/status.authenticated` 变 `false`，目录回退 live / overlay；原本没有条目则回显
  「当前没有已保存的凭据」。

#### 用 CLI 写入（脚本 / 无界面场景）

```bash
agentj auth login --api-key <WORKSPACE_API_KEY>          # 写入凭据
agentj auth login --api-key <KEY> --default-model opencode-go/kimi-k3
agentj auth login --offline                              # 跳过联网目录展示
printf '%s\n' "$KEY" | agentj auth login                 # stdin 管道（无 --api-key 时）
agentj auth status                                       # 来源 + 后 4 位 + 模型数
agentj auth logout                                       # 删除凭据
```

### 2. 设备码登录（RFC 8628，OAuth）

```bash
agentj auth login --device          # 与 --api-key 互斥
```

命令会打印一行验证地址（形如 `https://opencode.ai/console/device?user_code=…`）与一个
`user_code`：在**你自己的浏览器**里打开该地址、确认设备码即可。CLI 按上游返回的 `interval`
轮询（默认 5s），收到 `slow_down` 会自动延长等待，收到授权后立刻：

- 把 `{"type":"oauth","access":…,"refresh":…,"expires":…,"org_id":…}` 原子写入
  `auth.json`（**覆盖**该 provider 原有的 api_key 凭据）；
- 打印「已登录 …（已替换原有凭据；org …）」、凭据文件路径与（非 `--offline` 时）模型数。

设备码过期 / 被拒绝 / 上游未识别错误都会以可行动文案失败，且**不写盘**（原有凭据保持不变）。
退出订阅仍然是 `agentj auth logout`。Web 与 TUI 的订阅面板内置同一套流程
（`auth/deviceStart` → 显示码与地址 → 按 `interval` 轮询 → 授权后自动刷新面板），
无需打开终端。

> **密钥优先级**（两种方式共用）：`--api-key` > 该 provider 的 `api_key_env`（非空）
> > `auth.json`。解析不到密钥的**真实 provider 不会注册**（绝不发送空 `Bearer`）；
> 若最终没有任何可用 provider，内核启动直接报 `no usable model provider` 并退出非 0。
> 免密钥的 `mock*` kind 不受影响。

## 凭据生命周期

- **凭据文件**：`$XDG_DATA_HOME/agentj/auth.json`（回退 `$HOME/.local/share/agentj/auth.json`），
  目录 `0700`、文件 `0600`，临时文件 + rename 原子写。
- **权限严格**：既存文件权限不是 `0600`（含任何 group/other 位）时**报错拒绝使用**，
  不会静默覆盖。修复：`chmod 600 ~/.local/share/agentj/auth.json`。
- **只显示后 4 位**：`auth status`、启动日志与订阅面板只展示 api_key 的 `…<末 4 位>`
  （`redact()`），完整密钥 / access token / refresh token **永不出现在协议结果或日志里**。
- **OAuth 以 `org_id` + `expires_at` 标识**（没有可展示的后 4 位）。
- **access token 只有 600s**：内核与 CLI 在每次使用凭据前调用 `ensure_access`，
  剩余寿命不足 60s（或已过期）时用 refresh token 换新，并**就地回写** `auth.json`；
  刷新在 `auth.json.lock` 文件锁内**单飞**，锁内重读避免并发重复刷新。
- **refresh token 单次使用**：上游一旦轮换，旧 token 立即失效（复用会吊销整个会话），
  因此 AgentJ 绝不用旧 refresh token 重放；刷新失败会返回可行动错误
  （`…请运行 agentj auth login --device 重新登录`）而**不会回落成空 Bearer 或旧 token**。
- **长驻内核无需重启**：provider 外层的 `RefreshingProvider` 在每次 `chat` / `chat_stream`
  前保鲜，只有密钥真的变化时才重建内层 provider；`models()` 走本地目录、不触发刷新。
- **离线可用**：凭据读写与 `--offline` 全程零网络请求；联网只用于目录展示与 token 刷新
  （目录失败只 warn，不阻断）。

## 用量与配额

`provider/usage` 返回**三个窗口**（`percent` 是**已用**百分比）：

| 窗口 | 含义 |
|---|---|
| `rolling` | 滚动 5 小时 |
| `weekly` | 本周 |
| `monthly` | 本月 |

每个窗口带 `{kind, status, percent, resets_at}`；`status` 为 `ok` 或 `rate-limited`。
上游 `percent == 0` 时 `resets_at` 是「now + 整窗口」的占位值，会**被省略**而不是显示误导时间。

- **只有已用百分比，没有余额字段**：上游不提供剩余额度 / 金额，AgentJ 不编造。
- **按需拉取 + 进程内缓存 300s**：面板打开才取数，300 秒内重复打开复用缓存；
  `refresh: true`（面板的「刷新用量」）强制绕过缓存重拉。没有常驻轮询任务。
- **预警阈值**：`[model] quota_warn_percent`（int，默认 **80**，越界 clamp 到 `1..=100`）
  随结果下发（`warn_percent`），口径在 CLI / Web / TUI 三处一致。面板在任一窗口
  `status == "rate-limited"` 或 `percent >= warn_percent` 时以警示样式展示。
- **上游撞墙时**：若一轮 run 收到 429 且错误文案含 `GoUsageLimitError`，Web 运行面板会在
  该消息旁给一次性提示「可能是订阅额度受限 → 设置 · 订阅」。

## 模型目录：三层来源

`model/list` 按优先级回退，结果里的 `source` 如实标注实际层级：

1. **`account`（账号级）**：`GET https://opencode.ai/console/api/v2/config`
   （`Bearer <key|access>`；OAuth 凭据额外带 `x-org-id`）。它反映该 workspace
   **真实可选**的模型，因此**工作区被禁用的模型不会出现**——这正是它比匿名列表更可信的原因。
2. **`live`**：`GET https://opencode.ai/zen/go/v1/models`（匿名可读）∩ 提交件 overlay。
3. **`overlay`**：仓库提交件 `models/opencode-go.json` 的路由映射全集（离线兜底）。
4. 非 opencode-go provider：**`static`**，直接用 provider 自己上报的列表，不发任何网络请求。

合并规则：id 命中 overlay 则沿用其 `format` 与 `verified`；未命中（账号级 / live 新出现）
合成为 `openai-compat` + `verified: false`，并在启动日志标注「路由未验证」——
**绝不按名字猜路由**。目录结果有 **60s 复用窗口**，`refresh: true` 强制重拉。

刷新提交件 overlay（需要联网，手动 / CI）：

```bash
cargo run -p agentj-model --bin gen_opencode_catalog            # 重新生成
cargo run -p agentj-model --bin gen_opencode_catalog -- --check # 校验是否过期
```

## 可见性与选模型

- `kernel/status` 的 `providers[].models` 返回启动期快照（零协议变更）；`model/list`
  是**按需**的权威查询（含账号级差异）。
- **Web**：composer 模型下拉与订阅面板消费该列表；**TUI**：模型视图与订阅浮层同理。
- 运行期**可切换模型**（逐轮 `session/run` 自带 provider / 模型，无需重启），
  见 [运行期切模型](model-switching.md)；改**默认**模型仍是
  `agentj auth login --default-model <id>` 定点写回 `config.toml` 的 `[model] provider` /
  `default_model`（保留注释与格式）。
- 凭据状态用 `auth/status` 查询：它是**纯读投影**（不轮换 token、不发网络请求），
  返回 `authenticated`、`credential`（api_key → `key_hint`；oauth → `org_id` /
  `expires_at` / `expiring_soon`）与 `models_count`。

## 配置

```toml
[model]
provider = "opencode-go"
quota_warn_percent = 80                                    # 用量预警阈值，1..=100，默认 80
console_base_url = "https://opencode.ai/console"           # 控制面基址（设备流 + 账号级目录）
```

`console_base_url` 默认即 `https://opencode.ai/console`（旧文档曾把它记作一个 console
子域别名，属于误记）；只有自建代理 / 排障时才需要改。

## 合规请求头

- 所有 provider 请求统一 `User-Agent: agentj/<版本>`。
- opencode-go 的请求在本轮存在会话 ID 时携带 `x-opencode-session: <session_id>`
  （天然「每对话稳定」）。其他 provider **不发送**该头。
- 不做任何客户端身份伪装。

## 排障

| 现象 | 面板 / CLI 文案 | 含义与动作 |
|---|---|---|
| 无凭据 | 尚未登录：请运行 `agentj auth login --device` | 先登录；`auth/status` 会显示 `authenticated: false` |
| 401 | 密钥无效或已被撤销：请重新登录 | 凭据被吊销；重新 `agentj auth login` |
| 403 | 该工作区没有 OpenCode Go 订阅 | 有 key 但无订阅权益（**403 不会被当作 401**） |
| 窗口 `rate-limited` | 该行显示 `已限流` + 警示样式 | 已用满该窗口；等 `resets_at` 或换窗口 |
| 用量不可识别 | 用量暂时不可用（上游响应无法识别） | 上游形状变更；不编造数据，稍后重试 |
| 网络不可达 | 网络不可达，已显示缓存数据 | 结果 `stale: true` + 旧窗口 + `error.kind="network"` |
| run 撞额度 | 可能是订阅额度受限 → 设置 · 订阅 | 上游 `GoUsageLimitError`（非普通 429） |

网关错误包封 `{"type":"error","error":{"type","message"}}` 会被解析为 `类型: 消息`；
429 仍解析 `Retry-After` 并按既有指数退避重试。

## 边界（本版本不做）

- 不做多账号 / 账号切换（同一 provider 再次登录 = 替换原凭据）。
- 不做 OS keyring；不做按量付费 Zen（`/zen/v1`）。
- 不在顶栏 / 状态栏做常驻配额徽标（避免周期轮询或未定义的新鲜度语义）。
- 设备登录的最终授权永远发生在用户自己的浏览器会话中；五个订阅方法**不加额外权限门**
  （沿用既有 WS token / cookie / 回环鉴权边界）。
