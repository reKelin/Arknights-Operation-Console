---
status: implemented
scope: ci
depends_on: []
---

# CI 与测试

## Smoke Test

`.github/workflows/smoke-tests.yml` 在以下情况运行：

- Pull Request；
- 推送到 `main`；
- merge queue；
- Actions 页面手动运行；
- Release Pipeline 调用。

工作流并行执行以下检查：

| Job | 内容 |
|---|---|
| `Static checks` | Biome 和 TypeScript |
| `Unit tests` | Vitest、CI 脚本和工作流契约测试 |
| `Generated code drift` | Tauri/AxisLink 绑定和关卡目录漂移 |
| `Rust tests` | rustfmt、Clippy 和 Rust 测试 |
| `UI smoke tests` | Playwright 基本交互测试 |
| `Smoke Test` | 汇总以上检查结果 |

UI 冒烟测试位于 `tests/smoke/ui/basic.spec.ts`。测试通过
`tests/smoke/desktopMock.ts` 提供受控的 Tauri IPC 和后端快照。

## 本地运行

```powershell
npm ci
npm run test:ci
npm run check
npx tsc -b
npm run check:smoke
npm test
npx playwright install chromium
npm run test:smoke
```

启动或编译本地调试应用：

```powershell
npm run app:dev
npm run app:build:local
```

调试程序位于 `src-tauri/target/debug/arknights-operation-runner.exe`。

## Release Pipeline

`.github/workflows/release.yml` 支持两种入口：

1. 推送 `v*` tag：运行 Smoke Test、Windows 生产构建、原生窗口启动检查和制品校验，然后发布 GitHub Release。
2. 在 Actions 页面手动运行：执行相同验证并上传构建制品，不发布 GitHub Release。

发布 tag 前，确保 `package.json` 中的版本与 tag 一致：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

构建制品包含安装包、commit 信息和 `SHA256SUMS`。

## 分支保护

在 `main` 的 ruleset 或 branch protection 中启用 Require status checks，并设置以下 required checks：

- `Static checks`
- `Unit tests`
- `Generated code drift`
- `Rust tests`
- `UI smoke tests`
- `Smoke Test`

`Release validation` 不作为 PR required check。
