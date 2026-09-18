---
status: approved
scope: recording-continuation
depends_on:
  - specs/recording-candidates/requirements.md
  - specs/takeover-recording/requirements.md
---

# 录屏接续合并需求

- REQ-CONTINUATION-001：用户必须选择一个接管版本、对应实机场次和一个录屏关卡区段后才能开始接续合并。
  - AC-CONTINUATION-001：录屏区段索引不得转换为实机 `RecordingAttempt`；合并结果保存父版本、场次和录屏区段来源。
- REQ-CONTINUATION-002：录屏区段时间必须通过可信锚点或用户明确确认的帧偏移映射到本局游戏时间，不得跨缺帧或区段断点猜测连续时间。
  - AC-CONTINUATION-002：帧偏移必须等于目标接管帧减去录屏源锚点帧；不可信对齐没有人工确认时拒绝合并；候选必须全部来自所选区段。
- REQ-CONTINUATION-003：新版本必须完整保留父版本中的已执行前缀，并且只合入录屏源锚点之后的已确认候选。
  - AC-CONTINUATION-003：父版本事件原样保留；位于或早于源锚点的候选不进入结果；合入事件的帧和帧范围使用同一个已确认偏移校正。
- REQ-CONTINUATION-004：重复检测必须使用稳定来源标识和执行回执标识，不得按四舍五入后的游戏帧删除操作。
  - AC-CONTINUATION-004：父版本或本批次已包含同一 `candidateId` 时拒绝；同帧不同来源的操作按父前缀优先、候选来源顺序稳定保留。
- REQ-CONTINUATION-005：参数缺失、时间未确认和同格同类冲突必须进入人工校对，未经解决不得创建可导出或可代理的版本。
  - AC-CONTINUATION-005：待分类、字段不完整、`timeConfirmation=unconfirmed` 的候选拒绝进入合并计划；同帧同格同类但来源不同的操作要求用户明确保留或排除候选。
- REQ-CONTINUATION-006：完成校对后必须通过 E 的会话入口创建 `recordingMerge` 子版本，不得覆盖父版本或建立第二套版本存储。
  - AC-CONTINUATION-006：子版本使用父 `revisionId`、现有 `attemptId` 和录屏合并来源信息；旧版本仍可独立选择、导出和用于下一局代理。
- REQ-CONTINUATION-007：接续后的新版本必须继续服从现有导出和代理门禁。
  - AC-CONTINUATION-007：书签、未完成事件、未确认时间或待解决冲突存在时，指定版本导出和下一局武装均返回结构化错误。

## 当前证据边界

确定性夹具可以验证偏移、区段隔离、来源去重、同帧稳定顺序和冲突决议。仓库没有可对应同一实机接管节点的原始录屏，因此时间锚点选择、缺片段识别和接续后的实机代理循环仍未验收。
