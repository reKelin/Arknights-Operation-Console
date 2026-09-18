---
status: implemented
scope: trusted-clock-observation
depends_on:
  - specs/trusted-clock-observation/requirements.md
  - specs/trusted-clock-observation/design.md
---

# 可信观测与双时钟任务

1. 用有界 FIFO 和序号 envelope 替换 latest slot，补充溢出计数检查。
2. 实现 `ClockSnapshot`、`ClockQuality`、`HumanClock`、`ProxyClock` 和可信交接，保留录屏调用方的短期兼容入口。
3. Runner 消费 envelope 的连续性信息，丢失或过期时冻结并禁止代理执行。
4. WGC 优先无边框并只对边框兼容错误回退，快照保留警告。
5. 生成 TypeScript 绑定，运行 rustfmt、Clippy、Rust 测试、TypeScript 和漂移检查。
