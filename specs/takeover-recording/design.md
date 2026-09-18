---
status: implemented
scope: takeover-recording
depends_on:
  - specs/takeover-recording/requirements.md
---

# 接管续录设计

## 会话与版本

Rust 持有一个只存在于内存的 `OperationSession`。它包含稳定会话 ID、按创建顺序排列的 `AxisRevision`、当前查看版本、活动续录版本、下一局武装版本和接管状态。React 只选择版本并发送命令，不复制版本或判断执行前缀。

`AxisRevision` 包含稳定 ID、递增序号、可空父版本 ID、来源、关联录制场次、创建时逻辑帧和一份 `DraftAxis`。来源使用 `imported | manual | takeover | recordingMerge`；接管来源额外保存 runId、回执序号和时间可信标志，时间未可信的版本不能导出或武装。`recordingMerge` 预留给 G，E 不实现录屏合并。运行期间编辑只改变活动版本；创建接管版本后旧版本保持不可变。AxisLink v2 导出只读取所选版本内的 `DraftAxis`，不会增加版本字段。

每次下一局可信 F0 真正激活代理时，Runner 分配会话内唯一 `runId`。回执固定携带该 ID，接管状态保存被取消的 runId。相同 eventId 可以在不同版本和不同局重复执行；构造器只接受与本次 runId 相同的回执，历史运行的 confirmed 不能进入新版本。

当前 C 的单一 `RunnerState.axis` 在接入时迁入首个版本，但命令仍通过 Runner 取得活动轴，避免在 UI 或执行器建立第二套权威状态。事件 ID 在版本复制、回执确认和人工续录中保持稳定；新录制事件继续使用全会话唯一 ID 分配器。

## 接管事务

`TakeoverState` 使用 `idle | cancelling | awaitingPauseProof | recording | unknown`。有效 K 先在 Runner 锁内冻结本次 runId、父版本、录制场次和请求代次并关闭调度，再调用 D 的原子取消入口。若执行 worker 仍在收尾，状态保持 `cancelling`；最终回执归并并完成暂停确认后才一次性创建版本。Runner runtime 和 K 命令遵循相同的 Runner → worker 锁序，避免 runtime 已取走结果而 K 抢先创建空前缀。只有同一代次的取消完成结果可以推进状态，避免重复 K 或迟到回调创建多个版本。

接管新版本依据 D 的 `ExecutionReceipt` 构造。先按本次 `runId` 限定回执，再按 `receiptSequence` 排序，并要求 `eventId` 能唯一映射旧版本事件。构造器逐条处理回执：

- `confirmed`：把对应事件原样复制到新版前缀；顺序由回执序号和原事件稳定顺序共同确定，不按帧比较；
- `uncertain`：复制对应事件并在接管状态中登记待人工确认，随后停止构造已执行前缀；
- `failed` / `cancelled`：不复制该事件并停止；
- 没有回执的旧轴尾部不复制，只保留在旧版本。

同帧事件因此按真实回执保留。重复 eventId、倒退/重复 receiptSequence 或无法映射的回执使接管进入 `unknown`，不猜测前缀。

## 时钟交接

K 不读取 React 显示帧。旧 worker 收尾后，执行器在可信运行画面下主动尝试暂停；已经暂停时不重复发送暂停键，不可信时不猜测。接管使用 B/D 提供的 `ClockHandoff` 和最终新鲜暂停证明，将实际暂停后的 ProxyClock 帧、余数、来源时间和费用锚点交给 HumanClock，中间发生的时间推进保持连续。锚点不可信或暂停状态无法证明时，新版本可以建立以保留证据，但 `TakeoverState` 保持 `unknown`，P 和代理武装不可用。

## 命令与快捷键

全局快捷键按当前权威状态动态注册：人工续录时使用 P，代理模式且选定游戏窗口位于前台时使用 K。K 不显示窗口，也不在无效作用域创建状态。Console 提供以下最小命令：

- `takeover_now`：开始一次 K 接管事务；
- `resolve_uncertain_receipt`：复用 D 的人工确认入口；
- `select_axis_revision`：切换查看版本；
- `export_axis_revision`：对指定版本执行现有门禁后导出；
- `arm_revision_for_next_battle` / `disarm_proxy`：只控制下一局 F0 武装和停止。

界面停止调用 D 的取消/停止入口并保持版本不变。`arm_revision_for_next_battle` 拒绝当前已在关卡内的追赶执行。

## 下游合并边界

G 可以用 `recordingMerge` 来源创建子版本，并复用版本选择、导出和下一局武装命令。录屏 provenance 显式保存 `recordingAnalysisId`、`segmentIndex`、`frameOffset` 和候选 ID；去重键是三元组 `(recordingAnalysisId, segmentIndex, candidateId)`，segmentIndex 不转换为实机 `RecordingAttempt`。E 不创建工程文件、恢复文件或自动保存。
