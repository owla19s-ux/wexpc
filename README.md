# wexpc

WEX 的 PC（Windows）平台实现仓库。**与 `WEX` 仓库分离，不合并**；核心代码由 `WEX` 快照而来，本仓库只负责 Windows 平台后端与 PC 侧验证。

## 为什么单独一个仓库

`WEX` 以 Android 为第一交付平台。PC 需要的不是"第二套核心"，而是**同一套语义之上的第二个平台后端**：

```
Flutter UI / Dart client
        ↓ C ABI (bridge/flutter-rust)
Rust core (平台无关)          ← 与 WEX 保持一致
        ↓ platform backend
Windows 文件系统实现
```

## 平台后端契约

`crates/wex-local-resources/src/platform/` 下的每个后端必须同时满足：

| 契约 | Unix/Linux/Android | Windows |
|---|---|---|
| 目录 pin | `open(O_DIRECTORY \| O_NOFOLLOW)` 句柄 | `CreateFileW` + `FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT` |
| 相对打开 | `openat(dirfd, leaf, O_PATH \| O_NOFOLLOW)` | `NtCreateFile` + `OBJECT_ATTRIBUTES::RootDirectory` |
| 打开后重读 | `/proc/self/fd/<fd>` | 直接读已 pin 的句柄（同一对象） |
| 对象身份 | `dev` + `ino` | 卷序列号 + 文件索引 |
| 挂载/卷 | `/proc/self/fdinfo` 的 `mnt_id` | 卷序列号比对 |
| 链接拒绝 | `O_NOFOLLOW` + `nlink == 1` | `FILE_OPEN_REPARSE_POINT` + `nNumberOfLinks == 1` |
| 单分量语法 | 拒绝 `/` | 拒绝 `/ \ : < > " \| ? *`、结尾空格/句点、ADS |
| 大小上限 | 64 KiB | 64 KiB |
| 随机 ID | `/dev/urandom` | `BCryptGenRandom` |

任何后端都不得以"解析用户路径"作为准入方式。

## 当前状态

| 项 | 状态 |
|---|---|
| `wex-local-resources` Windows 后端 | 已实现，9 项契约测试通过 |
| Windows 全量 `cargo test` | 通过（核心 55 项 + 本 crate 9 项） |
| Linux 目标编译 | 通过（`cargo check --target x86_64-unknown-linux-gnu`） |
| 真机/真实文件系统边界验收 | **未做** |
| `ReadSession` 平台判定 | 改为能力查询 `platform_supported()`，不再写死 `cfg!` |
| `wex-access-store` 私密 ACL + 原子替换 | 已实现（SDDL `D:P(A;;FA;;;<SID>)` + `MoveFileExW`），见 `docs/PLATFORM-STORAGE.md` |
| `bridge/flutter-rust` `local_read` ABI | **未做** |

## 来源

初始快照来自 `WEX` 仓库 commit `8e92c1d`（`crates/`、`bridge/flutter-rust/`、`Cargo.toml`、`Cargo.lock`）。
