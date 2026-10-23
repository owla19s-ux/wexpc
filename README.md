# WEXPC

WEX 的 **PC 优先工程仓库**，首期交付目标为 Windows。它与 `WEX` 主仓库保持分离；当前包含从 WEX 快照继承的 Rust 核心、Windows 平台后端和既有 Flutter/Dart 参考实现。

> **当前产品方向已调整：PC 优先。** Android 是架构、功能、业务规则和已验证实现的参考来源，不再定义产品交付顺序，也不约束 PC UI 技术选型。PC 产品、功能和技术权威基线见 [PC 产品、功能与技术基线](docs/PC-PRODUCT-AND-TECH-BASELINE.md)。

## 仓库职责

- 以 PC/Windows 为第一交付目标，验证并建设桌面端所需的本地文件、进程、Git、连接器和安全能力。
- 将 WEX 中已确认的通用业务语义与安全规则迁移到可复用的 Rust 核心；逐项记录直接复用、适配、重构、替换或待验证。
- 现有 Flutter/Dart 页面与客户端代码是历史快照和业务逻辑参考，不是 PC 最终 UI 的既定实现。
- 本仓库目前**不是完整 PC 产品已交付的证明**；Windows 后端契约测试通过不等于 PC 桌面应用、完整 AI 工作链路或真实环境验收通过。

## 当前 Windows 平台后端

`crates/wex-local-resources/src/platform/` 下的平台后端需要满足：

| 契约 | Unix/Linux/Android | Windows |
|---|---|---|
| 目录 pin | `open(O_DIRECTORY \| O_NOFOLLOW)` 句柄 | `CreateFileW` + `FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT` |
| 相对打开 | `openat(dirfd, leaf, O_PATH \| O_NOFOLLOW)` | `NtCreateFile` + `OBJECT_ATTRIBUTES::RootDirectory` |
| 打开后重读 | `/proc/self/fd/<fd>` | 直接读已 pin 的句柄（同一对象） |
| 对象身份 | `dev` + `ino` | 卷序列号 + 文件索引 |
| 挂载/卷 | `/proc/self/fdinfo` 的 `mnt_id` | 卷序列号比对 |
| 链接拒绝 | `O_NOFOLLOW` + `nlink == 1` | `FILE_OPEN_REPARSE_POINT` + `nNumberOfLinks == 1` |
| 单分量语法 | 拒绝 `/` | 拒绝 `/ \\ : < > " \| ? *`、结尾空格/句点、ADS |
| 大小上限 | 64 KiB | 64 KiB |
| 随机 ID | `/dev/urandom` | `BCryptGenRandom` |

任何后端都不得以“解析用户路径”作为准入方式。

## 当前实现与验证状态

| 项 | 状态 |
|---|---|
| `wex-local-resources` Windows 后端 | 已实现，9 项契约测试通过 |
| Windows 全量 `cargo test` | 已通过（核心 55 项 + 本 crate 9 项，按既有记录） |
| Linux 目标编译 | 已通过（`cargo check --target x86_64-unknown-linux-gnu`，按既有记录） |
| `wex-access-store` 私密 ACL + 原子替换 | 已实现，见 [平台存储说明](docs/PLATFORM-STORAGE.md) |
| `bridge/flutter-rust` `local_read` ABI | 已实现，7 项 ABI 契约测试在 Windows 通过（按既有记录） |
| 完整 PC UI 与桌面安装包 | 尚不能由上述后端测试证明 |
| 真实文件系统边界、用户目录权限与故障恢复验收 | 尚未完成 |
| AI API、GitHub、本地 Git、构建/测试的完整 PC 闭环 | 尚不能由当前状态表证明 |

## PC 交付阶段

- **PC-0：技术验证** — 验证 Tauri 2 + React + TypeScript + Rust、画布持久化、SQLite、AI API 流式响应、受限目录访问、受控进程生命周期和 Windows 打包。
- **PC-1：首个可用版本** — 统一工作对象与关系、自由画布、多视图、AI 秘书/API、本地 Git、GitHub 真实读写、构建/测试/打包和操作回执/核验。
- **PC-1.1：开发环境扩展** — 国内托管服务、远程开发、容器与 CI，逐个连接器单独验收。
- **PC-2：跨设备与更多工作模式** — 手机配对、选择性同步、手机遥控 PC、更多领域工作模式。

阶段标签是计划，不是发布日期承诺。只有通过 PC-0 的关键验证后，才能冻结 PC-1 最终范围。

## 来源与关系

初始代码快照来自 WEX 仓库历史提交 `8e92c1d`（`crates/`、`bridge/flutter-rust/`、`Cargo.toml`、`Cargo.lock`）。两个仓库继续分离；不得因代码相似就假定两边运行时、交付范围或验收状态相同。
