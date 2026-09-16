---
status: implemented
scope: axislink-v2
depends_on:
  - specs/axislink-v2/requirements.md
  - specs/axislink-v2/design.md
---

# AxisLink v2 与关卡识别任务

1. 更新产品约束并定义 AxisLink v2、短坐标和关卡安全边界。（REQ-AXISV2-001、REQ-AXISV2-002、REQ-AXISV2-007）
2. 升级 JSON Schema、示例轴、Rust 转换和草稿编辑语义，明确拒绝 v1。（REQ-AXISV2-001、REQ-AXISV2-003）
3. 固定 ArknightsGameData 版本并生成普通与集成战略关卡目录。（REQ-AXISV2-004、REQ-AXISV2-008）
4. 实现目录搜索、手动关卡选择、地图按需缓存与短坐标边界校验。（REQ-AXISV2-002、REQ-AXISV2-004、REQ-AXISV2-008）
5. 实现进关标题 ROI、Windows OCR 和代码加中文名的确定性匹配。（REQ-AXISV2-005、REQ-AXISV2-006）
6. 将关卡结果接入实时窗口、录屏区段和监控快照，并在离关或换源时清理。（REQ-AXISV2-006、REQ-AXISV2-009）
7. 实现轴关卡与观测关卡分离、错关禁止录轴和不补跑调度。（REQ-AXISV2-007）
8. 更新关卡选择、操作点短坐标、识别状态和录屏区段 UI。（REQ-AXISV2-003、REQ-AXISV2-004、REQ-AXISV2-009）
9. 生成绑定并由 Windows CI 验证协议、目录、坐标、匹配、安全状态机和发行构建。（REQ-AXISV2-010）
