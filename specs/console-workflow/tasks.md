---
status: approved
scope: console-workflow
depends_on:
  - specs/console-workflow/requirements.md
  - specs/console-workflow/design.md
---

# 实现与验证

- [ ] 按原型重排工作台、编辑页和设置；复用真实命令与快照。
- [x] 默认生成录屏草稿，保留不确定字段、时间及来源，接入区段选择。
- [x] 更新内部生成绑定，保留自动填充、重复快照和多区段最小 Rust 检查。
- [ ] 执行格式、类型和允许的基础检查；记录原型对照结果，不执行发布构建或启动冒烟。

绑定生成、rustfmt 与 Clippy（全部 targets）已通过；Rust 测试仅新增，按仓库规则由 CI 执行。界面原型对照随后续 UI PR 交付。
