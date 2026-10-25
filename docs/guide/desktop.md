# 桌面应用（Desktop）

`agentj-ui/apps/desktop` 是 Tauri v2 壳，套用 `agentj-ui/apps/web` 的构建产物
（`frontendDist → ../../web/dist`），与工作区版本同源。

## 构建

```bash
npm --prefix agentj-ui run build --workspace=@agentj/web      # 先产出 frontendDist
npm --prefix agentj-ui run build --workspace=@agentj/desktop  # tauri build
```

产物位于 `target/release/bundle/`，四件套为 **deb / AppImage / dmg / msi**——但**一个平台
只能打本平台的包**：`.deb` / `.AppImage` 要 Linux、`.dmg` 要 macOS、`.msi` 要 Windows（含
WiX）。只打其中一部分时用 `--bundles`：

```bash
npm --prefix agentj-ui/apps/desktop run build -- --bundles deb,appimage   # Linux
npm --prefix agentj-ui/apps/desktop run build -- --bundles dmg            # 需在 macOS 上跑
npm --prefix agentj-ui/apps/desktop run build -- --bundles msi            # 需在 Windows 上跑
```

## 校验

```bash
./agentj-ui/apps/desktop/check-artifacts.sh linux       # deb + AppImage
./agentj-ui/apps/desktop/check-artifacts.sh linux-deb   # 只查 deb
./agentj-ui/apps/desktop/check-artifacts.sh macos       # dmg
./agentj-ui/apps/desktop/check-artifacts.sh windows     # msi
node agentj-ui/apps/desktop/check-desktop-ui.mjs        # 前端产物/配置一致性校验
```

脚本只检查产物是否存在（`artifacts-ok <platform>` 即通过），不验证版本号或安装行为；
不带参数时默认 `linux`，未知平台会报错并列出可选值。

## CI 打包（三平台）

`.github/workflows/ci.yml` 有三个打包作业，每个平台一个，彼此独立：

| 作业 | runner | 产物 | 校验 | 上传名 |
|------|--------|------|------|--------|
| `desktop-package` | `ubuntu-latest` | `deb` + `AppImage` | `check-artifacts.sh linux` | `agentj-desktop-linux` |
| `desktop-package-macos` | `macos-latest` | `dmg` | `check-artifacts.sh macos` | `agentj-desktop-macos` |
| `desktop-package-windows` | `windows-latest` | `msi` | `check-artifacts.sh windows` | `agentj-desktop-windows` |

三个作业的步骤相同：装 Node 24 → protocol / SDK 类型生成 → web 构建（dist 必须先存在，
否则 `frontendDist` 为空）→ `tauri build --bundles <…>` → 平台校验脚本 → 上传产物（保留 7 天，
没有产物即失败）。都不跑测试套件；Linux 作业 45 分钟超时，另两个 60 分钟
（macOS / Windows runner 更慢）。

- **macOS / Windows 作业按 `desktop-package` label 门控**：只有 PR 带该 label 或手动
  `workflow_dispatch` 时才会跑，避免每个 PR 都付 macOS/Windows runner 的成本。想验证跨平台打包，
  给 PR 打上 `desktop-package` label 再重跑 workflow。
- **`scripts/tests/assert-ci.sh desktop-package-platforms`** 把上面这份契约钉进仓库不变量
  （runner / bundle / 校验脚本 / `shell: bash` / 上传名 / 保留期 / label 门控），改 `ci.yml`
  却没同步这份契约会直接红。

### 产物未签名（用户绕过）

三个作业都**不签名、不公证**（仓库没有 macOS Developer ID 证书与 Apple 公证凭据，也没有
Windows 代码签名证书），所以首次打开会有系统警告：

- **macOS**：双击 `.dmg` 拖入「应用程序」后，右键（或 Control-点击）应用图标 → **打开** →
  确认打开；已装过则可用
  `xattr -d com.apple.quarantine /Applications/AgentJ.app` 清掉隔离属性。
- **Windows**：`.msi` 安装时 SmartScreen 可能提示「Windows 已保护你的电脑」→ **更多信息** →
  **仍要运行**。
- **Linux**：`deb` / `AppImage` 不受影响；`AppImage` 需 `chmod +x` 后运行。

## 远程连接

桌面壳默认连接本机内核（明文回环）。连接设置与 Web 相同：**设置 → 连接** 填
`Host` / `Port` / `Token`（持久化到本地，刷新恢复），可指向局域网/远程内核。
Desktop 壳的页面来源是 `tauri.localhost`，地址始终派生为明文回环 `ws://`；
完整解析顺序见 [Web 工作台](web.md)。
