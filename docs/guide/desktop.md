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

## CI 只验证 Linux 打包

`.github/workflows/ci.yml` 的 `desktop-package` 作业是**唯一跑 `tauri build`** 的地方：它在
`ubuntu-latest` 上装 Node 24 与 Tauri 系统依赖，先做 protocol / SDK 类型生成与 web 构建
（dist 必须先存在，否则 `frontendDist` 为空），再
`npm --prefix agentj-ui/apps/desktop run build -- --bundles deb,appimage`，用
`check-artifacts.sh linux` 校验后把 `*.deb` / `*.AppImage` 作为 `agentj-desktop-linux` 上传
（保留 7 天）。作业不跑测试套件，有 45 分钟超时。

- **`dmg` / `msi` 不在 CI 里，需手动构建**：仓库没有配置 macOS 签名 / 公证证书与 Windows
  代码签名（`windows-latest` 自带 WiX，缺的是证书），跨平台作业（`cross-platform`）只做
  `cargo build --workspace`，**不打包**。
  要发这两个包就在对应平台的机器上跑上面那两条 `--bundles dmg` / `--bundles msi` 命令，再
  用 `check-artifacts.sh macos` / `check-artifacts.sh windows` 自检。
- **CI 绿不等于四件套齐全**：它只保证 Linux 的 `deb` + `AppImage` 能构建、能通过存在性校验。

## 远程连接

桌面壳默认连接本机内核（明文回环）。连接设置与 Web 相同：**设置 → 连接** 填
`Host` / `Port` / `Token`（持久化到本地，刷新恢复），可指向局域网/远程内核。
Desktop 壳的页面来源是 `tauri.localhost`，地址始终派生为明文回环 `ws://`；
完整解析顺序见 [Web 工作台](web.md)。
