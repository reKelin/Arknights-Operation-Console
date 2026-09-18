---
status: implemented
scope: console-ui
depends_on: []
---

# Console 界面需求

- REQ-CONSOLE-001：产品界面必须统一使用“Arknights Operation Console”名称，同时保留 bundle identifier、内部包名和既有配置目录。
  - AC-CONSOLE-001：窗口标题、HTML 标题、界面品牌和 README 使用新名称；`io.github.kelin.arknights-operation-runner` 与 npm 包名不变。
- REQ-CONSOLE-002：主窗口必须提供人工录轴、录屏分析和代理指挥三种紧凑模式。
  - AC-CONSOLE-002：1100 px 与 860 px 宽度下均可切换三种模式；工作页默认高度为 280 px，设置和编辑页允许随内容增高。
- REQ-CONSOLE-003：界面必须只展示已有能力和真实快照状态。
  - AC-CONSOLE-003：人工录轴、录屏时钟、监控和 AxisLink 命令连接现有 Tauri 命令；操作候选和安全代理入口明确标为尚未接入，且代理按钮不可用。
- REQ-CONSOLE-004：时间轴必须使用贯穿式标尺并区分部署、技能、撤退和待分类操作。
  - AC-CONSOLE-004：部署为圆形、技能为菱形、撤退为粗叉、待分类为方形；每个主刻度包含五个等分；视野范围限制在 40 帧至 10 分钟。
- REQ-CONSOLE-005：设置和作战轴编辑必须使用独立页面。
  - AC-CONSOLE-005：设置页包含监控、外观、快捷键和执行页签；编辑页支持轴信息、操作筛选、合法参数校验和可选备注。
- REQ-CONSOLE-006：Console 快捷键说明必须统一为游戏前台 P 记录，以及 Console 前台 H 整理、Ctrl+S 导出。
  - AC-CONSOLE-006：界面不再展示旧 F1–F4 或 F12 快捷键；H 和 Ctrl+S 在 Console 前台生效。
