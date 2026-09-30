# 终端界面（TUI）

`agentj-ui/apps/tui`（Bun + `@opentui/react`）是终端前端，连接
`dashboard --port` / `serve --port` 的端点。

## 运行

```bash
bun --cwd agentj-ui/apps/tui install
bun --cwd agentj-ui/apps/tui run dev      # 直连常驻内核
bun --cwd agentj-ui/apps/tui test         # testRender + mock WS e2e
```

## 功能

- 连接管理（地址 / token / 重连，鉴权错误停止重连）。
- 会话：新建 / 切换 / 继续；只读展示当前模型。
- **Composer**：多行输入、`@` 文件补全、`/` palette。
- 流式渲染（标准帧 transcript，语义色）、审批模态、usage 结算（prompt/completion）。
- 主题切换；优雅退出。
- runview 只读 TODO 区块（`revision` 门控）。

## 视图与键位

侧栏在 **Threads**（会话）与 **Files**（文件）间切换。

| 键 | 作用 |
| --- | --- |
| `f` | 切换侧栏视图（Threads ⇄ Files） |
| `j` / `k` / `↑` / `↓` | 上下移动 |
| `Enter` / `l` / `→` | 展开目录 / 跳转 |
| `h` / `←` | 收起或跳父级 |
| `r` | 刷新 |
| `s` | 进入文件搜索（仅 Files 视图） |

> 焦点模型：单字母快捷键仅在侧栏获得焦点时生效，不会劫持 composer 输入。

## 文件视图与搜索

- 懒加载 `file/list`（depth=1）；stale-reply 代际守卫；collapse 保留子树；
  会话切换清空重取。
- 搜索：按 `s` 进入，**防抖 150ms 服务端查询**
  （`file/list { path: null, depth: 8, query }`，服务端 rank + `total`）；
  旧内核回包无 `total` 时自动降级为 depth-8 本地过滤。
  - 可打印字符过滤（含 `f`/`n`）、`Backspace` 删末字符；
  - `Esc` 非空清空 / 空退出；`Enter` 跳转（祖先展开）并选中；`r` 重建索引；
  - `truncated` 时显示 `· partial`。
- 展开状态按会话有效 `project_root` 持久化（`filetree.json`，未知回退
  `__default__`）。
