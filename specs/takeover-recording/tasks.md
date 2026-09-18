---
status: approved
scope: takeover-recording
depends_on:
  - specs/takeover-recording/requirements.md
  - specs/takeover-recording/design.md
---

# 接管续录任务

1. 定义 `OperationSession`、`AxisRevision`、版本来源和 `TakeoverState`，实现基于 D 回执的纯前缀构造与异常拒绝。（REQ-TAKEOVER-003、REQ-TAKEOVER-004、REQ-TAKEOVER-008）
2. 集成 D 的取消、输入释放、暂停证明和不确定回执人工确认；K 只在代理模式和游戏前台有效。（REQ-TAKEOVER-001、REQ-TAKEOVER-002、REQ-TAKEOVER-008）
3. 接入 ProxyClock → HumanClock 可信锚点交接；未知状态禁止 P 和再次代理。（REQ-TAKEOVER-005）
4. 将 Runner 当前轴迁入会话版本，保证旧版不可变、P 续录落入新版且事件 ID 稳定。（REQ-TAKEOVER-003、REQ-TAKEOVER-006）
5. 增加版本切换、指定版本导出、下一局 F0 武装和仅停止命令。（REQ-TAKEOVER-003、REQ-TAKEOVER-007）
6. 更新 Console 代理与编辑界面，接入键位确认、回执待确认、接管状态和版本选择，移除暂时禁用的代理占位。（REQ-TAKEOVER-001 至 REQ-TAKEOVER-008）
7. 使用现有脚本生成绑定并执行允许的静态检查；Windows 全局 K、输入释放、暂停证明和真实游戏循环留给 CI 与实机验收。

