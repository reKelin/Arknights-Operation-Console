---
status: approved
scope: console-workflow
depends_on:
  - specs/console-ui/requirements.md
  - specs/recording-candidates/requirements.md
  - specs/recording-continuation/requirements.md
---

# 原型工作流重构

- REQ-WORKFLOW-001：工作台必须按用户提供的 `console-workflow.html` 排布标题栏、三模式栏、主操作区和单轨时间轴。
  - AC-WORKFLOW-001：1100×280 和 860×280 下使用 40 px 标题栏、46 px 模式栏、108 px 主区、20 px 轴标题和 64 px 时间轴；深浅主题、SVG 标点、蓝色强调与黄色当前位置对应原型。
- REQ-WORKFLOW-002：H 必须直接打开表格轴编辑页；编辑页提供搜索、筛选、多选移除、批量改时、同帧排序和底部参数编辑。
  - AC-WORKFLOW-002：编辑调用现有 Rust 命令；有效字段失焦即更新会话草稿，未知时间仍显式确认，旧版本继续只读。
- REQ-WORKFLOW-003：录屏分析必须默认将已识别字段填入轴，只有不确定内容要求人工校对，不增加整批人工确认门槛。
  - AC-WORKFLOW-003：完成分析后自动生成首个有候选区段的草稿；已知字段保留，未知字段留空，区间时间或不可信时间保持未确认。重复快照不重复创建版本；H 直接编辑该草稿。
- REQ-WORKFLOW-004：多区段、来源去重、旧轴和接管前缀必须保留既有边界。
  - AC-WORKFLOW-004：切换区段按分析 ID 和区段查找或创建草稿；不同区段不混轴；分析草稿不覆盖父版本；接续仍经已有共同锚点和冲突确认，不能自动猜测偏移。未完成参数和时间继续阻止导出与代理。

## 证据边界

用户在本任务中明确要求默认自动补全、不确定内容才人工校对。现有识别器主要从状态转换提取候选，目前不会凭空识别干员、格子或把未知交互判为技能。此重构消费识别器实际提供的证据，不宣称新增了视觉识别模型。
