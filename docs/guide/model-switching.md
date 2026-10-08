# 运行期切模型

AgentJ 允许在**运行期**逐轮选择模型：不用改 `config.toml`、不用重启内核，选中后**下一轮**
`session/run` 就用该 provider / 模型。

可选模型来自内核真实注册的 provider 列表（`kernel/status` 的 `providers[].models`），
与 [opencode-go 订阅](subscription.md) 中「模型目录」一节描述的是同一份数据。

## 在哪里选

### Web 工作台

- composer（输入区）的**模型选择器**：点「模型」chip 打开弹层（标题「选择模型」），
  列表为内核真实模型目录，逐条保留**所属 provider**；同名模型出现在两个 provider 下时按
  provider 区分（不存在「按列表位置猜 provider」的情况）。
- 选中即成为本页当前选择，chip 标签显示所选模型（不再被内核默认值覆盖）。
- 详见 [Web 工作台](web.md) 的 Composer 一节。

### TUI

- `/models` 打开模型选择器：`j` / `k`（或 `↑` / `↓`）移动高亮，`Enter` 确认高亮行，
  `Esc` / 关闭返回。`▶` 标注的是**当前高亮行**——未移动时停留在当前选择（从未选过时为
  内核默认模型），上下移动后随光标走；只有 `Enter` 确认的条目才会成为本页的选择。
- 详见 [终端界面（TUI）](tui.md)。

### CLI

- `agentj run --provider <NAME> --model <MODEL_ID> "…"` 是同一 per-run 覆盖的命令行形式：
  只对本次运行生效，不写入配置、不进入会话。
- 两者相互独立：可只给 `--model`（用默认 provider）；`--attach` 附着到常驻内核时同样支持。
- 语义与 Web / TUI 完全一致：未知 provider 名 → 本轮运行失败（文案含该名与已注册列表），
  **不回落默认**；空白 / 缺省按「没给」处理。详见 [`agentj run`](cli.md#agentj-run)。

## 生效范围：下一轮 run

- 选择**按 run 生效**：确认后再发送的那一轮 `session/run` 才携带所选 provider / 模型；
  已经在跑的那一轮不受影响。
- 选择**不会写进会话**：会话快照没有模型字段，`SessionRunResult`、历史消息、
  用量统计都不带「本轮用的哪个模型」；同一会话的后续轮次沿用你当前的选择，
  直到你换一个或客户端重启（TUI）。
- 换一个模型 = 下一轮直接换，**不需要**重启内核、也不需要重新建会话。

## 没选 = 内核默认

- 从未选过（刚打开的客户端、脚本直连协议等）时，客户端**不发送**任何新字段，
  `session/run` 的参数与旧客户端逐字相同，内核按默认路径运行：内核默认 provider
  （`get_default()`，注册顺序中的第一个可用者）+ `config.model.default_model`
  （见 [配置](configuration.md) 的 `[model]` 段）。
- 因此「不选」永远不会因为本特性改变行为：老客户端、脚本直连协议的调用方都保持原语义。
- 协议允许**部分覆盖**：只给 provider 不给模型时，模型仍取配置默认；只给模型不给 provider 时，
  provider 仍取内核默认（两端的图形选择器通常两个都带）。

## 未知 provider：直接失败，不静默降级

- 若客户端发来的 provider 名**未注册**，内核**不会**回落到默认 provider，而是让本轮运行失败，
  错误文案包含该名字与**已注册 provider 列表**，例如：

  ```
  agent error: unknown provider: nope (registered: deepseek, opencode-go)
  ```

  修正方式：从选择器（或 `kernel/status`）里挑一个真实注册名。
- 字段类型非法（例如把数字、对象传给 `provider` / `model`）→ JSON-RPC `-32602 invalid_params`，
  `error.data.field` 指明是哪个字段——这是客户端 bug 的硬信号，内核不做「猜你想选哪个」的容错。
- 模型不属于所选 provider → 沿用既有未知模型错误路径（openai 兼容网关的 4xx 会原样透出）。
- 空白与缺省：`null`、空串、纯空白都视作「没给」，行为同「不选」。

## 选择存在哪儿

| 端 | 保存在哪 | 生命周期 |
| --- | --- | --- |
| Web | 浏览器本地存储 `agentj.composer.model`（与 composer 的 Agent / 熵变偏好同一组键） | 仅当前浏览器；其他标签页 / 设备各自选择 |
| TUI | 内存（当前 TUI 进程） | 退出 TUI 即丢失，下次启动回到内核默认 |

- 选择**只存在于客户端本地**，内核不复制这份状态；另一个客户端（或 `run --attach`）接同一会话时
  各自决定本轮用哪个模型，互不影响。
- Web 会把选择写入本地存储，但**真正决定本轮模型的始终是当前页面的选择**——
  重新打开页面后，请在弹层里再确认一次再发送下一轮。
- 本特性**不是**「会话级模型」：不存在「会话值 / 配置默认 / 请求值」三级优先，
  也不需要在会话里清除什么残留状态。

## 协议侧（选读）

`session/run` 的两个可选参数：

| 字段 | 说明 |
| --- | --- |
| `provider` | 已注册 provider 名（= `kernel/status.providers[].id`，如 `opencode-go`） |
| `model` | 模型 id（如 `opencode-go/kimi-k3`） |

- 省略 / `null` / 空串或纯空白 = 缺省；非字符串类型 → `-32602` + `error.data.field`。
- 未知 provider 名 → 本轮运行失败（文案含该名与已注册列表），**不回落默认**。
- 字段语义、错误码与示例见 [协议规范 §8.7](../specs/ipc-protocol.md)。
