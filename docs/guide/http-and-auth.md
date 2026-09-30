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

## 资产完整性

构建 Web UI 时生成 `dist/asset-manifest.json`（`sha384` 覆盖全部 bundle 产物含动态
chunk + 重写后 `index.html` + dist 实目录其余部署文件，含 `public/` 透传）。内核启动
逐文件校验（缺失/不符仅 stderr 警告，不阻断服务），并每 60s 游标轮转抽检 8 个文件
（不符 `warn!` 按路径去重）。
