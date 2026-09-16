---
status: implemented
scope: capture-bookmarks
depends_on:
  - specs/capture-bookmarks/requirements.md
  - specs/capture-bookmarks/design.md
---

# 全屏 OCR、窗口捕获与书签任务

1. 更新产品窗口、OCR、捕获和书签约束。（REQ-CB-001–REQ-CB-007）
2. 实现完整视口 OCR 与连续帧文本聚合。（REQ-CB-001）
3. 实现低权限窗口发现、直接 HWND 复验、缓冲回退和首帧超时。（REQ-CB-002、REQ-CB-003）
4. 提升时间轴缩放、窗口尺寸和圆点可读性。（REQ-CB-004）
5. 实现书签草稿及 P/H 条件热键。（REQ-CB-005、REQ-CB-007）
6. 实现书签列表和集中编辑命令。（REQ-CB-006）
7. 运行快速检查后直接合并，后台执行 CI，正式发版时构建冒烟。
