# opencode-go 订阅接入

AgentJ 把 [OpenCode Go](https://opencode.ai/docs/go/) 订阅作为一等 provider：登录一次、
凭据落盘，内核启动时发现订阅里的真实模型列表，Web / TUI 直接展示并高亮默认模型，
请求自带厂商要求的客户端标识头。

> 一期覆盖：凭据生命周期、动态目录、合规请求头、模型可见性。
> 二期才提供用量面板与 device-code OAuth（见文末「二期边界」）。

## 登录与凭据

```bash
agentj auth login --api-key <WORKSPACE_API_KEY>          # 写入凭据
agentj auth login --api-key <KEY> --default-model opencode-go/kimi-k3
agentj auth login --offline                              # 跳过联网目录展示
printf '%s\n' "$KEY" | agentj auth login                 # stdin 管道（无 --api-key 时）
agentj auth status                                       # 来源 + 后 4 位 + 模型数
agentj auth logout                                       # 删除凭据
```

- **凭据文件**：`$XDG_DATA_HOME/agentj/auth.json`（回退 `$HOME/.local/share/agentj/auth.json`），
  目录 `0700`、文件 `0600`，临时文件 + rename 原子写。
- **权限严格**：既存文件权限不是 `0600`（含任何 group/other 位）时**报错拒绝使用**，
  不会静默覆盖。修复：`chmod 600 ~/.local/share/agentj/auth.json`。
- **只显示后 4 位**：`auth status` 与启动日志只打印 `…<末 4 位>`，永不打印完整密钥。
- **离线可用**：`auth login` 永远离线可用；`--offline` 全程零网络请求也照常写盘。
  联网仅用于展示目录（失败只 warn，不阻断）。

## 密钥优先级

按以下顺序解析，取第一个可用者：

1. `--api-key`（命令行）
2. 该 provider 的 `api_key_env`（环境变量已设置且非空）
3. `auth.json` 中对应 provider 的条目

解析不到密钥的**真实 provider 不会注册**（不再发送空 `Bearer`）；若最终没有任何可用
provider，内核启动直接报 `no usable model provider` 并退出非 0。免密钥的 `mock*`
kind 不受影响。

## 模型目录 = live 发现 + 提交件 overlay

- **live**：`GET https://opencode.ai/zen/go/v1/models`（匿名可读，5s 超时）给出订阅里
  当前可用的模型 ID。
- **overlay**：仓库提交件 `models/opencode-go.json` 提供 `id → wire format`
  路由映射（live 不返回格式信息）。
- **合并规则**：live 成功 → 以 live 为准，overlay 中有映射的沿用其格式；live 中新出现、
  overlay 尚未收录的 ID 兜底按 `openai-compat` 尝试，并在启动日志标注「路由未验证」。
  live 失败（离线/超时）→ 回退 overlay 全集，仅 warn，不阻断启动。
- **刷新 overlay**（需要联网，手动/CI）：

  ```bash
  cargo run -p agentj-model --bin gen_opencode_catalog            # 重新生成
  cargo run -p agentj-model --bin gen_opencode_catalog -- --check # 校验是否过期
  ```

## 可见性与选模型

- `kernel/status` 的 `providers[].models` 返回真实模型列表（零协议变更，仅填充既有字段）。
- **Web**：composer 模型下拉消费该列表并高亮 `default_model`；无服务端数据时回退内置单条。
- **TUI**：模型视图消费该列表并以 `▶` 标注默认模型；无数据时保留原回退。
- 运行期**不切换模型**（需改协议，属二期）；选模型的方式是
  `agentj auth login --default-model <id>` 定点写回 `config.toml` 的 `[model] provider` /
  `default_model`（保留注释与格式），重启内核生效。

## 合规请求头

- 所有 provider 请求统一 `User-Agent: agentj/<版本>`。
- opencode-go 的请求在本轮存在会话 ID 时携带 `x-opencode-session: <session_id>`
  （天然「每对话稳定」）。其他 provider **不发送**该头。
- 不做任何客户端身份伪装。

## 错误提示

- 网关错误包封 `{"type":"error","error":{"type","message"}}` 会被解析为
  `类型: 消息`（如 `GoUsageLimitError: monthly limit`）。
- 401 / 403 会附带可行动提示：`…请运行 agentj auth login 重新获取凭据`。
- 429 解析 `Retry-After`（三个 wire format 行为一致）并按既有指数退避重试。

## 二期边界（一期不做）

- 用量 / 配额面板（`GET /zen/go/v1/usage`）。
- device-code OAuth 登录（`console.opencode.ai`，`refresh_token` 轮换）；
  一期凭据文件已预留 `oauth` 类型，读到时会明确报
  `unsupported credential type 'oauth': requires agentj phase 2 or newer`。
- 运行期切换模型（需新协议方法）。
- OS keyring、账号级收敛目录、按量付费 Zen（`/zen/v1`）。
