# AGENTS.md

本文件适用于整个仓库。面向用户的界面和文档使用简体中文；代码标识符使用英文；注释只解释不明显的约束，默认使用中文。

## 产品与技术边界

- 本仓库只实现 Arknights Operation Runner；Arknights Operation Studio 是外部系统。
- 首版仅支持 Windows 10/11 和明日方舟官方 PC 客户端，不实现模拟器或多设备适配。
- Runner 是轻量计时与作战轴工具，不渲染游戏画面、关卡地图或战斗模拟。
- 技术基线为 Tauri 2、React、TypeScript、Vite、Rust 2024 和 npm。
- 使用官方单应用布局：根目录承载 React，`src-tauri/` 是唯一 Rust crate。只有出现真实的第二个二进制或独立领域边界时才建立 Cargo workspace。
- bundle identifier 固定为 `io.github.kelin.arknights-operation-runner`。
- UI 使用普通 CSS/SVG，不引入 Tailwind 或组件库。前端状态优先使用 React 自带的 state、useReducer 和必要的 Context，不预装状态管理库。
- 仓库使用 Apache-2.0 许可证。

## 项目布局

- `src/`：React/TypeScript 界面。
- `src-tauri/`：Tauri 配置、Rust 权威状态与 Windows 集成。
- `protocol/`：AxisLink 等跨进程或跨仓库协议的 JSON Schema。
- `docs/product/`：产品级 requirements、architecture 和 roadmap。
- `docs/subsystems/`：只在子系统进入设计或开发时创建对应文档。
- `specs/<kebab-case-feature>/`：需要正式设计的功能规范。
- `.local/`：本地缓存和开发制品，禁止提交。

不要预建空模块、空 crate、占位接口或“以后可能用到”的目录。

## 核心公约

- 权威游戏逻辑时钟固定为 30 tick/s，并与 UI 渲染频率分离。
- 计时与关卡状态绑定：识别到进入关卡后自动归零并开始，暂停和倍速跟随游戏状态，离开关卡后结束；正式 UI 不提供手动开始或停止计时。
- Rust 持有时钟、调度、运行状态和真实副作用；React 发送命令并展示不可变快照或增量，不实现第二套权威调度器。
- AxisLink v2 的事件类型只表示玩家可执行的 `deploy`、`skill`、`retreat`。部署保存干员、格子与朝向；技能和撤退只保存目标格子。格子使用 A1（左下）到 I36（右上）的短代码。提醒、到点暂停和自动执行属于 Runner 运行策略，不写入轴事件。
- AxisLink 使用 JSON，JSON Schema 是唯一规范源并生成 Rust/TypeScript 类型。Tauri 内部命令以 Rust 类型为源生成 TypeScript 绑定，不套用 AxisLink。
- 实时录轴默认使用全局 F1、F2、F3 分别记录部署、技能、撤退。
- 选定游戏窗口位于前台时，P 记录待分类书签，H 打开书签列表；书签不属于 AxisLink，转换为合法操作前不得导出。
- 操作点在时间轴上以标点展示；执行参数保存在标点数据中，通过右键编辑，不增加常驻侧栏。
- 真实输入默认关闭，必须显式启用代理执行。窗口、时钟或状态不可信以及急停触发时立即停止，不盲目补发输入。
- 代理执行使用全局 F12 急停；只允许通过 Windows `InjectTouchInput` 和窗口坐标 API 产生输入，不读取游戏内存或注入游戏代码。

## UI 约束

- 只有一个默认约 1100×420 的紧凑窗口；宽高均可调整，最小约 860×340。
- 默认置顶，允许从“视图”菜单关闭置顶。
- 最小化进入托盘，关闭窗口直接退出。
- 界面只保留标题栏、计时器、下一操作、录轴控制和单轨时间轴；不添加游戏画面、侧栏、操作队列或独立浮窗。
- 时间轴支持横向滚动和缩放；当前帧之前的轨道与操作点必须高亮。
- 界面支持深色与浅色主题；主题和监控设置可以持久化，轴草稿仍不得自动保存。

## 依赖与生成代码

- 前端只使用 npm，并提交 `package-lock.json`；Rust 应用提交 `Cargo.lock`。
- 初始化应用时固定当时的 Node LTS 和 Rust stable 具体版本，分别写入 `.node-version` 和 `rust-toolchain.toml`。
- 优先使用标准库和已有依赖；没有第二个实现时不创建接口、工厂或兼容层。
- 生成文件必须由单一规范源产生并在 CI 检查漂移，不得手工修改。

## 可读性与功能模块化

