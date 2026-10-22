# Windows 存储后端（待实现）

`wex-access-store` 的 `Store` 目前只在 Unix 上可用，原因是四处平台语义：

| 能力 | Unix 做法 | Windows 需要的等价物 | 状态 |
|---|---|---|---|
| 权限根目录 | `DirBuilder::mode(0o700)` | 显式设置只允许当前用户的 DACL | **未实现** |
| 私密文件 | `OpenOptionsExt::mode(0o600)` | 文件级 DACL | **未实现** |
| 根权限校验 | `permissions().mode() & 0o077 != 0` → 拒绝 | 读取 DACL，拒绝授予 `Everyone`/`Users`/`Authenticated Users` 的 ACE | **未实现** |
| 原子替换 | 写临时文件 → `fs::rename` → 目录 `sync_all` | `MoveFileExW(MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` | **未实现** |

## 为什么没有顺手"近似实现"

删掉 `cfg(unix)` 判断、让文件以默认 ACL 创建，会让"只有当前用户可读写"这条性质**在没有提示的情况下失效**，而它正是 `Store` 保存授权记录的前提。按 WEX 的规则（不得为跨平台削弱现有安全语义），这属于必须显式决策的变更。

## 两个可选方案

**A. 显式 DACL（推荐）**

建目录/文件时用 `SetNamedSecurityInfoW` 写入只含当前用户 SID 的 DACL；校验时用 `GetNamedSecurityInfoW` 枚举 ACE，存在 `Everyone` / `Users` / `Authenticated Users` 的允许项即失败关闭。

- 与 Unix `0700/0600` 语义等价，校验可测
- 约 150–200 行 FFI，需要 ACL 测试

**B. 依赖继承 ACL + 位置约束**

要求 `Store` 根位于 `%LOCALAPPDATA%` 之下，只校验没有宽泛 ACE，不主动设置 DACL。

- 改动小，但语义弱于 Unix；根放到公共目录时只能拒绝

## 原子替换

两种方案都需要实现，且不能用 `fs::rename`（Windows 上目标存在时失败）：

```
写 grants.tmp → FlushFileBuffers → MoveFileExW(tmp, grants.json, REPLACE_EXISTING | WRITE_THROUGH)
```

`MOVEFILE_WRITE_THROUGH` 只保证重命名元数据落盘，**不等价于** Unix 的目录 fsync；上层契约应明确写"Windows 崩溃一致性弱于 Unix"。
