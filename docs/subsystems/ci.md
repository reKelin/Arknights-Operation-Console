---
status: implemented
scope: ci
depends_on: []
---

# CI、功能冒烟与本地验收

本页是当前测试与合并策略的规范来源。它取代旧文档中“不等待 CI 即合并”、
“前端不引入交互测试”及“只能使用 Windows CI 产物验收”的约定。
配置已实现不等于某个提交的 CI 或实机验收已经通过。

## 执行边界

| 入口 | 内容 | 门禁 |
|---|---|---|
| `.github/workflows/smoke-tests.yml` / `Smoke Test` | PR、main push、merge queue、手动入口；按测试类型并行执行静态检查、单元测试、生成代码漂移、Rust 测试和 UI 冒烟 | PR 当前待合并版本的 5 个测试 job 及汇总 job 必须成功 |
| `.github/workflows/release.yml` / `Release` | 仅 `v*` tag 或手动触发；复用同一提交的完整 Smoke Test，再进行生产构建和原生启动检查 | tag 发布只有在所有必需检查成功后执行；手动执行只产出构建制品，不公开 Release |
| 本地 app | `npm run app:dev` 或 `npm run app:build:local` | 调试、人工验收，不替代 PR 门禁，也不要求每个 PR 都构建 |

PR 不打安装包、不运行原生进程启动检查，也不自动运行发布流水线。
Rust 单元测试、rustfmt、Clippy、Tauri/AxisLink 绑定漂移和目录漂移都属于 Smoke Test，便于在合并前直接定位对应测试类型。
生产安装包只在 Release Pipeline 构建，避免把发布副作用混入普通 PR。

仓库为私有时，发布任务在分配 runner 前跳过；仓库已经公开时才允许 tag/手动执行。
重新变成公开仓库不会自动补跑历史任务。普通 PR 或 main push 均不触发重型流程。

发布安装包先在同一 Windows job 完成构建与启动检查，再上传带 commit 信息和 SHA256 校验的制品；
发布 job 只下载并发布这批制品，不重新编译。首先创建草稿 Release，上传成功后才公开。
已存在的 Release 不自动覆盖。手动发布入口是 Actions 的 Run workflow，不是预先点击“Publish release”。

## 基本功能冒烟

测试位于 `tests/smoke/ui/basic.spec.ts`，使用真实 React 页面、CSS 和生成的 Tauri 命令绑定。
`tests/smoke/desktopMock.ts` 仅替换 IPC、窗口尺寸调整和后端快照，不连接游戏，也不发送真实输入。
未声明的 IPC 调用、浏览器异常和界面错误会使测试失败；不能用默认返回成功的通用 mock 掩盖缺失功能。

| 功能 | 自动检查 | 尚需本地 Windows app 验证 |
|---|---|---|
| 下拉菜单 | 主题、轴版本、筛选、操作类型、方向、关卡搜索、录屏区段、生成方式、父版本和冲突决定；断言页面变化或最终请求内容 | 原生下拉弹层、遮挡与系统键盘操作 |
| 宽高响应 | 分别缩小、放大宽和高；检查主区域边界与主操作区随高度增长 | 拖动原生窗口边框、系统 DPI、多个屏幕 |
| 设置页滚动 | 缩小视口后用实际滚轮滚动，先确认底部控件进入视野；增高后检查不再溢出 | Windows WebView 滚轮与滚动条外观 |
| 可编辑的游戏执行键位 | 暂停、技能、撤退键的输入、失焦保存命令及返回设置后的回显 | 重启持久化、键名校验和真实键位行为 |
| 整理页窗口尺寸 | 进入时按需增高，离开时恢复进入前的宽和高，包含非默认尺寸和重复切换 | 真实 Tauri 窗口是否收到请求并完成调整 |
| 编辑及模式切换 | 三种模式切换、增加操作、修改保存、重新进入查看 | Rust 权威状态及真实录屏／游戏行为 |

