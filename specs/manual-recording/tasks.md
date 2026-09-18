---
status: implemented
scope: manual-recording
depends_on:
  - specs/manual-recording/requirements.md
  - specs/manual-recording/design.md
---

# 手动录轴与整理任务

1. 接入可信时钟字段，定义事件时间证据、确认状态、录制场次和 ConsoleMode。（REQ-MANUAL-003、REQ-MANUAL-007、REQ-MANUAL-008）
2. 将全局快捷键收缩为游戏前台 P，移除 F1–F4/F12；限定 H 与 Ctrl+S 的 Console 焦点和文本输入作用域。（REQ-MANUAL-001、REQ-MANUAL-002）
3. 让 P 静默创建带时间证据和场次归属的待分类候选。（REQ-MANUAL-001、REQ-MANUAL-003、REQ-MANUAL-007）
4. 增加分类、时间校正与显式确认命令，更新独立整理页的场次筛选和内联校验。（REQ-MANUAL-004、REQ-MANUAL-006、REQ-MANUAL-007）
5. 统一导出和代理前的就绪检查，拒绝未分类、缺参数、时间未确认和错关状态。（REQ-MANUAL-005）
6. 使用现有生成脚本更新前端绑定，补充 Rust 状态转换检查；时间轴纯数学未变化，沿用既有 Vitest 用例。
7. 执行允许的静态检查；保留 Windows 全局快捷键、实机时钟与游戏前台验收给 CI 和后续实测。
