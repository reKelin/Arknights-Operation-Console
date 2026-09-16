---
status: implemented
scope: interactive-demo
depends_on:
  - specs/interactive-demo/requirements.md
  - specs/interactive-demo/design.md
---

# 交互 Demo 任务

1. 初始化固定版本的 Tauri 2、React、TypeScript、Vite、Biome 和 Vitest 工程；配置窗口、置顶与托盘生命周期。（REQ-DEMO-002、REQ-DEMO-009）
2. 完整定义当时的 AxisLink v1 JSON Schema、示例轴和 `DraftAxis` 转换规则；该定义已由 AxisLink v2 取代。（REQ-DEMO-005、REQ-DEMO-006、REQ-DEMO-011）
3. 使用 `typify` 和 `tauri-specta` 生成 Rust/TypeScript 协议类型与可调用命令绑定。（REQ-DEMO-010）
4. 实现 Rust 自动模拟关卡时钟、RunnerState、稳定排序、区间调度和可恢复的模拟暂停。（REQ-DEMO-001、REQ-DEMO-007、REQ-DEMO-012）
5. 实现 Tauri 命令、快照事件、轴标题/`stageId` 编辑、录轴开关和全局 F1/F2/F3。（REQ-DEMO-004、REQ-DEMO-005、REQ-DEMO-006）
6. 实现紧凑计时器、“轴属性”与运行策略菜单、置顶菜单，以及可横向滚动、缩放、拖动、显示已走高亮的单轨时间轴。（REQ-DEMO-002、REQ-DEMO-003、REQ-DEMO-006、REQ-DEMO-007、REQ-DEMO-009）
7. 实现双击任意帧新增、右键补全执行参数和删除操作点。（REQ-DEMO-003、REQ-DEMO-005、REQ-DEMO-012）
8. 实现 AxisLink JSON 导入导出及不完整草稿阻断。（REQ-DEMO-006、REQ-DEMO-011）
9. 实现提示音、模拟暂停恢复和 dry-run 结果，确认不存在游戏集成依赖。（REQ-DEMO-007、REQ-DEMO-008）
10. 添加最小 Rust/Vitest 检查和 Windows GitHub Actions：自动覆盖 Schema、草稿转换、时钟、调度和时间轴纯逻辑；静态检查覆盖生成绑定与构建。
11. 为全局快捷键、托盘、关闭行为、窗口菜单和置顶切换保留非阻塞的 Windows 人工验收清单，明确这些 AC 不宣称由自动测试覆盖。
12. 确认 Windows CI 中的测试、静态检查和构建全部通过。
