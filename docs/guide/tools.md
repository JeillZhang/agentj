# 内置工具

内核自带 6 个工具，由模型在对话中自行调用；是否放行由
[权限配置](configuration.md) 决定。

| 工具 | 作用 | 外部依赖 |
| --- | --- | --- |
| `bash` | 执行 shell 命令 | `sh` |
| `glob` | 按 pattern 查找文件 | 无（进程内实现） |
| `grep` | 按行检索文件内容 | 可选 `rg`（缺失时进程内检索） |
| `read` | 读取文件 | 无 |
| `write` | 写入文件 | 无 |
| `todo` | 提交当前任务清单快照 | 无 |

除 `bash` 外都不需要外部命令：不装 `find` / `rg` 也能用。`grep` 只在机器上存在 ripgrep 时
优先调用它（命令行体验一致），否则回退到进程内检索——按行匹配、支持 `include` 通配、跳过
与 `glob` 相同的目录清单、跳过二进制与超大文件、命中上限 2000 条。

## `glob`

`glob` **完全在进程内执行，不调用 `find`，也不调用任何外部命令**：pattern 由 `globset`
编译、目录由 `walkdir` 遍历，工作区根为本次运行的会话根（无会话上下文时为进程当前目录）。

### pattern 语义

| pattern 形状 | 匹配对象 |
| --- | --- |
| 不含 `/` | 每个文件路径**最后一个分量（basename）**，因此在任意深度都能命中 |
| 含 `/` | 整个**根相对路径**（不带 `./` 前缀） |

含 `/` 的 pattern 以 `literal_separator(true)` 编译：`*` **不跨** `/`，`**` 是唯一可跨分量
的通配符。

| pattern | 匹配 | 不匹配 |
| --- | --- | --- |
| `*.rs` | `src/main.rs`、`src/util/mod.rs` | `src/main.txt` |
| `src/*.rs` | `src/main.rs` | `src/a/b.rs` |
| `src/**/*.rs` | `src/main.rs`、`src/a/b/c.rs` | `tests/x.rs` |

- pattern 两端的空白、前缀 `./` 与前导 `/` 会被忽略（`./src/*.rs` 等价于 `src/*.rs`）。
- pattern 语法非法时报错（`invalid glob pattern: …`），不会静默给出空结果。
- 无命中时返回文本 `no matches for <pattern>`。

### 输出

- 每行一个匹配文件，形如 `./src/main.rs`（`./` + 根相对路径），**已排序**。
- 上限 **2000** 条；命中更多时在末尾追加一行
  `# 已截断 N 条命中（上限 2000），请缩小 pattern`——请据此细化 pattern，不要把截断结果
  当作完整清单。
- 跳过目录与 `grep` 共用同一份清单：`.git`、`target`、`node_modules`、`dist`、`build`、
  `.venv`、`venv`、`__pycache__`、`.next`（按目录名匹配，符号链接不跟随）。

### 超时

`glob` 接受可选入参 `timeout_ms`：默认 **60 秒**，可设范围 **1–600 秒**（超出即钳制）。
超时**会让本次调用失败**，与 `bash` / `grep` 一致：模型看到的是错误文本
`Error: tool timeout after Ns`，同时**部分匹配一并丢弃**（不会返回已走过的半份清单）。
据此缩小 pattern 或调大 `timeout_ms` 后重试。

## 下一步

- 工具与权限、`[agent]` 等配置项 → [配置](configuration.md)
- 命令行入口 → [命令行（CLI）](cli.md)
