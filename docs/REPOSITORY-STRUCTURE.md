# WEXPC 仓库结构与代码状态

状态：工程目录说明。产品范围以 [PC 基线](PC-PRODUCT-AND-TECH-BASELINE.md) 为准；目录存在不证明功能完成。

## 当前桌面架构与依赖

| 位置 | 真实职责 | 依赖边界 |
| --- | --- | --- |
| `apps/desktop/src` | React 页面、草稿、选择与展示 | 经 IPC 发意图，不独立写项目数据库或裁决授权 |
| `apps/desktop/src-tauri/src` | 桌面组合入口、命令传输、Windows 凭证库 | 注入服务及平台能力，不复制业务规则 |
| `crates/wex-desktop-service/src/domain` | 工作对象、Space/Group/Context/位置、项目/成员/任务 DTO | 不调用网络、数据库或平台 API |
| `crates/wex-desktop-service/src/application` | 项目和组织用例、关联校验、连接选择、对话生命周期与恢复 | 复用 `wex-core`；调用连接器和唯一存储入口 |
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
| `docs/DEVELOPMENT-WORKFLOW.md`、`AGENTS.md` | 当前开发和 GitHub 操作 | 属工程 D 域，不定义产品内任务/施工卡机制 |

`apps/desktop` 已建立 Tauri/React 源码与 Rust 服务接线，覆盖项目保存、AI 成员绑定、任务草案和普通 API 对话。Windows CI 已通过 Host 编译检查、服务测试和前端构建；这不证明桌面窗口、凭证库或真实服务已经实机运行。实际入口、对应提交证据和剩余要求见 [桌面说明](../apps/desktop/README.md)。

Space 基础批增加可操作组织入口：创建空间、可选分组和 Context，保持对象身份地移动组织位置，显式关联/解除项目 Context。通用 Context 不强制有项目、仓库或 AI 成员；当前只有一个主要位置，以独立 Placement 表达，未来可迁移。共享、权限继承、删除/级联、完整 Context 对话绑定和最终 UI 尚未交付。

结构调整只把旧 Flutter 工程从 `apps/wex` 归入 `reference/flutter-android`，并同步 CI 工作目录。Rust crate 与 Bridge 未迁移，以保持 Cargo workspace 路径及现有测试契约；后续修改它们要先审接口、调用方和 Windows 运行路径。
