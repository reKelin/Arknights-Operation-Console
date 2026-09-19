---
status: superseded
scope: interactive-demo
depends_on:
  - specs/interactive-demo/requirements.md
  - specs/interactive-demo/design.md
---

# Windows 人工验收

本页保留早期 demo 的验收定义，不再作为当前操作指令。当前窗口、快捷键与本地 app 验收使用 [现行验收清单](../../docs/subsystems/ci-manual-acceptance.md)。

以下是早期 demo 的历史验收范围：

- AC-DEMO-002：窗口初始尺寸约 900×236，高度不可调整，宽度可以调整。
- AC-DEMO-004：窗口失焦后，F1/F2/F3 仍分别新增部署、技能、撤退草稿。
- AC-DEMO-007：标题栏“轴”菜单可以切换三种运行策略；模拟暂停后“继续模拟”可恢复。
- AC-DEMO-009：窗口默认置顶；“视图”菜单可以关闭置顶；最小化按钮隐藏到托盘；托盘“显示”恢复窗口；关闭按钮退出程序。
- AC-DEMO-010：前端只通过生成的 `commands` 调用 Rust，不存在手写 Tauri 命令名。

当前验收结果记录到现行清单，不把本页的历史需求视为当前功能已通过。
