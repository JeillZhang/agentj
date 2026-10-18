# 快速上手

## 环境要求

- Rust 1.96+（stable，实测 1.97.1）
- Node 24+（`agentj-ui/apps/web` + `agentj-ui/packages/protocol` +
  `agentj-ui/packages/sdk` 构建与测试需要）
- Bun ≥1.2（`agentj-ui/apps/tui` dev/build/test 需要）
- Rust 工具链（`agentj-ui/apps/desktop` Tauri 壳构建需要）
- DeepSeek API Key（集成测试与手动验证用）

## 构建

```bash
cargo build --release                       # 构建 agentj 二进制
cargo build --workspace                     # 或全工作区
```

## 启动内核

内核是所有端的公共接入点，一条命令即可常驻：

```bash
agentj serve --port 4096                    # 本机（回环，默认信任）
agentj dashboard --port 4096                # 同款协议入口（无 token 最简）
```

启动后：

- **Web UI（内核托管）**：浏览器打开 `http://127.0.0.1:4096/`（回环免登录）。
- **任意协议消费者**：连接 `ws://127.0.0.1:4096/ws` 或 `tcp://127.0.0.1:4096`。

## 局域网 / 远程访问

```bash
agentj serve --port 4096 --host 0.0.0.0 --token SECRET
```

浏览器打开 `http://<host>:4096/` → 内嵌登录页输入 token → 会话 cookie 生效，UI 与
WebSocket 同源可用。详见 [HTTP 面与鉴权](http-and-auth.md)。

用 dev server（`localhost:5173`）连远程内核时，在 **设置 → 连接** 填 `Host` / `Port`
与 `Token`；地址派生规则见 [Web 工作台](web.md)。

## 运行一次任务

```bash
agentj run "explain this codebase"          # 单轮
agentj run --attach tcp://127.0.0.1:4096 "hi"  # 附着到常驻内核
```

## 打开各端（开发）

```bash
# Web（dev server 按页面地址派生内核地址 <hostname>:4096；内核在 4096 时无需配置）
npm --prefix agentj-ui install
npm --prefix agentj-ui run dev --workspace=@agentj/web

# TUI
bun --cwd agentj-ui/apps/tui install
bun --cwd agentj-ui/apps/tui run dev
```

## 内置工具

内核自带 6 个工具：`bash`、`glob`、`grep`、`read`、`write`、`todo`。除 `bash` 外都不需要外部
命令（不装 `find` / `rg` 也能用）：`grep` 优先调用 `rg`（命令行体验一致），机器上没有
ripgrep 时自动回退到进程内检索（按行匹配、支持 `include` 通配、跳过 `.git` / `target` /
`node_modules`、跳过二进制与超大文件、命中上限 2000 条）。

## 下一步

- 命令行细节 → [CLI](cli.md)
- 界面功能 → [Web 工作台](web.md) / [TUI](tui.md) / [Desktop](desktop.md)
- 模型与权限配置 → [配置](configuration.md)
