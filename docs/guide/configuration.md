# 配置（`config.toml`）

AgentJ 使用 TOML 配置，包含 `[model]` / `[agent]` / `[permission]` / `[session]` /
`[license]` / `[usage]` / `[[mcp.servers]]` 等节。

## 示例

```toml
[model]
provider = "deepseek"
default_model = "deepseek-chat"

[[model.providers]]
name = "deepseek-chat"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
kind = "openai-compat"   # openai-compat | responses | anthropic | opencode-go

[agent]
max_tool_iterations = 50
max_tool_result_chars = 100000
question_timeout_s = 600

[permission]
[permission.tools.bash]
mode = "ask"

[[permission.tools.bash.command_rules]]
prefix = "rm -rf"
effect = "deny"

[session]
dir = "/data/agentj/sessions"

[usage]
history_enabled = true     # 用量历史（provider/usageHistory）默认开
history_days = 30          # 保留天数，1..=365
history_max_samples = 8640 # 每个 provider 的样本上限，100..=100000

[license]
key = "base64(payload).base64(sig)"

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
env = { FOO = "bar" }
```

## `[model]`

- `provider` / `default_model`：默认模型。
- `max_context_tokens`：单次请求的**显式** token 上限（**可缺省**，钳制到
  `1000..=10000000`）。不写时不强行用某个固定值：生效预算是逐请求算出来的
  ——`min(显式上限, 模型上下文窗口 × 90%)`，只有两者都未知才回 `128000`。例：模型窗口 200k
  → 实际预算 **180k**；显式给 50k 就还是 50k；上限 50k 遇上 20k 窗口 → **18k**。窗口由 provider
  上报（opencode-go 来自目录快照，与 `model/list` 的 `context_window` 同源；未知时整键省略、
  不猜），内核逐轮用 `ProviderRegistry::context_window` 取值。发送前内核把会话历史与工具结果
  裁剪到该预算内（保留 system 与最新消息；单条超长正文保留头尾并标注截断）。上游若仍报上下文
  超限，则按上游给出的窗口收紧预算重试（最多 2 次），因此长会话不会再把 400
  `context_length_exceeded` 直接抛给用户。
- `[[model.providers]]`：模型提供方列表。
  - `kind` 取值：`openai-compat`（OpenAI 兼容 SSE）/ `responses`（OpenAI Responses）/
    `anthropic`（Messages）/ `opencode-go`。另有内置 `Mock` 与 `Ollama`。
  - `api_key_env`：从环境变量读取 API Key。

## `[agent]`

- `max_tool_iterations`：单轮对话允许的工具调用轮数（默认 `50`，钳制到 `1..=1000`）。
  用尽后内核**不再把工具交给模型**，而是再要一趟「收尾」：要求它基于已经查到的东西直接
  给出结论，并把结论作为本轮答复。所以长任务不会在轮数用完时丢掉中间成果、只留下一句
  「让我再看看…」或一条内部错误；若收尾那一趟也拿不回来内容，才会如实报错。
- `max_tool_result_chars`：单个工具结果的字符上限（默认 `100000`，钳制到 `1000..=1000000`）。
  超过时内核在工具结果的**唯一转换点**只截断一次——保留开头 `max_tool_result_chars` 个字符
  （按 UTF-8 边界切分）并追加一条提示（丢弃字符数 + 原文长度，请模型缩小查询范围）；事件、
  回注与持久化历史看到的都是这份截断后的文本。
- `question_timeout_s`：`ask` 提问的默认超时秒数（默认 `600`，钳制到 `1..=86400`）。提问未
  显式给 `timeout_s` 时用它；超时无人回答则该提问按「已跳过」结算，不再无限等待（显式
  `timeout_s` 优先；客户端未接入提问通道时问题立即结算为跳过）。

## `[permission]`

- 全局与逐工具 `mode`（如 `ask`）。
- `timeout_ms`：`ask` 审批的自动拒绝等待预算（毫秒；默认 `60000`）。越界值钳制到
  `100..=600000` 并在 stderr 打印一行
  `warning: permission.timeout_ms <原始值> clamped to <钳制值>`——负值与 `0` 同样按钳制处理
  （警告里回显的是**原始值**，不会被归零），不视为配置错误。取值须落在 64 位有符号整数范围内；
  超出该范围的 TOML 整数字面量在解析阶段即报错（与其它整数键一致）。
- `[[permission.tools.<tool>.command_rules]]`：按命令前缀设置 `effect`
  （`deny` / `allow` 等）。权限决策引擎含超时自动 Deny 与 `allow_always`
  会话级记忆；取消即拒绝。
- 同一条审批只认**第一次**答复：重复或迟到的答复既不会覆盖已生效的决定，也不会把工具
  判成「查无此单」（工具只会报告真实原因：`timeout` / `cancelled` / `disconnected`）。
  断线重连的客户端只要重新 `session/subscribe`（Web/TUI 打开会话时自动做）就继续持有
  该会话的审批权。

## `[session]`

- `dir`：文件持久化会话目录（`FileBackedStore`），支撑跨进程/跨 UI 会话恢复。

## `[license]`

- `key`：Ed25519 许可证（`base64(payload).base64(sig)`）。
- 亦可用环境变量 `AGENTJ_LICENSE_KEY` 覆盖；空字符串忽略。
- `run` / `serve` / `dashboard` 按 entitlement 门控。

## `[usage]`

用量历史的开关与保留期（详见 [opencode-go 订阅](subscription.md) 的「历史趋势」一节）：

- `history_enabled`：是否落盘用量样本（默认 `true`）。`false` 时内核不建目录、不写样本，
  `provider/usageHistory` 直接回空数组。
- `history_days`：保留天数（默认 `30`，钳制到 `1..=365`）。
- `history_max_samples`：每个 provider 的样本上限（默认 `8640`，钳制到 `100..=100000`）。

目录不由这里指定：它在 `[web] state_dir` → `$XDG_DATA_HOME/agentj/usage` →
`$HOME/.local/share/agentj/usage` 中选择，与指标快照同一条链，只是多一层 `usage/`。

## `[[mcp.servers]]`

- `name` / `command` / `args` / `env`：MCP 服务器规格；CLI 启动时连接，
  `McpConnectionManager` 长驻连接并在退出时 `close_all`。
