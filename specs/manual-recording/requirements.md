---
status: implemented
scope: manual-recording
depends_on:
  - specs/console-ui/requirements.md
  - specs/game-monitoring/requirements.md
---

# 手动录轴与整理需求

- REQ-MANUAL-001：游戏窗口位于前台且监控处于关卡内时，按 P 必须静默创建一个待分类操作候选。
  - AC-MANUAL-001：P 不弹出 Console 或编辑器；候选出现在当前录制场次中；录制关闭、关卡外或游戏不在前台时不创建候选。
- REQ-MANUAL-002：Console 位于前台时，H 必须打开整理页，Ctrl+S 必须执行导出；文本输入期间不得触发快捷键动作。
  - AC-MANUAL-002：输入框、选择框或文本域获得焦点时，H 和 Ctrl+S 保持正常文本编辑语义；界面不注册或展示 F1–F4、F12。
- REQ-MANUAL-003：每个实时记录候选必须保存来源时间戳、帧范围和时钟质量。
  - AC-MANUAL-003：候选从权威观测时钟复制 source timestamp、最早/最晚可能帧和 ClockQuality；缺少可信观测时不得把当前显示帧写成已确认帧。
- REQ-MANUAL-004：用户必须完成时间确认和操作参数分类后，候选才能成为可导出的 AxisLink v2 操作。
  - AC-MANUAL-004：整理页可选择候选帧范围内的确定帧，或显式输入校正帧并确认人工校正；部署、技能、撤退继续使用 AxisLink v2 参数规则。
- REQ-MANUAL-005：未分类、参数不完整、时间未确认或关卡不匹配的操作必须阻止导出和代理使用。
  - AC-MANUAL-005：导出返回结构化错误并定位第一个失败操作；代理安全门复用同一套轴就绪检查，不把输入发送或显示帧当作游戏成功证据。
- REQ-MANUAL-006：人工改帧必须具有显式确认语义。
  - AC-MANUAL-006：拖动或输入新帧后，操作进入“人工校正待确认”；确认前保留原始观测帧范围且不能导出，超出该范围必须额外确认人工校正。
- REQ-MANUAL-007：一次进关到离关必须形成独立录制场次，场次只保存在当前会话并可清楚选择。
  - AC-MANUAL-007：新进关创建场次，离关结束场次；整理页可按场次筛选，跨场次候选不会隐式合并，退出应用后不自动恢复。
- REQ-MANUAL-008：工作模式必须由 Rust 权威状态表达，并为后续代理与接管流程保留最小稳定入口。
  - AC-MANUAL-008：快照包含 ManualRecording、RecordingAnalysis、Proxy 三种 ConsoleMode；切换模式不启动代理、不修改 AxisLink，也不隐式创建轴版本。
