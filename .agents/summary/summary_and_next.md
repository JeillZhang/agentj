# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台体验修复（v0.16.0）—— 六项修复，无协议变更**
> ——设计来源=用户反馈批次（无设计文档；PROTOCOL_VERSION 保持 0.13.0，工作区 0.15.2 → 0.16.0）

## v0.16.0: Web 工作台体验修复 ✅（本次完成）

- ✅ **(1) 协议版本 semver 兼容语义**：major 相同即兼容，仅 major 不匹配才显示 banner；0.12.1 client vs 0.13.0 server 不再误报。
- ✅ **(2) composer 选项弹层锚定**：逐触发器锚定（`.sel-wrap`），不再偏移到发送按钮附近。
- ✅ **(3) 选项/会话行 hover 反馈**：背景 + accent + 过渡。
- ✅ **(4) light 主题对比度**：`--aj-border #E4E7EA→#CBD2DB`、`--aj-raised #F9FAFB→#E9EDF1`。
- ✅ **(5) 左栏会话管理**：逐行左侧删除 + 一键清理（`session/delete`，排除当前会话，-32802/-32803 幂等容忍，`session/list` 过滤 deleted）。
- ✅ **(6) 中栏全宽拉伸**：对话区/composer 移除 720px 居中上限，全宽拉伸。
- ✅ 门禁全绿：Web 230 tests / tokens 11 / sdk 37 / tui 71 / web build / desktop-ui ok / check-version ok（PROTOCOL_VERSION 保持 0.13.0，工作区 0.15.2 → 0.16.0）。

## 遗留 / 下一步

- 无开放 bug。
- 下一功能 TBD（待用户/路线图确认）。
