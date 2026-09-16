---
status: implemented
scope: capture-bookmarks
depends_on:
  - specs/capture-bookmarks/requirements.md
---

# 全屏 OCR、窗口捕获与书签设计

## OCR

视觉层输出黑屏标题候选。OCR 使用完整 16:9 游戏视口，超过系统最大尺寸时等比缩小；会话聚合器对连续结果逐行去重，并持续用代码加中文名匹配目录。首次只读到 `OPERATION` 时继续采样，进入战斗后锁定结果。

## 窗口

窗口发现使用 `PROCESS_QUERY_LIMITED_INFORMATION` 查询完整路径，失败时只允许标题为“明日方舟”或含 `Arknights` 的可见顶层窗口。选择直接按 ID 还原 HWND。无标题栏缓冲失败时使用完整帧；连接阶段超过两秒没有首帧转为结构化错误。

## 时间轴与书签

缩放范围为 0.5–32，最高档每 10 帧一个刻度。当前进度、录屏状态和操作均为圆点；同帧操作只改变纵向位置。

书签是内部 `DraftKind::Bookmark`，有帧、顺序和标题，但不属于 AxisLink v2。P 创建书签，H 打开集中编辑窗口；转换成三种合法操作并补全参数后才可导出。批量平移使用有符号帧差并统一限制在合法帧范围。
