---
status: approved
scope: game-monitoring
depends_on:
  - specs/game-monitoring/requirements.md
  - specs/game-monitoring/design.md
---

# 游戏监控与同步任务

1. 更新产品约束、窗口尺寸和阶段路线，固定参考项目提交与独立实现边界。（REQ-MONITOR-002、REQ-MONITOR-004）
2. 将默认轴改为空轴，实现带二次确认的清空命令、按钮和全局 F4。（REQ-MONITOR-001）
3. 实现响应式大字号布局、深色/浅色 CSS 变量和最小应用设置持久化。（REQ-MONITOR-002、REQ-MONITOR-003、REQ-MONITOR-009）
4. 实现 `Arknights.exe` 候选窗口枚举、重新验证和 WGC 捕获生命周期。（REQ-MONITOR-004、REQ-MONITOR-012）
5. 从 OBS 样本确定参考 ROI，实现战斗状态、费用可见性、填充相位、满费和回绕的纯视觉函数。（REQ-MONITOR-005、REQ-MONITOR-006）
6. 实现 30 Hz 观测时钟：进关运行起表、倍率分段、暂停、费用锚定、满费外推、未知冻结和离关归零保轴。（REQ-MONITOR-007、REQ-MONITOR-008、REQ-MONITOR-009）
7. 扩展 Tauri 命令、快照事件和生成绑定，接入监控源、设置、逻辑秒分母和可信状态 UI。（REQ-MONITOR-003、REQ-MONITOR-004、REQ-MONITOR-007、REQ-MONITOR-009）
8. 实现 MKV/MP4 元数据探测、FFmpeg BGRA 解码、离线分析进度、轨迹压缩、取消和时间轴定位。（REQ-MONITOR-010、REQ-MONITOR-011、REQ-MONITOR-012）
9. 从 `V:\OBS` 派生最小 ROI 夹具，补充 Rust/Vitest 检查和 Windows 人工验收清单。（REQ-MONITOR-013）
10. 在 Windows GitHub Actions 验证生成绑定、Biome、TypeScript、Vitest、rustfmt、Clippy、Rust 测试和 release 构建。
11. CI 全绿后通过 merge commit 合入 `main`，将规范状态改为 `implemented` 并发布 `v0.2.0`。
