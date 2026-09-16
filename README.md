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
- AxisLink v2 JSON 导入导出，使用 A1–I36 格子短代码；
- 提示、到点暂停和执行预演。
- Alt+滚轮缩放、滚轮横向滚动和同帧操作堆叠；
- 实验性代理执行：头像识别、格子投影、合成触摸与 F12 急停。

当前版本支持 `Arknights.exe` 窗口选择、Windows Graphics Capture、视觉状态/费用同步，
MKV/MP4 录屏时钟轨迹分析，以及进关标题的关卡代码和中文名 OCR。自动识别不唯一时
可以从内置目录手动确认；地图按需下载并用于坐标边界校验。项目不读取游戏内存。

代理执行默认关闭，需二次确认，只使用 Windows `InjectTouchInput`。自动化输入可能存在
账号或反作弊风险；窗口、关卡或时钟不可信时会立即停止。

## 开发

需要 Node.js 22.16.0、Rust 1.98.1、WebView2 和 Visual Studio C++ Build
Tools。录屏分析还需要 PATH 中可用的 `ffmpeg` 和 `ffprobe`。自动识关使用 Windows
简体中文 OCR 语言功能；该功能不可用时仍可手动选择关卡。

```powershell
npm ci
npm run bindings
npm run tauri dev
```

格式、类型、测试和 Windows 构建由 GitHub Actions 执行。

关卡目录与地图来自固定版本的
[Kengxxiao/ArknightsGameData](https://github.com/Kengxxiao/ArknightsGameData)，
来源提交记录在 `src-tauri/data/stages.json`。

## 许可证

Apache-2.0
