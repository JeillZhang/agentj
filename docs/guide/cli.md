# 命令行（CLI）

二进制 `agentj` 提供三个子命令：`run` / `serve` / `dashboard`。

## `agentj run`

运行一次（或附着）代理任务。默认使用内核默认 provider 与 `[model].default_model`；
`--provider` / `--model` 只对**本次运行**生效，不写入配置、不影响后续运行。

```bash
agentj run "explain this codebase"
agentj run --provider deepseek --model deepseek-chat "explain this codebase"
agentj run --model opencode-go/kimi-k3 "hi"                            # 仅覆盖模型，provider 用默认
agentj run --attach tcp://127.0.0.1:4096 --provider opencode-go "hi"   # 附着到常驻内核
```

| 参数 | 说明 |
| --- | --- |
| `--provider <NAME>` | 仅本次运行使用的 provider 名（默认：内核默认 provider） |
| `--model <MODEL_ID>` | 仅本次运行使用的模型 id，如 `opencode-go/kimi-k3`（默认：`[model].default_model`） |

- 两个 flag **相互独立**：只给 `--model` 合法（用默认 provider 跑指定模型）；`serve` /
  `dashboard` **不接受**这两个 flag（常驻内核的覆盖由各客户端在 `session/run` 中携带）。
- **空白值等同缺省**：`--provider ""` / `--provider "   "`（`--model` 同理）都按「未提供」处理，
  由内核回落到默认值，不是错误。
- **未知 provider 不回落**：`--provider nope` 会让本轮运行失败（不会改用默认 provider）。CLI 在
  stderr 打印一行 `error: agent error: unknown provider: nope (registered: …)` 并以退出码 `1`
  结束，stdout 不会出现最终回答；`--attach` 的远端错误同样映射为这一行。
- 未知模型 / 上游网络或鉴权失败同样以 `error: …` + 退出码 `1` 结束。

## `agentj serve`

启动常驻内核。`serve`（无 `--port`）为 stdio 单客户端；`serve --port` 为多客户端常驻
TCP 内核，并在同端口提供 WebSocket `/ws`。

```bash
agentj serve                                     # stdio 单客户端
agentj serve --port 4096                          # 常驻多客户端（本机）
agentj serve --port 4096 --host 0.0.0.0 --token SECRET   # 局域网/远程 + 鉴权
agentj serve --port 4096 --web-dist path/to/dist  # 显式指定 Web UI 产物
```

| 参数 | 说明 |
| --- | --- |
| `--port <PORT>` | 常驻多客户端端口；同端口提供 `/ws` 与内核托管 HTTP |
| `--host <HOST>` | 绑定地址。默认 `127.0.0.1`（本机信任）；`0.0.0.0` 支持局域网/远程直连 |
| `--token <TOKEN>` | 访问令牌（`?token=` / `initialize.token` / 登录 cookie 通道） |
| `--web-dist <PATH>` | 内核托管的 Web UI 产物目录（见下「四级发现」） |

**Web UI 产物四级发现**：`--web-dist` → 环境变量 `AGENTJ_WEB_DIST` → cwd
`agentj-ui/apps/web/dist` → 可执行文件相对两档。显式 flag/env 无效即停并告警；
缺失则静态 UI 停用、仅 `/ws` 可用。启动时打印 `http://<host>:<port>/`。

启动日志会打印 dist 版本警告（`dist/version.json` 缺失/不可解析/与内核对不一致），
不阻断服务。

## `agentj dashboard`

同款协议入口（最简，无 token 亦可）。补齐 `--port` / `--host` / `--token` /
`--web-dist`。

```bash
agentj dashboard --port 4096
agentj dashboard --port 4096 --token SECRET
```

## 可观测性

```bash
curl http://127.0.0.1:4096/version   # 版本事实 + login_limiter + integrity 摘要
curl http://127.0.0.1:4096/metrics   # Prometheus 文本（鉴权门控）
RUST_LOG=debug agentj serve --port 4096   # 限流逐次明细（默认 info）
```

详见 [HTTP 面与鉴权](http-and-auth.md)。
