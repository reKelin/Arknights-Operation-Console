---
status: implemented
scope: console-ui
depends_on:
  - specs/console-ui/requirements.md
  - specs/console-ui/design.md
---

# Console 界面任务

1. 统一产品显示名称、内部标识、构建产物和配置路径，并设置紧凑窗口尺寸。（REQ-CONSOLE-001、REQ-CONSOLE-002）
2. 重组工作台为人工录轴、录屏分析和代理指挥三种模式，并连接已有快照与命令。（REQ-CONSOLE-002、REQ-CONSOLE-003）
3. 实现独立设置页与轴编辑页，保留现有监控、主题、轴参数和操作编辑能力。（REQ-CONSOLE-005）
4. 更新单轨时间轴的贯穿式刻度、五等分、操作形状和 40 帧至 10 分钟缩放范围。（REQ-CONSOLE-004）
5. 接入 H 与 Ctrl+S，移除界面中的旧快捷键说明，并明确禁用尚未满足安全约束的代理入口。（REQ-CONSOLE-003、REQ-CONSOLE-006）
6. 执行允许的前端格式、类型、构建和纯逻辑检查；保留 Windows 实机视觉验收给后续集成阶段。
