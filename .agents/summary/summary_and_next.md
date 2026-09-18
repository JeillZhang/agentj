# Summary & Next

> 本文件仅保留**当前正在开发**功能设计文档的内容：进度、剩余步骤、遗留问题与下一步。同步更新时**替换**（不要追加）前一功能内容。
> 当前功能：**Web 工作台跟随修复（v0.16.1）—— 三项修复，无协议变更**
> ——设计来源=用户反馈批次（无设计文档；PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.0 → 0.16.1）

## v0.16.1: Web 工作台跟随修复 ✅（本次完成）

- ✅ **(1) 左栏会话删除按钮移至收藏星标下方**：flat 行右栏堆叠（star top 6 → delete top 30），行 `min-height: 56px`，左 padding 还原为 10px；项目分组内会话行不变。
- ✅ **(2) 右上角模型 chip 移除**：`aj-topbar__actions` 不再渲染 `deepseek-chat`；模型仍由 composer 与 usage meta 行展示；「⋯」菜单状态区保留。
- ✅ **(3) 会话单击不再自动收藏**：`handleContinue` 移除 T15 遗留的自动收藏，仅星标按钮触发；新增回归测试（点击行 → 收藏为空 + `aria-pressed=false`；点星 → 收藏生效），已用 stash 验证旧代码上先红。
- ✅ 门禁全绿：Web 231 tests / sdk 37 / web build / desktop-ui ok / check-version ok（PROTOCOL_VERSION 保持 0.13.0，工作区 0.16.0 → 0.16.1）。

## 遗留 / 下一步

- 无开放 bug。
- 待确认：新建会话仍会自动收藏（v0.11 遗留，测试 `new session becomes active and favorited`）——用户尚未表态，暂保留。
- 下一功能 TBD（待用户/路线图确认）。
