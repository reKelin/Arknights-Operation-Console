---
status: approved
scope: axislink-v2
depends_on:
  - specs/axislink-v2/requirements.md
---

# AxisLink v2 与关卡识别设计

## 协议与草稿

`protocol/axislink.schema.json` 是 AxisLink v2 的唯一规范源。格子是 `^[A-I](?:[1-9]|[12][0-9]|3[0-6])$` 字符串；A 表示最下行，1 表示最左列。部署要求 `operator`、`tile`、`direction`，技能和撤退只要求 `tile`。

Rust 草稿继续允许执行参数暂缺。导出前按事件类型检查完整性；导入先检查版本并明确拒绝 v1，再执行严格字段校验。地图 JSON 的第一行是最上行，因此短代码的映射为：

```text
column = number - 1
row_from_bottom = letter - A
json_row = map_height - 1 - row_from_bottom
```

## 关卡目录与地图

`scripts/sync-stage-catalog.mjs` 从固定提交的 `stage_table.json` 和 `roguelike_topic_table.json` 生成 `src-tauri/data/stages.json`。运行时目录只读；地图按 `levelId` 的小写安全路径从同一提交下载到 Tauri 缓存目录，并在完整解析后原子替换缓存。

地图只保留当前阶段需要的矩形网格、尺寸和格子属性。不渲染地图，不把敌人、波次和路线引入 Runner。目录中存在但上游缺少地图的关卡返回 `map_unavailable`，仍允许手动选作轴关卡。

## OCR 与匹配

窗口和录屏在 `BattleBegin` 状态裁取标题区域。单个 Windows OCR 工作线程使用 `zh-CN` 同时读取拉丁关卡代码和中文关卡名，不阻塞 WGC 回调。OCR 结果按空白、大小写、全角字符和横线规范化后同时匹配目录的 `code` 与 `name`。

唯一匹配直接确认；只差 `#f#` 的普通/突袭对默认普通关并保留候选；其他歧义、只识别到一项或无结果均保持未确认。识别结果在当前窗口会话和每个录屏区段内粘住，离关或换源后清除。OCR 错误是非致命关卡警告，不改变视觉时钟可信度。

## 关卡安全

轴的 `stageId` 是期望关卡，OCR 或手动选择产生观测关卡。已有期望关卡不会被 OCR 覆盖。两者不一致时：

- 权威计时继续；
- 禁止新增实时操作点；
- 不发送提醒、暂停请求或预演执行；
- 把期间已经到期的事件标记为已处理，恢复一致时不得补发。

录屏识别只写入 `RecordingSegment`，不参与当前轴调度。

## UI

轴属性和监控菜单复用可搜索关卡选择器。操作点编辑器对三种事件都显示短代码；只对部署显示干员与朝向。状态栏显示 OCR 原文、关卡匹配状态和轴/观测一致性；未知导入 `stageId` 继续显示但标记为目录外。

## 依赖与验证

Windows OCR 直接使用与 `windows-capture` 一致的 `windows 0.62.2`。地图请求使用已存在于依赖图中的 `reqwest 0.13.5`。CI 重新生成目录和 TypeScript 绑定并检查漂移；真实 OCR 与 WGC 生命周期保留人工验收，纯匹配、坐标和安全状态机使用确定性测试。
