---
status: implemented
scope: proxy-execution
depends_on:
  - specs/proxy-execution/requirements.md
  - specs/proxy-execution/design.md
---

# 代理执行任务

1. 固定代理执行、急停、风险提示和失效安全规范。（REQ-PROXY-001、REQ-PROXY-006、REQ-PROXY-010）
2. 修复录屏圆点、同帧纵向堆叠、滚轮缩放/滚动和标题栏拖动。（REQ-PROXY-007、REQ-PROXY-008、REQ-PROXY-009）
3. 生成干员目录，按需缓存头像与关卡投影资源。（REQ-PROXY-003、REQ-PROXY-004）
4. 实现头像槽位匹配、地图投影和坐标校验。（REQ-PROXY-003、REQ-PROXY-004）
5. 实现触摸点击/拖拽、同帧事务、安全门、执行记录和 F12 急停。（REQ-PROXY-001、REQ-PROXY-002、REQ-PROXY-005、REQ-PROXY-006）
6. 接入代理执行 UI 和生成绑定，补充确定性基础检查。（REQ-PROXY-001、REQ-PROXY-010）
7. 直接合入 PR，后台运行快速 CI；正式发布时执行 release 构建与启动冒烟。
