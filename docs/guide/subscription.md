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
> > `auth.json`。解析不到密钥的**真实 provider 不会注册**（绝不发送空 `Bearer`），
> 内核照常启动并打印一条 `no usable model provider` 汇总警告；`serve` / `dashboard`
> 随后可在客户端「设置 · 订阅」链接 Key，**当场激活、免重启**。只有一次性 `run`
> （无 IPC 客户端可录入凭据）仍以该文案启动即报错退出非 0。
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

### 历史趋势（落盘取样）

内核把**每次真的向上游取数**的用量记成一条样本，`provider/usageHistory` 把这些样本读回来：

- **记什么**：一条样本 = `{ at, windows }`，即该次上游返回的三个窗口（`rolling` / `weekly` /
  `monthly`）的百分比快照。查询支持按 `provider` / `kind` / `since` / `limit` 过滤
  （`limit` 上限 5000，只保留最靠后的一段）；回复里的 `retention_days` 是**本次实际生效**的
  保留天数，可直接拿来标注横轴。查询是纯本地读，不触网、不需要凭据。
- **只在真取数时记**：300s 缓存命中、`stale: true` 的回落数据、`no_credential` / 鉴权失败 /
  网络不可达都不会产生样本——趋势画的是上游用量，不是「谁点了刷新」。
- **落在哪**：一个 provider 一个文件（`<provider>.json`），目录按 `[web] state_dir` →
  `$XDG_DATA_HOME/agentj/usage` → `$HOME/.local/share/agentj/usage` 解析；目录 `0700`、文件
  `0600`，临时文件 + rename 原子替换。写历史失败只记一条警告，不影响当次用量查询。
- **去重与裁剪**：60 秒内且三个窗口的值完全没变视为重复，不落新样本；超出保留期或条数上限的
  旧样本自动丢弃。
- **看得到的地方**：Web 订阅面板每个窗口行下方一条迷你曲线；TUI 订阅浮层在窗口行里追加块字符
  曲线与首尾值（形如 `10%→30%`）。不足两条样本时**不画空图**——Web 显示「趋势需要至少两次
  取样」、TUI 直接省略该行，刚链接订阅时看不到趋势是正常的。Web 的曲线既可以鼠标逐点悬停
  （提示该点的百分比与本地时刻），也可以键盘操作：聚焦后默认停在新样本，`←` / `→` 逐点移动、
  `Home` / `End` 跳到首尾、`Esc` 或失焦收起，读出的值同时写进无障碍实时区域，不依赖鼠标。
- **关掉它**：`[usage] history_enabled = false` 后内核**不再建目录、不写任何样本**，
  `provider/usageHistory` 直接返回空数组（用量本身照常）。三个目录来源都解析不出来时，历史
  **整体禁用**（没有内存兜底，也不会在重启后凭空出现），内核启动会打印
  `warning: no usage history directory (set XDG_DATA_HOME, HOME or [web] state_dir);
  usage history is disabled until one exists`。
- **并发写是「后写者胜」**：同一目录下两个进程（例如不同端口的两个 `serve`）同时落盘时，
  后一次 `rename` 会覆盖前一次刚记的样本——丢一条取样而不是写坏文件；下一次真取数会补回来。

### 常驻状态指示灯

Web 顶栏与 TUI 状态行各有一枚常驻用量指示灯，读数一律取**最差的那个窗口**：

- **Web（顶栏）**：一个订阅灯，悬停或聚焦展开三行窗口；最差窗口 `ok` / `warn`（≥70%）/
  `error`（≥90%）三档分级。没有链接凭据时显示「未链接凭据」并给出「设置 → 订阅」入口；
  凭据已失效/被撤销记为 `invalid`（提醒重新订阅），与「用量读取失败」的 `error` 区分开——
  密钥坏了和上游取不到数是两回事。
- **TUI（状态行）**：连接状态后追加一段 `· sub 12%/40%`（按窗口顺序列出百分比，`~`
  表示数据已过期，最差窗口 ≥70% 转警示色、≥90% 转错误色）。没有凭据时不加后缀，
  状态行保持原样。

新鲜度契约（两端一致，指示灯**不引入**周期轮询）：

- **没有周期轮询**：不设定时器、不在后台刷数；Web 在挂载时、TUI 在连接打开时读一次，
  其余只靠用户显式刷新。
- **年龄始终可见**：过期数据带 `~`，Web 的悬停面板同时给出「N 分钟前」的年龄，
  读数不会假装是当前值。
- **焦点只补一次**：Web 窗口重新获得焦点时，仅当上次成功读取已超过 300s 才重取，
  300s 内不重复请求。

## 模型目录：三层来源

`model/list` 按优先级回退，结果里的 `source` 如实标注实际层级：

1. **`account`（账号级）**：`GET https://opencode.ai/console/api/v2/config`
   （`Bearer <key|access>`；OAuth 凭据额外带 `x-org-id`）。它反映该 workspace
   **真实可选**的模型，因此**工作区被禁用的模型不会出现**——这正是它比匿名列表更可信的原因。
2. **`live`**：`GET https://opencode.ai/zen/go/v1/models`（匿名可读）∩ 生成本地快照。
3. **`overlay`**：`crates/agentj-model/data/opencode-go.json` 的路由映射全集（离线兜底）。
4. 非 opencode-go provider：**`static`**，直接用 provider 自己上报的列表，不发任何网络请求。

合并规则：id 命中快照则沿用其 `format` 与 `verified`；未命中（账号级 / live 新出现）
合成为 `openai-compat` + `verified: false`，并在启动日志标注「路由未验证」——
**绝不按名字猜路由**。目录结果有 **60s 复用窗口**，`refresh: true` 强制重拉。

### 路由映射从哪来

`/v1/models` 只返回 id（无元数据），因此 `format` 取自 **models.dev**（`https://models.dev/api.json`）
——它是官方[端点表](https://opencode.ai/docs/go/#api-endpoints)「AI SDK 包」一列的机器可读镜像：
模型级 `provider.npm` 覆盖优先，否则继承 provider 级 `npm`（opencode-go 默认
`@ai-sdk/openai-compatible` ⇒ `/v1/chat/completions`）。models.dev 未收录的 id 不猜路由，
一律 `openai-compat` + `verified: false`。

生成器把两个来源烘焙成提交件（**产物，不要手改**）：

```bash
cargo run -p agentj-model --bin gen_opencode_catalog            # 重新生成（需联网）
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

用量历史的开关与保留期在 `[usage]` 段：

```toml
[usage]
history_enabled = true        # false = 不建目录、不落样本，provider/usageHistory 回空
history_days = 30             # 保留天数，钳制 1..=365
history_max_samples = 8640    # 每个 provider 的样本上限，钳制 100..=100_000
```

三项都有默认值，不写即上面这三行；越界值夹到区间内而不报错。取值含义见上面
[历史趋势](#历史趋势落盘取样)。

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
- 设备登录的最终授权永远发生在用户自己的浏览器会话中；五个订阅方法**不加额外权限门**
  （沿用既有 WS token / cookie / 回环鉴权边界）。
