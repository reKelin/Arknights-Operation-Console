---
status: implemented
scope: manual-recording
depends_on:
  - specs/manual-recording/requirements.md
---

# 手动录轴与整理设计

## 状态边界

Rust 在 `RunnerState` 中持有 ConsoleMode、当前会话的 RecordingAttempt 列表和事件时间证据。React 只选择模式、场次并提交分类或时间确认命令，不自行推导可信帧。

`DraftEvent` 继续作为内部草稿操作，并增加不写入 AxisLink 的录制元数据：

- `attemptId`：所属录制场次；
- `sourceTimestampNs`：产生 P 输入时最近的权威观测来源时间；
- `frameRange.start/end`：该来源时刻可能对应的逻辑帧闭区间；
- `clockQuality`：记录时的观测质量；
- `timeConfirmation`：`unconfirmed | observed | manuallyCorrected`。

导入的 AxisLink 事件和在编辑器中明确新增的事件没有来源时间戳，但以文件或人工输入的确定帧进入 `observed` / `manuallyCorrected` 状态。实时 P 候选只有在权威时钟给出可确认的单帧范围时才能直接进入 `observed`；其余情况保持 `unconfirmed`。

AxisLink v2 Schema 不增加上述字段。导出仍由 `DraftAxis::to_axis_json` 生成，转换前调用统一的轴就绪检查。

## 录制场次

Runner 在可信的进关边界创建 RecordingAttempt，在离关边界结束它。场次包含稳定 ID、开始与结束来源时间戳、关卡证据和关联事件 ID。P 只向活动场次添加候选，不触发窗口显示或模式切换。

场次与草稿均只存在于内存。导入轴不创建伪造的实机场次；手动新增操作归入“人工编辑”筛选项。

## 时间确认

时间编辑使用单独命令，输入包括事件 ID、目标帧与用户是否确认人工校正。目标帧位于观测范围内时可以确认观测时间；超出范围时必须显式确认人工校正。普通 `move_event` 不再直接把实时候选变成确定操作。

界面在帧输入附近展示原始范围、来源时间和质量。保存分类参数不能隐式确认时间；确认时间也不能隐式补全操作参数。

## 快捷键

全局快捷键插件只注册 P，并且只在已选择的游戏窗口位于前台时保持注册。H 与 Ctrl+S 由 Console WebView 的键盘事件处理，焦点位于 input、select、textarea 或 contenteditable 时直接返回。旧 F1–F4、F12 不再注册；界面停止入口由后续代理事务继续复用已有命令。

## 模式与后续入口

ConsoleMode 只描述当前工作视图。`set_console_mode` 停止现有实验性代理状态，但本功能不会武装代理、追赶当前局或创建版本。D/E 可以在该命令和快照字段之上增加下一局 F0 武装与 K 接管，不需要改变手动候选模型。
