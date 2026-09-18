---
status: superseded
scope: proxy-execution
depends_on:
  - specs/axislink-v2/requirements.md
---

# 代理执行需求

本规范的输入与急停部分已由 `specs/paused-execution/requirements.md` 取代；既有投影、资源和时间轴要求继续作为历史实现依据。

- REQ-PROXY-001：真实输入必须通过明确的“代理执行”开关启用。
  - AC-PROXY-001：每次启动默认关闭；连续两次确认后开启；离关、换源、失焦、不可信、错关或急停时自动关闭。
- REQ-PROXY-002：代理执行必须只使用 Windows 用户输入接口。
  - AC-PROXY-002：部署、技能与撤退仅调用 `InjectTouchInput` 和窗口坐标 API，不访问游戏内存或注入游戏进程。
- REQ-PROXY-003：部署必须视觉定位目标干员。
  - AC-PROXY-003：轴中干员的头像资源按需缓存；只有部署栏唯一高置信匹配时才执行拖拽，失败时停止而不猜测卡位。
- REQ-PROXY-004：格子必须按关卡投影到游戏客户区。
  - AC-PROXY-004：A1–I36 按字母行、数字列转换；部署使用侧视投影，技能和撤退使用正视投影。
- REQ-PROXY-005：同帧操作必须稳定串行执行。
  - AC-PROXY-005：同帧按 `order`、`id` 排序；任一步失败立即停止该帧后续操作，不补发。
- REQ-PROXY-006：代理执行必须支持全局急停。
  - AC-PROXY-006：F12 立即取消活动触点、关闭代理执行并留下失败记录。
- REQ-PROXY-007：时间轴必须正确展示录屏标记和同帧操作。
  - AC-PROXY-007：录屏状态为圆点；同帧操作共享同一 X 坐标并纵向堆叠。
- REQ-PROXY-008：时间轴必须支持剪辑软件式滚轮。
  - AC-PROXY-008：滚轮横向滚动；Alt+滚轮围绕指针所在帧缩放。
- REQ-PROXY-009：自绘窗口必须可以拖动。
  - AC-PROXY-009：标题栏非交互区域按下并拖动时调用 Tauri 窗口拖动。
- REQ-PROXY-010：代理执行风险必须对用户可见。
  - AC-PROXY-010：首次确认明确说明合成输入可能存在账号或反作弊风险。
