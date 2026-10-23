# WEXPC 仓库结构与代码状态

状态：工程目录说明。产品范围以 [PC 基线](PC-PRODUCT-AND-TECH-BASELINE.md) 为准；目录存在不证明功能完成。

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

结构调整只把旧 Flutter 工程从 `apps/wex` 归入 `reference/flutter-android`，并同步 CI 工作目录。Rust crate 与 Bridge 未迁移，以保持 Cargo workspace 路径及现有测试契约；后续修改它们要先审接口、调用方和 Windows 运行路径。
