---
status: implemented
scope: paused-execution
depends_on:
  - specs/paused-execution/design.md
---

# 暂停执行任务

1. 更新真实输入、快捷键、键位确认和接管边界。（REQ-PEX-001、REQ-PEX-002、REQ-PEX-009）
2. 为执行画面增加序号、源时间戳、新鲜等待和非阻塞发布。（REQ-PEX-004、REQ-PEX-005）
3. 实现触摸与键盘输入的取消安全收尾。（REQ-PEX-002、REQ-PEX-009）
4. 实现部署、技能和撤退的暂停事务与结果证据。（REQ-PEX-003、REQ-PEX-004、REQ-PEX-007）
5. 实现稳定回执和待人工确认入口。（REQ-PEX-007、REQ-PEX-008）
6. 在 Console 模式与可信时钟接口稳定后接入提前暂停、F0 武装、目标帧调度、K 接管和生成绑定。（REQ-PEX-005、REQ-PEX-006、REQ-PEX-009、REQ-PEX-010）
7. 为 Windows CI 留下取消收尾、过期画面、跨帧、同帧顺序和不确定结果的最小回归检查。（REQ-PEX-004、REQ-PEX-006、REQ-PEX-007、REQ-PEX-009）
