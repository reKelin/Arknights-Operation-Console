<div align="center">
  <h1>Arknights Operation Console</h1>
  <p>面向明日方舟实机计时、作战轴记录与执行的轻量桌面工具。</p>
  <p><strong>实机监控、手动录轴与录屏时钟已接入；安全代理流程尚未开放</strong></p>
</div>

当前 Console 包含：

- 人工录轴、录屏分析和代理指挥三种紧凑工作模式；
- 可自由调整尺寸的深色/浅色界面，以及独立设置和轴编辑页面；
- 可横向滚动、缩放和拖动的单轨时间轴；
- 游戏前台按 P 记录待分类操作，Console 前台按 H 整理、Ctrl+S 导出；
- 操作点参数、时间与备注编辑；
- AxisLink v2 JSON 导入导出，使用 A1–I36 格子短代码；
- 提示和录屏时钟轨迹分析；
- 40 帧到 10 分钟视野、Alt+滚轮缩放、滚轮横向滚动和同帧操作堆叠。

当前版本支持 `Arknights.exe` 窗口选择、Windows Graphics Capture、视觉状态/费用同步，
MKV/MP4 录屏时钟轨迹分析，以及进关标题的关卡代码和中文名 OCR。自动识别不唯一时
可以从内置目录手动确认；地图按需下载并用于坐标边界校验。项目不读取游戏内存。

仓库已有实验性触摸执行代码，但 Console 暂不开放代理入口。完成暂停执行事务、键位确认、
执行回执和接管续录后才会开放安全代理流程。

## 开发

需要 Node.js 22.16.0、Rust 1.98.1、WebView2 和 Visual Studio C++ Build
Tools。录屏分析还需要 PATH 中可用的 `ffmpeg` 和 `ffprobe`。自动识关使用 Windows
简体中文 OCR 语言功能；该功能不可用时仍可手动选择关卡。

```powershell
npm ci
npm run bindings
npm run app:dev
```

按需编译本机调试 app：

```powershell
npm run app:build:local
```

Windows 产物为 `src-tauri/target/debug/arknights-operation-console.exe`，不生成安装包。

关卡目录与地图来自固定版本的
[Kengxxiao/ArknightsGameData](https://github.com/Kengxxiao/ArknightsGameData)，
来源提交记录在 `src-tauri/data/stages.json`。

## 许可证

Apache-2.0
