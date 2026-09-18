---
status: approved
scope: recording-candidates
depends_on:
  - specs/recording-candidates/requirements.md
  - specs/recording-candidates/design.md
---

# 录屏操作候选任务

1. 定义原始 PTS、30 Hz 游戏帧范围、候选证据、待确认字段和已确认操作的会话内类型。（REQ-RECORDING-001、REQ-RECORDING-003）
2. 实现 time base 与原始 PTS 解析、纳秒换算、倒退和溢出检查。（REQ-RECORDING-001、REQ-RECORDING-004）
3. 实现稳定状态序列提取器，识别部署手势和种类未知的选中操作，跨断点时停止匹配。（REQ-RECORDING-002、REQ-RECORDING-004）
4. 实现三类 AxisLink 操作的人工确认校验；未完成候选保持不可导出、不可代理。（REQ-RECORDING-005、REQ-RECORDING-006）
5. 使用合成 JSON 夹具验证变帧率 PTS、暂停、抖动、断点、截断操作和字段校验。（REQ-RECORDING-007）
6. 在可信时钟接口稳定后，把源 PTS 和游戏帧范围接入录屏解码链及监控快照，并生成前端绑定。（REQ-RECORDING-001、REQ-RECORDING-006）
7. 接入候选校对页面和会话内轴转换；保持 AxisLink v2 Schema 不变。（REQ-RECORDING-005、REQ-RECORDING-006）
8. 使用真实官方 PC 客户端录屏完成人工标注对照，记录未通过场景，不以合成夹具替代实机结论。（REQ-RECORDING-002、REQ-RECORDING-007）
