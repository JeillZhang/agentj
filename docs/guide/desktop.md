# 桌面应用（Desktop）

`agentj-ui/apps/desktop` 是 Tauri v2 壳，套用 `agentj-ui/apps/web` 的构建产物
（`frontendDist → ../../web/dist`），与工作区版本同源。

## 构建

```bash
npm --prefix agentj-ui run build --workspace=@agentj/web      # 先产出 frontendDist
npm --prefix agentj-ui run build --workspace=@agentj/desktop  # tauri build
```

产物位于 `target/release/bundle/`，输出 **deb / AppImage / dmg / msi** 四件套。

## 校验

```bash
./agentj-ui/apps/desktop/check-artifacts.sh linux   # 校验 deb + AppImage
node agentj-ui/apps/desktop/check-desktop-ui.mjs    # 前端产物/配置一致性校验
```

## 远程连接

桌面壳默认连接本机内核（明文回环）。连接设置与 Web 相同（持久化到本地，刷新恢复），
可通过「连接设置」指向局域网/远程 `ws://`（https → wss）。
