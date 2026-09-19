---
status: implemented
scope: interactive-demo
depends_on:
  - specs/interactive-demo/requirements.md
---

# 交互 Demo 设计

> 本文中的 AxisLink v1 定义已由 `specs/axislink-v2/` 取代，不再受支持；其余内容保留为 Demo 历史设计。

## 文件边界

- `protocol/axislink.schema.json`：AxisLink v1 唯一规范源。
- `src-tauri/src/axis.rs`：使用 `typify` 从 Schema 编译 Rust 类型，并定义允许缺少执行参数的内部草稿类型。
- `src-tauri/src/runner.rs`：模拟关卡时钟、轴状态、录轴和调度。
- `src-tauri/src/bindings.rs`：Tauri 请求、快照、结构化错误和 `tauri-specta` 导出入口。
- `src/App.tsx`：唯一窗口的界面组合和临时表单状态。
- `src/Timeline.tsx`：时间轴显示、滚动、缩放和指针交互。
- `src/generated/`：生成文件，不得手工修改。

没有第二个实现前不创建 clock、storage 或 executor trait。

## AxisLink v1

根对象包含：

- `schemaVersion = 1`
- `title`
- `stageId`
- `timebase.fps = 30`
- `events`

所有对象设置 `additionalProperties: false`。公共事件字段为：

- `id`：1–64 个字符的稳定字符串；
- `frame`：大于等于 0 的整数；
- `kind`：`deploy | skill | retreat`；
- `operator`：ArknightsGameData `character_table.json` 中的角色键，例如 `char_002_amiya`；
- `label`：可选的 0–120 字符说明。

`stageId` 使用 ArknightsGameData 关卡数据中的稳定关卡 ID。部署事件额外要求 `tile: { x, y }`：原点是关卡二维格子数组左上角，`x` 向右、`y` 向下，均为大于等于 0 的整数；`direction` 固定为 `up | right | down | left`。技能和撤退事件没有额外字段。

同帧事件保持数组原有顺序。导入时拒绝未知版本、未知或额外字段、负帧、重复 ID 和缺失载荷。

## 编辑草稿

F1/F2/F3 和双击新增时无法立即知道全部执行参数，因此 Console 内部保存 `DraftAxis` / `DraftEvent`。双击时间轴先打开三项类型选择，用户选择部署、技能或撤退后才创建草稿：

- `id`、`frame`、`kind` 必填；
- `stageId` 在草稿轴中允许为空；
- `operator`、部署格和朝向允许为空；
- `label` 可选；
- `isComplete` 由 Rust 根据事件类型计算，不单独持久化。

时间轴可以展示不完整草稿，但导出前必须逐项转换为严格 `AxisDocument`；存在不完整项时返回包含事件 ID 和缺失字段的结构化错误。

## Rust 状态

`RunnerState` 直接保存在 Tauri managed state 中：

- 当前 `DraftAxis`
- `MockBattleClock`
- 是否录轴
- 时间轴创建序号
- 运行策略
- 当前已触发事件 ID

后台线程每 16 ms 更新一次模拟源，但逻辑帧由 `Instant` 差值计算，不依赖线程唤醒次数。模拟关卡自动循环：

1. 启动后短暂等待；
2. 进入 1× 战斗并从 0 帧开始；
3. 中途切换一次 2×；
4. 到达预定结束帧后离关；
5. 等待后开始下一轮。

正式 UI 不提供时钟启停按钮。调度器跨过多个帧时必须触发区间内每个未触发事件，不能只比较相等。

## Tauri 边界

命令保持最少：

- 获取当前快照；
- 设置草稿轴的标题和 `stageId`；
- 设置录轴状态；
- 按类型记录当前操作；
- 替换、移动或删除操作点；
- 导入、导出 AxisLink；
- 继续已暂停的模拟关卡；
- 设置运行策略和窗口置顶。

Rust 定期向前端发送快照事件。TypeScript 调用只经过生成绑定，不直接写字符串命令名。

类型生成使用固定版本的 `typify` 与 `tauri-specta 2.0.0-rc.25`：AxisLink Rust 类型来自 JSON Schema，命令和事件由 `tauri-specta` 生成可调用的 TypeScript 绑定。CI 重新生成后执行 diff 检查。

## UI

窗口结构固定为标题栏、72 px 计时信息带和 132 px 时间轴：

- 计时区横向显示时间、逻辑帧、速度、状态、误差和下一操作倒计时。
- 时间轴头显示文件名、录轴开关、F1/F2/F3、导入导出和缩放。
- 轨道内容宽于视口并横向滚动；已走部分使用强调色。
- 双击轨道空白处新增操作点；拖动标点调整帧；右键打开紧凑编辑菜单。
- 底部条显示选中操作点摘要。

标题栏“轴”菜单放置“轴属性”和运行策略；“轴属性”紧凑弹层用于填写标题和必需的 `stageId`。“视图”菜单放置置顶开关。模拟暂停时，计时信息带临时显示“继续模拟”按钮。窗口默认置顶；最小化时隐藏并保留托盘图标；关闭事件直接退出。

## 导入导出

使用 Tauri 原生文件对话框。Rust 读取、验证并整体替换当前轴；导出使用格式化 JSON。首版不自动保存、不维护最近文件和历史版本。

## 提醒预演

运行策略只存在于 Console 状态：

- `notify`：到达提前量时由前端 Web Audio 播放提示。
- `pause`：到点将模拟时钟置为暂停；用户点击计时信息带中的“继续模拟”后恢复。
- `dryRun`：到点生成预演结果，不发送系统输入。

新一轮关卡开始时清空已触发集合，并把调度器上一帧设为 `-1`，确保 0 帧事件能够触发。活动关卡中的任何新增、替换、移动、删除或导入完成后，按当前帧重建已触发集合：`frame <= currentFrame` 的事件视为已处理，其余事件待触发。这样过去事件移到未来后可再次触发，未来事件移入过去时不会立即执行。

## 错误处理

所有命令返回 `CommandError { code, message, field }`。文件路径、JSON 和 Schema 是信任边界；错误显示中文，日志不记录完整用户文件内容。
