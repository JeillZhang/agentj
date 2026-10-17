# 配置（`config.toml`）

AgentJ 使用 TOML 配置，包含 `[model]` / `[agent]` / `[permission]` / `[session]` /
`[license]` / `[[mcp.servers]]` 等节。

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

[permission]
[permission.tools.bash]
mode = "ask"

[[permission.tools.bash.command_rules]]
prefix = "rm -rf"
effect = "deny"

[session]
dir = "/data/agentj/sessions"

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
- `max_context_tokens`：单次请求的近似 token 上限（默认 `128000`，钳制到
  `1000..=10000000`）。发送前内核把会话历史与工具结果裁剪到该预算内（保留 system 与最新
  消息；单条超长正文保留头尾并标注截断）。上游若仍报上下文超限，则按上游给出的窗口收紧
  预算重试（最多 2 次），因此长会话不会再把 400 `context_length_exceeded` 直接抛给用户。
- `[[model.providers]]`：模型提供方列表。
  - `kind` 取值：`openai-compat`（OpenAI 兼容 SSE）/ `responses`（OpenAI Responses）/
    `anthropic`（Messages）/ `opencode-go`。另有内置 `Mock` 与 `Ollama`。
  - `api_key_env`：从环境变量读取 API Key。

## `[agent]`

- `max_tool_iterations`：单轮对话允许的工具调用轮数（默认 `50`，钳制到 `1..=1000`）。
  用尽后内核**不再把工具交给模型**，而是再要一趟「收尾」：要求它基于已经查到的东西直接
  给出结论，并把结论作为本轮答复。所以长任务不会在轮数用完时丢掉中间成果、只留下一句
  「让我再看看…」或一条内部错误；若收尾那一趟也拿不回来内容，才会如实报错。

## `[permission]`

- 全局与逐工具 `mode`（如 `ask`）。
- `timeout_ms`：`ask` 审批的自动拒绝等待预算（毫秒；省略 = 60s 默认）。
- `[[permission.tools.<tool>.command_rules]]`：按命令前缀设置 `effect`
  （`deny` / `allow` 等）。权限决策引擎含超时自动 Deny 与 `allow_always`
  会话级记忆；取消即拒绝。

## `[session]`

- `dir`：文件持久化会话目录（`FileBackedStore`），支撑跨进程/跨 UI 会话恢复。

## `[license]`

- `key`：Ed25519 许可证（`base64(payload).base64(sig)`）。
- 亦可用环境变量 `AGENTJ_LICENSE_KEY` 覆盖；空字符串忽略。
- `run` / `serve` / `dashboard` 按 entitlement 门控。

## `[[mcp.servers]]`

- `name` / `command` / `args` / `env`：MCP 服务器规格；CLI 启动时连接，
  `McpConnectionManager` 长驻连接并在退出时 `close_all`。