录屏接续面板只展示当前选中的一个区段，因此通过上层区段菜单验证它的联动与请求参数，
不凭空创建界面没有提供的额外选项。

**已知功能缺口：P、H、Ctrl+S、K 当前是固定快捷键，设置页只展示，不能自行修改。**
暂停／技能／撤退键的编辑测试不能代替这些 Console 快捷键的可配置性测试。
本次 CI 改造没有扩展 Rust 设置结构或全局快捷键注册功能，也不宣称完成了该功能需求。

`smoke-desktop.ps1` 只验证生产二进制进程存活及存在可响应的原生窗口；
它不证明 WebView 内容正确、安装升级成功或 WGC/游戏输入可用。
使用隔离的临时配置目录，仅停止本次脚本启动的进程。

## 本地命令

```powershell
npm ci
npx playwright install chromium
npm run test:ci
npm run check
npx tsc -b
npm run check:smoke
npm test
npm run test:smoke

# 按需启动或编译本机调试 app，不生成安装包、不发布
npm run app:dev
npm run app:build:local
```

Windows 调试程序通常位于 `src-tauri/target/debug/arknights-operation-runner.exe`。
本地编译仍需要 README 中的 Rust、C++ Build Tools 和 WebView2；本机调试产物不等于通用便携安装包。

## 耗时与优化

各 Smoke Test job 和发布验证的参考时长只用于观察，不是测试断言或硬性时间目标。
各步骤耗时和退出码写入 `.local/ci-timings.jsonl` 并显示在命令日志中；Release validation 另在 Actions Summary 中报告趋势。
Release 历史按同一 workflow、事件、来源仓库及可比较分支筛选；tag 发布使用该发布 workflow 的历史版本任务。
同一任务最近最多五次成功运行用于观察中位数，最近连续三次超过参考值时给出优化提示。
单次偏慢、样本不足、API 暂不可用或 fork 权限不足，都不会改变功能测试结果。

参考时长使用 job 开始执行到结束的耗时，覆盖依赖准备，不包含该 job 首次分配 runner 前的排队。
本次运行的报告生成于 job 完成前，因此本次值是截至报告步骤的近似值；历史值使用完成后的完整耗时。
发布工作流还包含前置 Smoke Test 和最后的发布任务，应同时在 Actions 的整条运行时间中观察串行等待。

发现持续偏慢时先区分冷缓存、依赖安装、生成步骤、编译和具体慢用例，再优化。
不为缩短耗时移除必要测试，不用重试把偶发功能失败改成通过。PR 新提交会取消同一 PR 的旧运行。
workflow 的 45/120 分钟及用例的短超时只是防卡死保护，与性能参考值分开。

## 合并门禁配置

在 GitHub main 分支 ruleset / branch protection 中启用 Require status checks，
将 `Static checks`、`Unit tests`、`Generated code drift`、`Rust tests`、`UI smoke tests` 和汇总检查 `Smoke Test` 设为 required。
旧配置如要求 `windows`、旧 `CI` 或旧 `Smoke` 检查，需要替换，否则它们不再产生结果会阻止合并。
不要把 `Release validation` 设置成 PR required check。不添加路径过滤，不用跳过整个测试 job 来制造通过结果；
生成漂移或任一功能用例失败必须阻止合并。

**提交 workflow 文件不会自动修改 GitHub 的分支保护。** 应在第一条 Smoke Test 运行出现后确认 required check 名称；
未完成远端规则设置前，代码本身不能阻止维护者手动合并。
创建 PR 不等于获得自动合并授权；未获授权不合入 main。

## 验证记录范围

本次改造应分别记录脚本单元测试、工作流静态检查、锁文件检查、真实 Playwright 执行、
Windows 构建及人工验收结果。任何因依赖或运行环境不可用而未执行的项目，必须写成“未执行”，不能标为通过。
