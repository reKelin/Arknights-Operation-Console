---
status: draft
scope: interactive-demo
depends_on:
  - specs/interactive-demo/requirements.md
  - specs/interactive-demo/design.md
---

# Windows 人工验收

以下项目不由纯逻辑单元测试覆盖，只能在 Windows CI 产物上确认：

- AC-DEMO-002：窗口初始尺寸约 900×236，高度不可调整，宽度可以调整。
- AC-DEMO-004：窗口失焦后，F1/F2/F3 仍分别新增部署、技能、撤退草稿。
- AC-DEMO-007：标题栏“轴”菜单可以切换三种运行策略；模拟暂停后“继续模拟”可恢复。
- AC-DEMO-009：窗口默认置顶；“视图”菜单可以关闭置顶；最小化按钮隐藏到托盘；托盘“显示”恢复窗口；关闭按钮退出程序。
- AC-DEMO-010：前端只通过生成的 `commands` 调用 Rust，不存在手写 Tauri 命令名。

验收完成后将本页 `status` 改为 `implemented`，并记录对应 CI 产物或提交。
