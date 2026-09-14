---
status: approved
scope: game-monitoring
depends_on:
  - specs/game-monitoring/requirements.md
---

# 游戏监控与同步设计

## 文件边界

- `src-tauri/src/monitor.rs`：监控生命周期、窗口与录屏源、线程通信。
- `src-tauri/src/monitor/vision.rs`：参考坐标、战斗状态与费用条纯视觉函数。
- `src-tauri/src/monitor/clock.rs`：30 Hz 观测时钟状态机和录屏轨迹。
- `src-tauri/src/settings.rs`：主题、费用周期分母和游戏 UI 比例的应用设置。
- `src-tauri/src/runner.rs`：持有轴、调度、监控快照和清空确认状态。
- `src/App.tsx`：监控源、主题、设置、录屏进度和清空确认界面。
- `src/Timeline.tsx`：响应式时间轴与录屏轨迹定位。

仍使用单个 `src-tauri` crate。窗口与录屏已经是两个真实来源，使用 `MonitorSource` enum 共享后续视觉和时钟链，不建立可注入插件体系。

## 外部基线与依赖

- ArknightsCostBarRuler `70d3826298927ba15d6145b0bd92dca8b62fa603` 仅用于核对战斗状态分类、费用边界和失效案例，不作为依赖，也不复制源代码。
- 实时捕获使用 `windows-capture 2.0.1`；关闭光标捕获和黄色边框，只处理选定窗口。
- 录屏使用 `std::process::Command` 调用系统 `ffprobe`/`ffmpeg`，不自动下载或捆绑二进制。缺失时给出可执行文件名和修复提示。
- 不引入图像处理框架；少量 ROI 直接扫描 BGRA 像素。

## 设置

应用设置保存在 Tauri `app_config_dir/settings.json`：

```text
theme = dark | light
frames_per_cost = 15..150
game_ui_scale = 0..100
```

写入使用临时文件加同目录替换。损坏或未知版本回退默认值并返回提示。默认值为深色、30 帧/费用、100% 游戏 UI 比例。该文件不包含轴、录屏路径或窗口句柄。

## 实时窗口源

`list_game_windows` 使用 `windows-capture::window::Window::enumerate`，读取进程名、标题和捕获能力，只返回 `Arknights.exe` 的可见顶层窗口。UI 使用一次扫描结果中的短期候选 ID 调用 `select_game_window`；Rust 再次验证候选，避免接受前端提供的任意句柄。

捕获回调把最新 BGRA 帧和单调时间写入容量为 1 的通道；满时替换旧帧。窗口关闭、尺寸变化或捕获错误终止当前会话，时钟进入不可信冻结状态。重新选择窗口创建新会话。

## 视觉观测

每帧先按有效游戏视口换算到 1920×1080 参考坐标，再根据 `game_ui_scale` 调整边缘 UI。视觉函数只返回：

```text
ObservedBattleState
cost_phase
cost_visible
cost_full
confidence
capture_timestamp
```

速度键与暂停键的亮度、边缘和连通段组合用于区分 1×、2×、0.2×、暂停、部署与方向调整；关卡标题画面和连续非战斗帧定义关卡边界。费用条沿多条相邻扫描线取中位数，降低压缩噪声影响。当前填充宽度按 `frames_per_cost` 映射到费用相位；只有高置信度的满到空变化才提交循环回绕。

## 权威时钟

权威值始终是 30 Hz 游戏逻辑帧：

- 等待状态不计时；第一帧可信运行状态进入关卡并建立 F0。
- 1×、2×、0.2×分别按 1、2、1/5 累加；暂停不累加。
- 费用相位可把墙钟外推校正到最近可信锚点，校正幅度超过容差时先扩大误差区间，不向后跳过已经触发的调度事件。
- 满费期间只有速度状态可信且观测未过期时外推。
- 未知、窗口失效或超过 250 ms 没有新观测时冻结。
- 连续 30 个非战斗观测确认离关，帧和调度触发集合归零，轴保持不变。

设置的 `frames_per_cost` 只负责费用映射和显示：

```text
logical_seconds = game_frame / frames_per_cost
subframe = game_frame % frames_per_cost
display = MM:SS:subframe/frames_per_cost
```

AxisLink 继续保存原始 30 Hz `game_frame`。

## 清空轴

`request_clear_axis` 在非空轴上第一次调用只返回带截止时间的 `clearPending` 快照；3 秒内第二次调用清空 `events`、重置创建序号和调度集合，保留标题与 `stageId`。空轴调用直接成功。按钮和全局 F4 共用该命令。

## 录屏源

`analyze_recording` 先用 `ffprobe` 验证单视频流、尺寸、帧率和时长，再启动 `ffmpeg` 解码为 30 Hz BGRA 原始帧。解码线程按帧序号生成确定性时间戳，不使用处理耗时。

每帧经过同一视觉和时钟状态机，结果压缩为状态发生变化或游戏帧变化时的 `RecordingTracePoint`。UI 展示进度、检测到的关卡区段和定位滑块。轨迹与录屏路径只存在内存；取消、失败和完成均不得改写轴。

首版接受恒定帧率 MKV/MP4。检测到不可用帧率、解码短读或尺寸变化时停止并返回结构化错误。

## UI

默认窗口 960×300，最小 760×260，不限制最大宽高。标题栏增加“监控”和主题入口；计时条显示来源、战斗状态、可信度、`MM:SS:FF/N` 与绝对 F 值。时间轴填充剩余高度。

字体在现有基础上增加约 2–3 px。颜色全部通过 CSS 变量定义，根元素使用 `data-theme="dark|light"` 切换，不维护两份样式表。

## 验证

- 视觉纯函数使用合成 BGRA 缓冲区和从 OBS 录屏提取的小型 ROI PNG。
- 时钟测试使用显式纳秒时间戳，不等待现实时间。
- 录屏测试只验证元数据解析、命令错误与短小夹具，不提交原始录像。
- Windows 人工验收覆盖候选窗口、WGC 生命周期、失焦 F1–F4、主题持久化和真实关卡边界。
