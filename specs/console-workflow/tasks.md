---
status: implemented
scope: console-workflow
depends_on:
  - specs/console-workflow/requirements.md
  - specs/console-workflow/design.md
---

# 实现与验证

- [x] 按原型重排工作台、编辑页和设置；复用真实命令与快照。
- [x] 默认生成录屏草稿，保留不确定字段、时间及来源，接入区段选择。
- [x] 更新内部生成绑定，保留自动填充、重复快照和多区段最小 Rust 检查。
- [x] 执行格式、类型和允许的基础检查；记录原型对照结果，不执行发布构建或启动冒烟。

绑定生成、rustfmt、Clippy（全部 targets）、Biome 和 TypeScript 检查已通过。新增 Rust 自动填轴与时间轴刻度纯逻辑检查，按仓库规则未在本地运行测试。

浏览器模拟快照核对了 1100/860 宽度的三模式工作台、深浅主题、460 px 编辑页、设置及视频结果视图；主区行高为 40/46/108/20/64 px，无主区横向溢出和浏览器异常。时间轴按设计允许横向滚动。预览不连接游戏或生成真实输入，不替代 Windows 桌面窗口缩放、录屏识别和代理接管的实机验收；未执行发布构建或桌面启动冒烟。
