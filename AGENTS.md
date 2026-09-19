# AGENTS.md

界面、文档和必要注释使用简体中文，代码标识符使用英文。本文适用于整个仓库。

## 开发入口

- [README](README.md)：产品能力、环境与启动。
- [架构约束](docs/architecture.md)：术语、职责边界和必须保持的行为。
- [ADR 0001](docs/adr/0001-state-and-protocol.md)：Rust 权威状态与协议边界。
- [ADR 0002](docs/adr/0002-time-and-execution-evidence.md)：来源时间、双时钟与执行证据。

Graft 用于定位当前实现，架构约束与 ADR 用于判断修改是否符合设计。本机可用时，先用 `graft ask "<问题>" --source` 辅助定位；当前源码、测试和配置始终是实现事实的依据。大型代码变更后运行 `graft build`，图文件作为可再生本地缓存处理。

## 实现规则

- 使用现有 Tauri 2、React、TypeScript、Vite、Rust 2024 和 npm；根目录为前端，`src-tauri/` 是唯一 Rust crate。
- UI 使用普通 CSS/SVG 和 React 状态。按领域组织代码，保持依赖单向；优先标准库和已有依赖，有真实复用需求时再抽取共享代码。
- 名称表达领域含义和单位；函数职责单一，注释解释不明显的约束。
- 工具链版本以 `.node-version`、`rust-toolchain.toml` 为准，提交 npm/Cargo lockfile。
- 修改 Schema 或 Rust 命令类型后运行 `npm run bindings`，提交规范源与生成结果；生成文件不得手工修改。
- 更新关卡目录使用 `npm run stages:sync`，核对来源版本与生成 diff。
- `.local/` 存放本地缓存和制品，禁止提交；凭据、令牌和个人敏感信息不得写入源码、日志或文档。

## 本地验证

按修改范围选择检查；非平凡逻辑保留能在错误时失败的最小测试。

| 修改范围 | 命令 |
|---|---|
| 前端格式、类型与构建 | `npm run check`、`npm run build` |
| 前端纯逻辑 | `npm test` |
| UI 基本交互 | 首次运行 `npx playwright install chromium`，然后 `npm run check:smoke`、`npm run test:smoke` |
| CI 脚本与工作流 | `npm run test:ci` |
| Rust | 先 `npm run build`，再执行下方命令 |
| 原生调试应用 | `npm run app:build:local` |

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

UI 冒烟使用受控 Tauri IPC，不发送真实游戏输入。窗口、全局快捷键、WGC 捕获、时钟误差、代理接管、真实录屏识别和安装升级另做 Windows 实机验收，记录提交号、环境、步骤与结果。自动测试通过不代表实机行为已验收；只报告实际执行的检查。

## CI 与 Release

- [Smoke Test](.github/workflows/smoke-tests.yml) 用于 PR、main 推送、merge queue、手动运行及 Release 调用。合并前要求 `Static checks`、`Unit tests`、`Generated code drift`、`Rust tests`、`UI smoke tests` 和汇总 `Smoke Test` 通过。
- [Release](.github/workflows/release.yml) 仅接受 `v*` tag 或手动触发，私有仓库不运行。先复用同一提交的 Smoke Test，再在 Windows 构建安装包、检查原生启动与制品。
- tag 版本须与 npm、Cargo、Tauri 清单一致。tag 触发时发布已验证制品；手动运行只上传制品。发布前校验 SHA256SUMS，生产构建不作为 PR required check。
- 创建 PR 不等于授权合并、创建 tag 或发布；生产 Release 须有用户授权。按需构建本地调试应用。
- 数据下载的 GitHub Token 仅发送给 `api.github.com`。
- 当前环境提供 git-workflow 时遵循其规则；仅提交当前任务相关改动。

## 文档维护

只维护 README、本文、架构约束和少量 ADR。代码布局、符号和调用关系通过源码或 Graft 查询，不在文档中重复维护。

改变协议、持久化、时钟、输入授权或模块权威边界前，先更新架构约束；影响跨模块或长期取舍的重要决策才写 ADR，包含背景、决定、代价和已否决/被替代方案。ADR 不猜测历史动机；完成任务清单、实现流水账和过期方案由 Git 保存。
