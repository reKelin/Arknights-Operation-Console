<div align="center">
  <h1>Arknights Operation Runner</h1>
  <p>面向明日方舟实机计时、作战轴记录与执行的轻量桌面工具。</p>
  <p><strong>开发中</strong></p>
</div>

当前交互 Demo 包含：

- 自动模拟关卡时钟和下一操作倒计时；
- 可横向滚动、缩放和拖动的单轨时间轴；
- 全局 F1/F2/F3 实时记录部署、技能和撤退；
- 操作点执行参数编辑；
- AxisLink JSON 导入导出；
- 提示、到点暂停和执行预演。

Demo 不捕获或操作游戏客户端，不读取游戏内存。

## 开发

需要 Node.js 22.16.0、Rust 1.98.1、WebView2 和 Visual Studio C++ Build Tools。

```powershell
npm ci
npm run bindings
npm run tauri dev
```

格式、类型、测试和 Windows 构建由 GitHub Actions 执行。

## 许可证

Apache-2.0
