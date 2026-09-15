<div align="center">
  <h1>Arknights Operation Runner</h1>
  <p>面向明日方舟实机计时、作战轴记录与执行的轻量桌面工具。</p>
  <p><strong>实机监控与录屏时钟已接入；真实输入执行尚未接入</strong></p>
</div>

当前交互 Demo 包含：

- 空白轴启动、F4 二次确认清空；
- 可自由调整尺寸的大字号深色/浅色界面；
- 可横向滚动、缩放和拖动的单轨时间轴；
- 全局 F1/F2/F3 实时记录部署、技能和撤退；
- 操作点执行参数编辑；
- AxisLink JSON 导入导出；
- 提示、到点暂停和执行预演。

当前版本支持 `Arknights.exe` 窗口选择、Windows Graphics Capture、视觉状态/费用同步，
以及 MKV/MP4 录屏时钟轨迹分析。项目不读取游戏内存。

## 开发

需要 Node.js 22.16.0、Rust 1.98.1、WebView2 和 Visual Studio C++ Build
Tools。录屏分析还需要 PATH 中可用的 `ffmpeg` 和 `ffprobe`。

```powershell
npm ci
npm run bindings
npm run tauri dev
```

格式、类型、测试和 Windows 构建由 GitHub Actions 执行。

## 许可证

Apache-2.0
