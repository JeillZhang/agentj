# 配置（`config.toml`）

AgentJ 使用 TOML 配置，包含 `[model]` / `[permission]` / `[session]` / `[license]` /
`[[mcp.servers]]` 等节。

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
- `[[model.providers]]`：模型提供方列表。
  - `kind` 取值：`openai-compat`（OpenAI 兼容 SSE）/ `responses`（OpenAI Responses）/
    `anthropic`（Messages）/ `opencode-go`。另有内置 `Mock` 与 `Ollama`。
  - `api_key_env`：从环境变量读取 API Key。

## `[permission]`

- 全局与逐工具 `mode`（如 `ask`）。
- `[[permission.tools.<tool>.command_rules]]`：按命令前缀设置 `effect`
  （`deny` / `allow` 等）。权限决策引擎含 60s 超时自动 Deny 与 `allow_always`
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
