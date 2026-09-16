---
status: implemented
scope: axislink-v2
depends_on:
  - docs/product/requirements.md
---

# AxisLink v2 与关卡识别需求

- REQ-AXISV2-001：Runner 必须只导入和导出 AxisLink v2。
  - AC-AXISV2-001：导出文件固定使用 `schemaVersion = 2`；导入 v1 返回 `unsupported_version`，不迁移事件。
- REQ-AXISV2-002：操作点必须使用面向玩家的格子短代码。
  - AC-AXISV2-002：格子格式限定为 A1 到 I36；字母从下到上，数字从左到右。
- REQ-AXISV2-003：三种操作必须使用正确的执行语义。
  - AC-AXISV2-003：部署包含干员、格子和朝向；技能与撤退只包含格子，不包含干员或朝向。
- REQ-AXISV2-004：Runner 必须提供可搜索的关卡目录和手动选择。
  - AC-AXISV2-004：目录包含普通与集成战略关卡的 `stageId`、代码、名称和地图路径；未知导入 ID 保持可见。
- REQ-AXISV2-005：Runner 必须从进关标题画面自动识别关卡。
  - AC-AXISV2-005：Windows OCR 同时读取关卡代码与中文名；仅唯一匹配或普通/突袭成对时自动确定，否则提供过滤后的手动候选。
- REQ-AXISV2-006：关卡 OCR 失败不得破坏游戏时钟。
  - AC-AXISV2-006：OCR 缺少语言、包身份、识别失败或无匹配时，计时继续，关卡状态标记为未确认并允许手动选择。
- REQ-AXISV2-007：轴关卡与观测关卡必须保持独立并阻止错关调度。
  - AC-AXISV2-007：OCR 不覆盖已有 `stageId`；不一致时禁止录轴和调度，恢复一致后不得补发错关期间已经到期的事件。
- REQ-AXISV2-008：地图必须按固定数据版本加载并用于坐标校验。
  - AC-AXISV2-008：目录固定到明确的 ArknightsGameData 提交；地图按需下载并缓存，已加载地图上的格子必须在实际宽高范围内。
- REQ-AXISV2-009：录屏分析必须保留每个区段的关卡识别结果。
  - AC-AXISV2-009：每个关卡区段独立保存 OCR 原文、匹配状态和候选，不修改当前轴。
- REQ-AXISV2-010：自动验证必须覆盖协议、坐标、匹配和错关安全。
  - AC-AXISV2-010：Windows CI 覆盖 v2 往返、拒绝 v1、短代码边界、关卡匹配歧义、地图边界以及错关不补跑。
