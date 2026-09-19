---
status: implemented
scope: console-workflow
depends_on:
  - specs/console-workflow/requirements.md
---

# 工作流与状态边界

React 保留现有快照与命令边界。原型中的固定时间、倒计时、进度、版本号、样例操作和自动成功均替换为真实数据。工作台不常驻候选列表、接续面板、状态页脚；接续是编辑页的按需入口。

录屏 Ready 首次到达时，Rust 为首个有候选的区段创建 `recordingMerge` 草稿版本，复用已有来源字段。新增 `select_recording_segment(segment_index)` 内部命令选择已有区段版本，或为尚未选择的区段创建版本。每次创建保留父版本；已有旧版本只读规则不变。

默认填充保留候选中的确定类型、合法参数及来源时间。`unconfirmed_fields` 标记的参数留空；范围取起点仅作为待校对位置。只有可信、精确且未标记不确定的时间自动确认为 observed。缺失参数使 complete 为 false，仍使用既有导出与代理完整性门禁。没有候选的区段返回明确错误，不伪造操作。

独立分析轴可直接使用或按 H 编辑。接管后的视频仍先形成独立分析草稿；其已确认事件可供已有接续合并器消费，人工只处理共同锚点和冲突等不确定项。React 不实施截断、去重、帧排序或代理调度。

规格不改变 AxisLink、持久化、30 Hz 时钟、输入权限、代理二次确认和急停语义。
