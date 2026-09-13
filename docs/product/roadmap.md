---
status: approved
scope: product
depends_on:
  - docs/product/requirements.md
  - docs/product/architecture.md
---

# 开发路线

## 阶段 1：交互 Demo

- 建立 Tauri 2 + React/TypeScript + Rust 单应用工程和 Windows CI。
- 实现自动模拟关卡时钟、紧凑计时器、可横向滚动的单轨时间轴。
- 实现 F1/F2/F3 实时录轴、任意帧新增、拖动改帧和右键编辑执行参数。
- 实现 AxisLink JSON 导入导出、提醒与自动暂停预演。
- 所有执行只写预演结果，不接触游戏。

## 阶段 2：实机时钟

- 接入 ArknightsCostBarRuler WebSocket，获得费用帧、累计帧和战斗状态。
- 实现费用锚定、暂停/倍速分段和满费墙钟外推。
- 为人类操作增加不丢帧的状态识别链、误差区间和回溯校正。

## 阶段 3：零帧执行

- 发现并验证官方 PC 客户端窗口。
- 使用 `InjectTouchInput` 实现暂停部署。
- 参考 AFA 实现暂停选中、技能和撤退。
- 增加执行确认、急停、失败停机和暂停事务日志。

## 阶段 4：自动记录

- 从输入与视觉结果生成待确认操作点。
- 逐步识别干员、格子、朝向和技能目标。

## 阶段 5：录屏推断

- 对视频 PTS 复用状态与动作识别器。
- 输出带误差和置信度的候选轴，不直接生成可自动执行的权威轴。
