---
status: implemented
scope: takeover-recording
depends_on:
  - specs/manual-recording/requirements.md
  - specs/paused-execution/requirements.md
---

# 接管续录需求

- REQ-TAKEOVER-001：选定游戏窗口位于前台且 Console 处于代理模式时，K 必须立即取消当前代理事务并释放全部活动输入。
  - AC-TAKEOVER-001：非代理模式、未选窗口或游戏不在前台时 K 不创建版本；有效 K 阻止后续输入，键和触点释放完成后才进入接管整理。
- REQ-TAKEOVER-002：接管必须保持游戏暂停；无法从新鲜画面证明暂停时必须显示未知并禁止继续自动执行。
  - AC-TAKEOVER-002：接管结果只使用 D 的暂停证明和新鲜观测；不得把“取消成功”或旧暂停证明显示为已暂停。
- REQ-TAKEOVER-003：每次有效接管必须且只能创建一个本局新轴版本，并保留旧版本到本次应用会话结束。
  - AC-TAKEOVER-003：重复 K、界面停止或取消回调重入不重复创建版本；旧版和新版可切换并分别导出，不写工程文件且不自动保存。
- REQ-TAKEOVER-004：新版本的已执行前缀必须按本次代理运行的执行回执建立，不得按接管帧粗略切割。
  - AC-TAKEOVER-004：只消费与本次 `runId` 相同的回执，再按严格递增的 `receiptSequence` 和稳定 `eventId` 处理；`confirmed` 项进入前缀，同帧已确认项保留，`uncertain` 项进入待确认，`failed` / `cancelled` 及未执行尾部只留在旧版本。
- REQ-TAKEOVER-005：接管后的人工时钟必须继承可信锚点和本局逻辑帧，不得重新归零。
  - AC-TAKEOVER-005：只有 D/B 提供的可信交接锚点可启动续录；缺少可信锚点时版本仍可查看，但 P 续录和代理武装保持不可用并说明原因。
- REQ-TAKEOVER-006：接管后 P 必须向新版本续录，新操作沿用现有场次、时间证据和导出门禁。
  - AC-TAKEOVER-006：旧版本不因续录改变；新版中的待确认回执、未分类、缺参数、时间未知或错关操作继续阻止导出和代理。
- REQ-TAKEOVER-007：界面停止必须只停止代理，不创建新版本；“用于代理”必须只武装下一次进关 F0。
  - AC-TAKEOVER-007：停止后当前版本和模式不变；武装不追赶当前局，下一次可信进关才从 F0 调度，并可在开始前取消。
- REQ-TAKEOVER-008：结果不明的执行回执必须提供人工确认入口，且不得伪造游戏成功或观测时间。
  - AC-TAKEOVER-008：用户只可将 D 的 `uncertain` 回执解析为 `confirmed` 或 `failed`；确认沿用原 `eventId` / `receiptSequence`，不生成虚假 `observedFrame` 或来源时间戳。
