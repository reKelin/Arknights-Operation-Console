---
status: implemented
scope: trusted-clock-observation
depends_on:
  - specs/trusted-clock-observation/requirements.md
---

# 可信观测与双时钟设计

## 边界

- `monitor/live.rs` 负责 WGC 配置和实时源时间戳。
- `monitor.rs` 负责有界事件队列、丢失计数和监控快照。
- `monitor/clock.rs` 负责人工／代理时钟、可信度和交接值。
- `runner.rs` 只消费时钟快照并执行既有通知调度；React 不计算游戏时间。
- 本功能不实现键盘、触摸或暂停执行事务。

## 观测传输

`MonitorEventQueue` 使用容量 64 的 `VecDeque`。发布时创建 `MonitorEventEnvelope { sequence, source_timestamp_ns, dropped_before, event }`。容量满时移除最旧项并累积丢失数；该数字附到下一次实际取出的 envelope。这样捕获回调不会等待 UI／Console，也不会静默覆盖 latest slot。

源时间戳是源媒体时间：实时 WGC 使用会话起点后的单调时间，录屏使用视频 PTS。它不是主机收到事件的时间，也不跨监控会话比较。事件序号同样只在一个 `MonitorManager` 生命周期内单调递增。

## 时钟模型

`HumanClock` 和 `ProxyClock` 各自包含一个私有 `ClockState`。定点推进、纳秒转帧、速度映射和费用锚点目标计算是无状态纯函数。两种时钟使用相同算法，只有允许的观测状态不同：代理时钟不接受 `PointTwoXRunning` 或 `DeployingOperator`。

`ClockSnapshot` 包含模式、活动状态、当前帧、`speed_fifths`、质量、误差帧、最后可信锚点和最近源时间戳。丢帧把质量设为 `lost`；后续普通高置信度观测只能进入 `uncertain`。获得可信且非满费的费用相位后，时钟用费用锚点重新约束并恢复 `trusted`。

`ClockHandoff` 是值对象，只包含帧、定点余数、源时间戳和费用锚点状态。`trusted_handoff` 先验证活动、质量和时间锚点，再暂停发送方；`accept_handoff` 覆盖接收方的时钟状态但不携带通知、执行回执或调度历史。

## WGC 回退

启动先使用 `DrawBorderSettings::WithoutBorder`。只对 `BorderConfigUnsupported` 或设置边框时返回的兼容／权限 HRESULT 重试 `Default`。回退成功后，`MonitorSnapshot.capture_warning` 保留提示；其他初始化错误直接返回。该路径不把默认边框描述为无边框成功。
