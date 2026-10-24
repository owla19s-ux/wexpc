# WEXPC 仓库结构与代码状态

状态：工程目录说明。产品范围以 [PC 基线](PC-PRODUCT-AND-TECH-BASELINE.md) 为准；目录存在不证明功能完成。

## 当前桌面架构与依赖

| 位置 | 真实职责 | 依赖边界 |
| --- | --- | --- |
| `apps/desktop/src` | React 页面、草稿、选择与展示 | 经 IPC 发意图，不独立写项目数据库或裁决授权 |
| `apps/desktop/src-tauri/src` | 桌面组合入口、命令传输、Windows 凭证库 | 注入服务及平台能力，不复制业务规则 |
| `crates/wex-desktop-service/src/domain` | 工作对象、Space/Group/Context/位置、项目/成员/任务 DTO | 不调用网络、数据库或平台 API |
| `crates/wex-desktop-service/src/application` | 项目和组织用例、任务接续记录、成员工作资料、关联校验、连接选择、对话生命周期与恢复 | 复用 `wex-core`；调用连接器和唯一存储入口 |
| `crates/wex-desktop-service/src/infrastructure/sqlite.rs` | SQLite 事务、应用实例锁、schema 迁移和快照 I/O | 不发网络请求，不赋予组织关系访问权限 |
| `crates/wex-desktop-service/src/connectors` | 现有 OpenAI 兼容 HTTP 文本执行器 | 不写项目业务状态或决定任务完成 |
| `crates/wex-core` 与资源/授权后端 | 已有连接/权限/操作规则和 Windows 安全资源实现 | 按实际需要接入；不因保留代码就宣称所有工具已可用 |

当前按真实职责拆模块，未为远景创建空 crate；未来只有独立复用、依赖隔离或测试需要才提取新 crate。旧 `wex-workspace-store` 保留参考与测试，不与 PC SQLite 双写同一组织事实。`bridge/flutter-rust` 与 Flutter 资产继续作为历史平台实现，不进入桌面正式 IPC 链。

| 路径 | 当前用途 | 处理原则 |
| --- | --- | --- |
| `crates/wex-core`、`wex-protocol` 等 | 迁入的 Rust 领域、协议与平台相关实现 | 按 PC 工作对象、长期任务、权限和 Windows 行为逐项审查；不因 crate 名称而认定 Dispatcher 已完成 |
| `crates/wex-local-resources`、`wex-access-store` | Windows 文件及授权存储后端 | 保留契约测试，补真实环境与失败恢复验证 |
| `crates/wex-workspace-store` | 旧 Space/Group/Context 组织存储 | “workspace” 是历史包名，不可倒推 PC 产品术语或完整对象模型 |
| `crates/wex-desktop-service` | PC 新业务的 Rust/SQLite 唯一写入口、项目/成员/任务草案和普通 API 文本对话 | 实际复用 Catalog 规则；不双写旧配置，不等于完整并发施工引擎 |
| `apps/desktop` | Tauri 2 Host 与 React/TypeScript 基础桌面 UI | 已有正式命令接线；Windows 构建与实机、完整工具链仍待验证 |
| `bridge/flutter-rust` | 旧 Flutter FFI，仍属于现有 Cargo workspace | 保留可编译参考；未来桌面 Host 的边界单独设计，不能直接当作 Tauri 接线 |
| `reference/flutter-android` | 从 WEX 迁入的 Flutter/Dart UI、连接器与测试快照 | 历史参考，不是 PC 桌面应用或最终 UI；CI 的 Flutter job 只验证此参考资产 |
| `.github/workflows/rust.yml` | Rust 与参考 Flutter 检查 | Windows job 只验证现有后端；没有桌面安装包/实机验收 |
| `docs/PC-PRODUCT-AND-TECH-BASELINE.md` | 当前产品与技术主基线 | 0.1.1 首版结果与建议施工批次、未验证选型分开记录 |
| `docs/DEVELOPMENT-WORKFLOW.md`、`AGENTS.md` | 当前开发、双环境分工、共同日志与交接/裁决规则 | 属工程D域；PR #4讨论区只追加实际记录，不等同产品内施工卡/范围锁 |
| `docs/INTERFACE-CONTRACTS.md` | 可编码契约正文、版本/采用与替代关系 | 当前API-PC-01为DRAFT-0范围登记，未形成实现接口；不把文档当模块完成 |

`apps/desktop` 已建立 Tauri/React 源码与 Rust 服务接线，覆盖项目保存、AI 成员绑定、任务草案和普通 API 对话。Windows CI 已通过 Host 编译检查、服务测试和前端构建；这不证明桌面窗口、凭证库或真实服务已经实机运行。实际入口、对应提交证据和剩余要求见 [桌面说明](../apps/desktop/README.md)。

Space 基础批增加可操作组织入口：创建空间、可选分组和 Context，保持对象身份地移动组织位置，显式关联/解除项目 Context。通用 Context 不强制有项目、仓库或 AI 成员；当前只有一个主要位置，以独立 Placement 表达，未来可迁移。项目和普通对话可分别绑定 Context；共享、权限继承、删除/级联和最终 UI 尚未交付。

结构调整只把旧 Flutter 工程从 `apps/wex` 归入 `reference/flutter-android`，并同步 CI 工作目录。Rust crate 与 Bridge 未迁移，以保持 Cargo workspace 路径及现有测试契约；后续修改它们要先审接口、调用方和 Windows 运行路径。


任务接续记录由独立 `domain/task_records.rs` 与 `application/task_records.rs` 保存和追加，通过同一 SQLite 入口持久化（schema 4）。它是用户手动资料，保留当时任务快照；不等同于 Core Operation 的可信执行回执，不新增权限或替代审查/验证状态。