- 命名必须表达领域含义和单位，避免无上下文缩写、含糊名称以及散落的魔法数字；代码结构应优先做到自解释，注释只说明无法由代码表达的原因、约束和取舍。
- 函数和组件应该只承担一个可描述的职责；优先使用提前返回和直线式流程，避免深层嵌套、隐式状态变化及同时混合解析、业务决策与副作用。
- 代码必须按功能或领域能力保持内聚，同一功能的状态、纯逻辑、界面和必要测试放在易于共同定位的位置；跨功能依赖通过最小且明确的公开入口，不读取其他模块内部实现。
- 模块依赖必须保持单向。共享代码只在存在真实的多个调用方且语义一致时提取，不建立无边界的 `utils`、`common` 或通用服务容器。
- 模块化以职责边界和可独立验证为依据，不以文件行数为依据；没有真实复用或独立变化原因时，不拆分文件、不增加抽象层。
- 修改既有功能时应遵循周边代码的命名与组织方式；如果局部结构已经妨碍理解，应在当前需求范围内完成最小必要整理，不顺带进行无关重构。

## 测试与验证

- Agent Hub 不在本地运行测试；默认由 GitHub Actions 的 Windows runner 执行验证。
- 开发过程中只保留可快速验证的基础检查：生成文件漂移、Biome、`tsc`、rustfmt、Clippy 和 Rust 测试；不得执行 `tauri build --release` 或启动冒烟。正式发布时才执行 release 构建与启动冒烟。
- 前端首版不引入 React Testing Library 或端到端测试框架；Vitest 只覆盖时间轴帧/像素换算、滚动范围和拖动落点等纯逻辑。
- 非平凡的分支、循环、解析器和状态转换至少保留一个能在错误时失败的最小检查。
- 快速开发阶段不要求独立 Agent 审查。创建 PR 并确认可以合并后立即使用 squash merge 合入 `main`，不等待完整 CI。
- 不等待任何 CI 流程；CI 始终在后台运行，不主动 watch、poll 或阻塞等待，收到失败结果后再定位、修复并提交。开发速度优先。
- 只报告实际执行过的检查；失败时修复原因，不跳过 hooks 或删除测试。

## Git

- 开始实质改动前检查适用规则、当前分支、工作区、暂存区、未跟踪文件、分支起点和近期提交历史。
- 边界明确且改动较大的独立功能性修改才需要创建 `<type>/<topic>` 主题分支。
- 主题分支的 `type` 使用 `feat`、`fix`、`refactor`、`docs`、`test` 或 `chore`。
- 一个 commit 只有一个可独立回滚的逻辑目的。提交信息使用 `<type>(<scope>): <简洁中文动作>`。
- 只暂存当前 commit 的明确文件或 hunks；提交前检查 staged diff 和 `git diff --check`。
- 未经用户明确授权，不得 commit、push、merge、amend、rebase、squash、force push、跳过 hooks、删除分支或丢弃工作区内容。
- PR 以边界明确、可整体验收的特定功能更新为单位创建，可以包含完成该功能所需的规范、代码、测试、文档及同一功能范围内的小缺陷修复；不按发布版本聚合无关功能，也不机械地为一个版本或单个小缺陷各建一个 PR。
- 主题分支通过 GitHub Pull Request 的 squash merge 合入 `main`，不使用 merge commit。
- Release 说明固定使用 `## 亮点`、`## 新增`、`## 改进`、`## 修复`、`## 文档`、`## 其他`；`## 亮点` 下每项使用 `###` 标题和描述，其余分类中的每个条目末尾必须补充指向对应 GitHub Pull Request 的 PR 编号超链接，格式为 `([#12](https://github.com/<owner>/<repo>/pull/12))`；空分类保留标题。

## 文档与规范

- README 面向使用者，不是规范来源；demo 完成后再补全功能、使用说明和声明。
- 产品级约束分别写入 `docs/product/requirements.md`、`architecture.md` 和 `roadmap.md`。
- 局部设计写入 `docs/subsystems/<subsystem>.md`，不得在多个文档重复定义同一规范。
- Git 保存历史，不维护手工文档变更日志。
- 跨进程协议、持久化格式、游戏时钟语义、真实输入、执行解锁与急停，以及架构边界变化，必须先创建 feature spec。
- feature spec 固定包含 `requirements.md`、`design.md`、`tasks.md`；规范完成后即可开始编码，不设置独立审查门槛。
- 规范文档使用 YAML front matter，字段为 `status`、`scope`、`depends_on`；状态只使用 `draft`、`approved`、`implemented`、`superseded`。
- 需求编号使用 `REQ-<SCOPE>-###`，验收编号使用 `AC-<FEATURE>-###`；删除后的编号不得复用。
- “必须/应该/可以”分别表示强制、默认和可选；每条“必须”要求必须关联可验证的验收标准。
- 明确区分事实、决定、假设和未知项。结论必须能追溯到代码、版本化资料或实机证据。
- 规范图表只使用 Mermaid 或纯文本字符图。

## 代码公约

- 同帧操作使用明确且稳定的顺序，不依赖 `HashMap` 遍历顺序。
- 外部命令、内部调度项、执行请求和只读运行记录使用不同类型，不建设会隐式改变顺序的通用事件总线。
- 信任边界必须验证输入并返回结构化错误。
- 不得把凭据、令牌、Cookie、私钥或个人敏感信息写入代码、日志、文档和提交。
