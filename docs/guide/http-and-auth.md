# HTTP 面与鉴权（内核托管）

内核在 `--port` 端口上同时提供 WebSocket `/ws` 与一组 HTTP 路由，用于托管内置 Web UI
及可观测性探测。

## 路由

| 路由 | 说明 |
| --- | --- |
| `/` | Web UI（`no-cache`，SPA fallback：未知页面 200） |
| `/assets/*` | 静态资源（`Cache-Control: public, max-age=31536000, immutable`） |
| `/ws` | WebSocket 网关 |
| `/login` / `/logout` | 登录页 / 登出（会话 cookie） |
| `GET /version` | 版本事实（未鉴权，`Cache-Control: no-store`） |
| `GET /metrics` | Prometheus 指标（鉴权门控，`text/plain; version=0.0.4`，`no-store`） |

`/ws`、`/login`、`/logout`、`/version`、`/metrics` 均为显式路由，优先于 SPA fallback。

## 鉴权

- **回环 peer** 或**未配置 token** → 直出。
- **非回环 + 已配 token**，无有效 cookie → `302 /login?next=<同源相对路径>`
  （`next` 只放行 `/` 开头且非 `//`，防开放重定向）。
- 登录页 POST 校验后颁发不透明会话 cookie
  （`agentj_session=<id>; HttpOnly; SameSite=Lax; Path=/`，内存态，重启即失效）；
  `/logout` 撤销。

### WebSocket 三通道

1. `?token=<TOKEN>` 查询参数；
2. `initialize.token` 请求字段；
3. 有效会话 cookie + 同源 `Origin`（免 token）。

## 登录限流

非回环登录失败按来源 IP 计数：第 5 次失败起锁定 30s、×2、封顶 900s；
`429 + Retry-After`（秒）；锁定期不校验 token 且新尝试升级锁定；成功即清零。
`RUST_LOG=debug` 可查看逐次明细。

## `/version`

未鉴权 JSON，含 `product` / `protocol` / `build_time` 等 dist 版本事实，以及：

- `login_limiter`：聚合计数（`attempts` / `failures` / `lockouts` / `blocked` /
  `locked_now` / `tracked_ips`，无 per-IP 明细）。
- `integrity`：资产清单摘要（`manifest_present` / `checked` / `mismatched` / `missing`）
  与运行期抽检 `runtime { rounds, checked, mismatched, missing, last_run_age_secs? }`。

## `/metrics`

Prometheus 文本。回环 / 未配 token / 有效 cookie → 直出 200；远程无 cookie → `401`
（不重定向）。含聚合计数/gauges + per-IP 序列（Top-50 截断 +
`agentj_login_ip_series_truncated` 指示）+ 清单摘要。

## 反向代理

内核默认只信任**直接 TCP peer**。要让 `/metrics` 与登录限流看到经代理转发的真实客户端
IP，须在内核侧显式开启代理头信任：

```toml
[web]
trust_proxy_headers = true          # 默认 false；仅当直接 peer 是回环时生效
state_dir = "/var/lib/agentj"       # 可选；默认 $XDG_DATA_HOME/agentj
```

代理须把客户端 IP 写进请求头（nginx）：

```nginx
location / {
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_pass http://127.0.0.1:4096;
}
```

- **信任根是回环 peer**：只有发起连接的直接 peer 是回环（本机代理）时才采信
  `X-Forwarded-For` / `X-Real-IP`；否则一律退回真实 peer。`X-Forwarded-For` 从右向左取
  第一个非回环合法项（回环跳数视为代理自身，不是客户端）。
- **红线：派生 IP 只用于识别**——限流键、per-IP 序列标签与日志字段；**永不参与鉴权**。
  `auth_required` 的回环免鉴权判据始终只吃真实 TCP peer，请求头无法用它换取免登录。
- **多实例聚合**：限流状态按进程持有，状态文件按端口分片
  （`$XDG_DATA_HOME/agentj/metrics-state-<port>.json`，目录 `0700` / 文件 `0600`）。多实例
  部署时不要假设单实例聚合，per-IP 序列请在 Prometheus 侧用 `sum by (ip)` 聚合；
  `agentj_web_metrics_state_*` 指标反映各实例自身的落盘状态。
- 状态目录可用 `[web] state_dir` 覆盖；不可用时启动只警告、不落盘，服务照常。

## 资产完整性

构建 Web UI 时生成 `dist/asset-manifest.json`（`sha384` 覆盖全部 bundle 产物含动态
chunk + 重写后 `index.html` + dist 实目录其余部署文件，含 `public/` 透传）。内核启动
逐文件校验（缺失/不符仅 stderr 警告，不阻断服务），并每 60s 游标轮转抽检 8 个文件
（不符 `warn!` 按路径去重）。