成员工作资料由 `application/member_brief.rs` 统一组装供 Tauri 预览和正式对话请求使用：主 AI 读取本项目任务/团队/记录，其余成员读取当前分配任务/记录；不携带凭证或其他成员会话。它是只读投影，未另建业务存储或替代 Core 的请求校验。

## API 复用与 DeepSeek 施工交接（2026-10-24）

用户确认：重视 WEX Core，API 可直接复用则复用；不能直接复用的 PC API 代码交给 DeepSeek 编写。本轮只做定向源码核对，不继续全量审查 Android 资产。

核对来源为 WEX PR #72 的提交 `aa9044584977e9833be27e5f00c25fe37b528642`，不是仅看 main 或本仓旧快照：
- `apps/wex/lib/extensions/android_connectors/ai_extension.dart`：依赖 Dart `dart_openai`/HTTP；有文本及工具流聚合、终态校验、错误脱敏、禁止重定向、DeepSeek reasoning 续传。
- `apps/wex/lib/extensions/android_connectors/runtime_conversation_controller.dart`：依赖 Flutter 状态通知、Android 安全存储及执行桥；有取消保留部分文字、连接变更复核、工具回执与模型历史区分。
- PR 的最新改动并非全部已验证，不能照搬历史通过数字作为当前源码证据。以上是定向源码检查，未对该 Android 提交运行测试。

| 内容 | PC 处理 |
| --- | --- |
| Rust Core Catalog 连接/模型选择与有效性规则 | 当前桌面服务已经直接复用，继续作为唯一规则来源 |
| Core Operation/Dispatcher、授权与真实回执 | 保留并按工具用例接入；API 模块只产出调用意图，不执行或授权工具 |
| 现有 PC HTTP 文本连接器 | 保留 URL/HTTPS、凭证隔离、响应上限、错误脱敏和无自动重试边界；当前仅非流式文本，不是完整 API 模块 |
| Android API 终态、工具参数、取消、reasoning 续传规则及反例 | 作为 Rust 实现与验收参考；需要移植和重新验证，不宣称 Dart 代码直接复用 |
| Flutter 控制器、Android Vault/FFI 与旧 SDK 源码快照 | 不作为 PC 运行依赖迁入，不另建一套连接目录、授权库或会话存储 |

### DeepSeek 待领取任务 API-PC-01

状态：复用范围与本批验收已梳理；可编码接口、独立 `crates/wex-api` 起点及 `feat/api-protocol` 分支尚未建立，尚未正式派发。大肥鱼可先配置手机工具链，环境结果待其实际报告。Work当前未发现可直接调用DeepSeek的入口/凭证；大肥鱼在独立手机环境施工与Work直接调用DeepSeek是两件事，不得互相冒称。具体双环境交接以[工程操作](DEVELOPMENT-WORKFLOW.md)为准。

目标：从现有Rust连接器与相关规则抽取可在手机独立验证的API模块，补齐 OpenAI-compatible/DeepSeek Chat Completions 的流式协议执行能力；先维护正式协议适配层，不把 SDK 依赖存在当作完整接入。完整 Provider/模型参数、Responses 等仍属产品目标，另批实现。

施工所有权（本次双环境安排替代旧目录分工）：正式交接后，大肥鱼独占 `crates/wex-api/**` 的API实现和模块测试；Work负责现有桌面 `connectors` 薄适配、Core/应用状态、Host/Vault/UI、共享工作区与锁文件。Work先准备可运行起点，交接后不并发改鱼所持目录。API只产出协议事件/调用意图，不执行或授权工具；依赖/接口变更提交清单由所有者串行整合。当前不存在独立crate，不把目录规划当作代码完成。

输入：当前 `connectors/openai_compatible.rs`、`application` 的请求/完成接口、上述固定提交的两个 Android 文件与 `ai_extension_stream_test.dart`、`ai_extension_test.dart`。只参考相关 API 用例，不迁入整个 Android 工程。

交付要求：
1. 明确请求、文本增量、完整工具调用、正常结束和失败事件的类型边界。工具调用输出只交给应用层，不直接触发网络外部写操作。
2. SSE 支持跨字节/UTF-8 分片与换行边界；按 choice/tool index 聚合；严格验证角色、唯一调用 ID、函数名、JSON 对象参数与 finish_reason；EOF/[DONE] 本身不能证明成功。
3. 取消或流中断保留已收到文本，不能把部分输出当作完整答案进入重放历史。DeepSeek reasoning 续传按连接、会话和本轮完整工具 ID 集隔离，禁止跨会话串用。
4. 延续禁止凭证请求重定向、响应/帧/缓冲上限、超时及脱敏。认证、429、服务端/协议错误不回显响应正文，不自动重试生成或工具写入。
5. 模拟服务验证正常文本/工具、多片参数、usage 尾帧、重复/冲突终态、无终态、非法参数、200 内错误、断流、取消、超限、连接隔离与无重定向。连接停用后的落库/派发复核由应用层保持权威，不能由连接器缓存绕过。
6. 大肥鱼先在手机提供模块实际测试证据；Work整合后本地验证Rust/fmt/clippy/前端，当前不能运行的Windows Host/平台验证按Commit登记，11月1日以后再评估云端检查。真实DeepSeek、Windows运行与模拟器分别记录；模拟HTTP测试不替代真实服务。

此交接不阻塞 Space/Context 和轻项目管理的独立施工；API 实际执行者接入前，不擅自把指定给 DeepSeek 的新增实现改交其他模型。
