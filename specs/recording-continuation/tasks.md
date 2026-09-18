---
status: approved
scope: recording-continuation
depends_on:
  - specs/recording-continuation/requirements.md
  - specs/recording-continuation/design.md
---

# 录屏接续合并任务

1. [ ] 实现单区段时间对齐、接管边界过滤和有符号帧校正。（REQ-CONTINUATION-001 至 REQ-CONTINUATION-003）
2. [ ] 实现按稳定来源 ID 去重、同帧稳定顺序和冲突计划。（REQ-CONTINUATION-004、REQ-CONTINUATION-005）
3. [ ] 使用确定性夹具覆盖可信/人工偏移、跨区段、重复候选、同帧多操作、溢出和冲突决议。（REQ-CONTINUATION-002 至 REQ-CONTINUATION-005）
4. [ ] E 的会话接口稳定后接入 `recordingMerge` 子版本创建、指定版本导出和下一局武装门禁。（REQ-CONTINUATION-006、REQ-CONTINUATION-007）
5. [ ] 增加独立接续校对组件并最小挂载到录屏页面，复用现有版本选择和候选校对状态。（REQ-CONTINUATION-001、REQ-CONTINUATION-005）
6. [ ] 生成绑定并完成允许的静态检查；真实接管录屏对齐和新轴代理循环留作实机验收。
