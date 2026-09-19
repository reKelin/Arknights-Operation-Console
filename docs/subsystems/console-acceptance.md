---
status: approved
scope: console-acceptance
depends_on:
  - docs/product/roadmap.md
  - specs/manual-recording/requirements.md
  - specs/proxy-execution/requirements.md
  - specs/paused-execution/requirements.md
  - specs/takeover-recording/requirements.md
  - specs/recording-candidates/requirements.md
  - specs/recording-continuation/requirements.md
---

# Console 集成验收记录

本记录描述当前代码能够证明的范围和正式交付仍需补充的证据。它不是发布记录，也不把未执行的检查标为通过。

## 两条攻略循环

| 流程 | 当前代码证据 | 当前结论 |
|---|---|---|
| 人工录轴 → 代理 → K 接管 → 人工续录 → 新轴代理 | P 记录写入当前实机场次；指定 revision 在下一局 F0 武装；执行回执按 `runId` 隔离；K 取消在途输入后按确认回执创建 takeover 子版本；后续 P 只修改活动子版本；新版本继续复用完整性和关卡门禁 | 调用链已静态接通；真实窗口、输入、暂停证明和完整循环未实机验收 |
| 录屏提取/接续 → 校对 → 新轴代理 | 原始 PTS 保留到候选；候选按 `recordingAnalysisId + segmentIndex + candidateId` 隔离；校对结果先暂存；新建录屏轴使用空基轴，接续模式保留接管前缀；确认偏移和冲突后原子创建 `recordingMerge` 子版本；新版本复用导出和武装门禁 | 确定性逻辑和调用链已静态接通；真实素材识别准确率、时间对齐和新轴实机代理未验收 |

## 静态核对结果

- AxisLink v2 Schema 未加入会话版本、候选或回执字段；这些数据只存在于 Rust/Tauri 内部状态。
- 录屏区段不会伪造成实机 `RecordingAttempt`；接续只接受带真实场次的 takeover/recordingMerge 父版本，新建录屏轴不要求实机场次。
- 接管前缀由当前 `runId` 的有序执行回执决定，不按帧粗略截断；未确认回执继续阻止导出和武装。
- 录屏合并按完整来源身份去重；同帧不同来源保持稳定顺序，同格同类冲突必须人工保留或排除。
- 停止代理不会创建 revision；只有 K 接管收尾或已确认录屏合并创建子版本。
- 生成绑定来自 Rust 类型；PR Smoke Test 与发布验证分离，检查范围及本地验收边界见 [CI 规范](ci.md)。Tauri 绑定漂移、rustfmt、Clippy 和 Rust 测试属于 PR Smoke Test，Release Pipeline 复用这些门禁后再构建。
- Console 更名保持旧 WiX UpgradeCode；NSIS 仅在默认当前用户安装路径和旧卸载信息一致时迁移 Runner，并在安装成功后清理旧项。

## 未验收项

- 官方 PC 客户端中的 WGC 无黄框优先捕获及兼容回退。
- 倍速、减速、暂停、满费、费用锚点恢复、丢帧、离关和人工/代理时钟交接的实机误差。
- 三类暂停输入、同帧连续部署、目标帧跨越、失焦、过期画面、尺寸变化和取消释放。
- 录屏多区段、不同帧率、缺片段、战斗中途开始、重复操作及接续对齐的真实素材对照准确率。
- 原型重构的浏览器模拟快照已核对 1100/860 宽度、深浅主题、编辑页与视频结果视图；真实 Tauri 窗口缩放、系统 DPI、密集标点和长文本仍需 Windows 实机验收。
- 旧 Runner 到 Console 的 MSI/NSIS 实际安装升级、配置继承、release 构建和启动验收。
